import assert from 'node:assert/strict'
import test from 'node:test'
import { AvatarBotMap, AvatarMoods } from './fork-avatar-mood.ts'
import { cachedBots, isCurrentMirrorSchema, MIRROR_SCHEMA, mirrorCacheStamp } from './fork-mirror-cache.ts'
import { isActive, transitions, type IslandBot } from './island.ts'
import { isUnavailable, isWorking, isWorkingIn, needsInput, normalizeBot, normalizeScreen, screenAgent, screenUseSubtitle, type Bot, type ScreenState } from './models.ts'

// runtime-audiencia-01: o Hub manda `unavailable` quando o runtime não tem audiência neste Space.
const leftovers = { activity: 'Editando loja.ts', startedAt: 5, resumeAt: 6, workingChat: 'outra' }

test('normalizador: indisponível, desconhecido ou ausente nunca vira Livre e chega sem runtime', () => {
  for (const status of ['unavailable', 'paused', undefined]) {
    const bot = normalizeBot({ id: 'donna', name: 'Donna', unread: 2, lastMessage: 'oi', lastAt: 9, ...leftovers, ...(status ? { status: status as Bot['status'] } : {}) })
    assert.equal(bot.status, 'unavailable', String(status))
    assert.deepEqual([bot.activity, bot.startedAt, bot.resumeAt, bot.workingChat], ['', null, null, null])
    assert.deepEqual([bot.name, bot.unread, bot.lastMessage, bot.lastAt], ['Donna', 2, 'oi', 9], 'o autorizado fica')
    assert.ok(isUnavailable(bot) && !isWorking(bot) && !needsInput(bot) && !isWorkingIn(bot, 'donna'))
  }
  const working = normalizeBot({ id: 'a', status: 'working', ...leftovers })
  assert.deepEqual([working.status, working.activity, working.startedAt, working.workingChat], ['working', 'Editando loja.ts', 5, 'outra'])
})

test('cache anterior a 13 não conserva runtime nem diz Livre; o schema atual conserva o autorizado', () => {
  for (const schema of [10, 11, 12]) {
    for (const status of ['idle', 'working'] as const) {
      const cached = normalizeBot({ id: 'donna', status, unread: 1, lastMessage: 'oi', computer: true, connectors: ['ligado'], ...leftovers })
      const stamp = `donnabot-mirror-v${schema}/1.0/1.0`
      assert.equal(isCurrentMirrorSchema(stamp), false)
      const [old] = cachedBots(stamp, [cached])
      assert.deepEqual([old!.status, old!.activity, old!.startedAt, old!.resumeAt, old!.workingChat], ['unavailable', '', null, null, null], `${schema}/${status}`)
      assert.deepEqual([old!.unread, old!.lastMessage, old!.computer, old!.connectors], [1, 'oi', true, ['ligado']], 'perfil e prévia autorizados ficam')
      assert.equal(cached.status, status, 'não altera o objeto de origem')
    }
  }
  const cached = normalizeBot({ id: 'donna', status: 'working', ...leftovers })
  assert.equal(cachedBots(undefined, [cached])[0]!.status, 'unavailable', 'sem carimbo também')
  const stamp = mirrorCacheStamp('1.0', '1.0')
  assert.equal(stamp, `donnabot-mirror-v${MIRROR_SCHEMA}/1.0/1.0`)
  assert.equal(isCurrentMirrorSchema(stamp), true)
  assert.equal(isCurrentMirrorSchema('donnabot-mirror-v30/1.0/1.0'), false, 'cache anterior não contém principal próprio do Space')
  assert.equal(isCurrentMirrorSchema('donnabot-mirror-v31/1.0/1.0'), false, 'cache do #629 ainda não contém principal próprio do Space')
  assert.equal(isCurrentMirrorSchema('donnabot-mirror-v28/1.0/1.0'), false, 'cache da main anterior não contém o cartão D511')
  assert.equal(cachedBots(stamp, [cached])[0]!.status, 'working')
})

test('trabalhando → indisponível: rosto para sem concluído; ilha sem Pronto e sem contar', () => {
  const moods = new AvatarMoods()
  const bots = new AvatarBotMap(moods)
  const working = normalizeBot({ id: 'donna', status: 'working', ...leftovers })
  bots.set('donna', working)
  assert.equal(moods.get('donna').base, 'think')
  bots.set('donna', normalizeBot({ ...working, status: 'unavailable' }))
  assert.equal(moods.get('donna').base, 'idle')
  assert.equal(moods.get('donna').action, null, 'sem reação de concluído ou erro')
  bots.set('donna', normalizeBot({ ...working, status: 'idle' }))
  assert.equal(moods.get('donna').action, null, 'o que vem depois também não conclui')

  const island = (status: Bot['status']): IslandBot => ({ ref: 'c/donna', name: 'Donna', shape: 'blob', color: 'blue', status, working: status === 'working', backend: 'Claude Code', folder: '', detail: '', steps: [], error: null })
  assert.ok(!isActive(island('unavailable')))
  assert.deepEqual(transitions(new Map([['c/donna', 'working']]), [island('unavailable')]), [])
  assert.deepEqual(transitions(new Map([['c/donna', 'needsInput']]), [island('unavailable')]), [])
  assert.deepEqual(transitions(new Map([['c/donna', 'unavailable']]), [island('idle')]), [])
  assert.deepEqual(transitions(new Map([['c/donna', 'unavailable']]), [island('error')]), [])
})

test('Screen (R1): só `using` com ID mostra quem usa; ausente, desconhecido ou contraditório é indisponível', () => {
  const base = { enabled: true, connected: true, platform: 'macos', capture: true, input: true, displays: [], userControl: false, computerUse: true, viewers: 0 }
  const use = (raw: Record<string, unknown>) => { const s = normalizeScreen({ ...base, ...raw })!; return [s.agentUse, s.agentBot, screenAgent(s)] }
  assert.deepEqual(use({ agentUse: 'using', agentBot: 'donna' }), ['using', 'donna', 'donna'], 'Loja: quem usa, autorizado')
  assert.deepEqual(use({ agentUse: 'unavailable', agentBot: null }), ['unavailable', null, null], 'Pessoal: Donna usando na Loja')
  assert.deepEqual(use({ agentUse: 'idle', agentBot: null }), ['idle', null, null])
  for (const raw of [{ agentBot: 'donna' }, {}, { agentUse: 'paused', agentBot: 'donna' }, { agentUse: 'unavailable', agentBot: 'donna' }, { agentUse: 'using', agentBot: null }, { agentUse: 'idle', agentBot: 'donna' }]) {
    assert.deepEqual(use(raw), ['unavailable', null, null], JSON.stringify(raw))
  }
  assert.equal(screenAgent(null), null)
})

// R1 (decisão A): uso por bot desconhecido vem antes da contagem de quem vê e de Ready, na bandeja.
test('bandeja: indisponível nunca vira contagem, nome ou Ready; nomes e contagem só com uso conhecido', () => {
  const tela = (agentUse: ScreenState['agentUse'], agentBot: string | null, viewers = 0): ScreenState => ({ enabled: true, connected: true, platform: 'macos', userControl: false, computerUse: true, capture: true, input: true, displays: [{ id: 1, name: 'Tela principal', main: true }] as ScreenState['displays'], agentUse, agentBot, viewers })
  const nome = (id: string) => (id === 'scout' ? 'Scout' : undefined)
  for (const viewers of [0, 1, 3]) assert.equal(screenUseSubtitle(tela('unavailable', null, viewers), nome), 'Uso do computador indisponível', `viewers ${viewers}`)
  assert.equal(screenUseSubtitle(tela('using', 'outro', 2), nome), 'Uso do computador indisponível', 'bot fora desta lista')
  assert.equal(screenUseSubtitle(tela('using', 'scout'), nome), 'Scout está usando')
  assert.equal(screenUseSubtitle(tela('using', 'scout', 1), nome), 'Seu celular está vendo')
  assert.equal(screenUseSubtitle(tela('idle', null, 2), nome), '2 aparelhos vendo')
  assert.equal(screenUseSubtitle(tela('idle', null), nome), 'Pronto · Tela principal')
})

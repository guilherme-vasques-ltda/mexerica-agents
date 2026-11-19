import assert from 'node:assert/strict'
import test from 'node:test'
import { FIXED_ACTIVITY_KINDS, parseActivityCode } from './fork-activity-code.ts'
import { cachedBots, isCurrentMirrorSchema, MIRROR_SCHEMA, mirrorCacheStamp } from './fork-mirror-cache.ts'
import { normalizeBot, unavailableRuntime } from './models.ts'
import { parseTowerSnapshot } from './fork-torre.ts'

// MX-147: `activity` legado continua; `activityCode` é opcional e nunca derruba o bot.

test('códigos conhecidos: kinds fixos, needs_approval com title e downloading com item', () => {
  for (const kind of FIXED_ACTIVITY_KINDS) assert.deepEqual(parseActivityCode({ kind }), { kind })
  assert.deepEqual(parseActivityCode({ kind: 'needs_approval', title: 'Edit src/pace.js' }), { kind: 'needs_approval', title: 'Edit src/pace.js' })
  assert.deepEqual(parseActivityCode({ kind: 'downloading', item: 'extrato.pdf' }), { kind: 'downloading', item: 'extrato.pdf' })
  assert.deepEqual(parseActivityCode({ kind: 'installing', item: 'Node.js' }), { kind: 'installing', item: 'Node.js' })
  assert.deepEqual(parseActivityCode({ kind: 'tool', title: 'Edit src/a.ts' }), { kind: 'tool', title: 'Edit src/a.ts' })
  // Num grupo, `member` opcional acompanha qualquer kind; ausente ou null não aparece.
  assert.deepEqual(parseActivityCode({ kind: 'tool', title: 'x', member: 'Byte' }), { kind: 'tool', title: 'x', member: 'Byte' })
  assert.deepEqual(parseActivityCode({ kind: 'thinking', member: 'Byte' }), { kind: 'thinking', member: 'Byte' })
  assert.deepEqual(parseActivityCode({ kind: 'thinking', member: null }), { kind: 'thinking' })
  // Campo extra de um Hub mais novo é ignorado, não copiado.
  assert.deepEqual(parseActivityCode({ kind: 'typing', novo: 1 }), { kind: 'typing' })
})

test('desconhecido, ausente e malformado viram null (quem mostra usa activity)', () => {
  const malformed: unknown[] = [
    undefined, null, 'typing', 7, true, [], [{ kind: 'typing' }], {}, { kind: null }, { kind: 3 }, { kind: '' }, { kind: 'Typing' }, { kind: 'flying' },
    { kind: '__proto__' }, { kind: 'constructor' }, { kind: 'toString' },
    { kind: 'needs_approval' }, { kind: 'needs_approval', title: '' }, { kind: 'needs_approval', title: '   ' }, { kind: 'needs_approval', title: 3 }, { kind: 'needs_approval', title: { x: 1 } },
    { kind: 'installing' }, { kind: 'tool', title: '' }, { kind: 'tool', item: 'x' },
    { kind: 'thinking', member: '' }, { kind: 'thinking', member: ' ' }, { kind: 'thinking', member: 7 }, { kind: 'tool', title: 'x', member: {} },
    { kind: 'downloading' }, { kind: 'downloading', item: null }, { kind: 'downloading', item: ['a'] }, { kind: 'downloading', title: 'x' },
  ]
  for (const raw of malformed) assert.equal(parseActivityCode(raw), null, JSON.stringify(raw))
})

test('compatibilidade: Hub antigo sem activityCode e Hub novo com código ou lixo', () => {
  const legado = normalizeBot({ id: 'a', status: 'working', activity: 'Writing a reply…' })
  assert.deepEqual([legado.activity, legado.activityCode], ['Writing a reply…', null])
  const novo = normalizeBot({ id: 'a', status: 'working', activity: 'Typing…', activityCode: { kind: 'typing' } as never })
  assert.deepEqual([novo.activity, novo.activityCode], ['Typing…', { kind: 'typing' }])
  const livre = normalizeBot({ id: 'a', status: 'working', activity: 'Editando loja.ts', activityCode: { kind: 'flying' } as never })
  assert.deepEqual([livre.activity, livre.activityCode], ['Editando loja.ts', null], 'atividade livre preservada')
  const lixo = normalizeBot({ id: 'a', status: 'needsInput', activity: 'Needs approval: Edit x', activityCode: 'needs_approval' as never })
  assert.deepEqual([lixo.activity, lixo.activityCode], ['Needs approval: Edit x', null])
  // Normalizar de novo (cache) é idempotente.
  assert.deepEqual(normalizeBot(novo).activityCode, { kind: 'typing' })
})

test('segurança de audiência: runtime indisponível apaga o código, mesmo com sobras do Hub ou do cache', () => {
  const sobras = { activity: 'Needs approval: Edit segredo.ts', activityCode: { kind: 'needs_approval', title: 'Edit segredo.ts' } as never }
  for (const status of ['unavailable', 'paused', undefined]) {
    const bot = normalizeBot({ id: 'donna', ...sobras, ...(status ? { status: status as never } : {}) })
    assert.deepEqual([bot.status, bot.activity, bot.activityCode], ['unavailable', '', null], String(status))
  }
  assert.equal(unavailableRuntime({ activityCode: { kind: 'typing' } }).activityCode, null)
  const autorizado = normalizeBot({ id: 'a', status: 'needsInput', ...sobras })
  assert.deepEqual(autorizado.activityCode, { kind: 'needs_approval', title: 'Edit segredo.ts' })
})

test(`MIRROR_SCHEMA 37+ (hoje ${MIRROR_SCHEMA}): cache anterior perde o runtime, inclusive o código; o atual o conserva`, () => {
  assert.equal(mirrorCacheStamp('1.0', '1.0'), `donnabot-mirror-v${MIRROR_SCHEMA}/1.0/1.0`)
  const cached = normalizeBot({ id: 'a', status: 'working', activity: 'Typing…', activityCode: { kind: 'typing' } as never })
  const antigo = `donnabot-mirror-v${MIRROR_SCHEMA - 1}/1.0/1.0`
  assert.equal(isCurrentMirrorSchema(antigo), false)
  const [velho] = cachedBots(antigo, [cached])
  assert.deepEqual([velho!.status, velho!.activity, velho!.activityCode], ['unavailable', '', null])
  const [atual] = cachedBots(mirrorCacheStamp('1.0', '1.0'), [cached])
  assert.deepEqual([atual!.status, atual!.activity, atual!.activityCode], ['working', 'Typing…', { kind: 'typing' }])
})

test('Torre: activityCode na projeção segue as mesmas regras e o texto legado fica', () => {
  const U = { availability: 'unavailable' }
  const work = (extra: Record<string, unknown>) => ({ availability: 'available', source: 's', coverage: 'c', value: { status: 'working', activity: 'Downloading a.pdf', startedAt: 5, resumeAt: null, reason: null, conversation: null, ...extra } })
  const bot = (botId: string, runtime: unknown) => ({ botId, name: botId, title: null, avatarColor: '', avatarShape: '', avatarConfig: null, runtime, open: null, pending: U, queue: U, usage: { dailyBotTokens: U, subscriptionLimit: U, contextTokens: U } })
  const snap = parseTowerSnapshot({ version: 1, hubId: 'h', snapshotId: 's', collectedAt: 1, spaceId: 'p', bots: [
    bot('a', work({ activityCode: { kind: 'downloading', item: 'a.pdf' } })), bot('b', work({})), bot('c', work({ activityCode: { kind: 'downloading' } })),
  ] }, { hubId: 'h', spaceId: 'p' })
  const codes = snap.bots.map((b) => (b.runtime.availability === 'available' ? b.runtime.value.activityCode : 'indisponível'))
  assert.deepEqual(codes, [{ kind: 'downloading', item: 'a.pdf' }, null, null])
  for (const b of snap.bots) assert.equal(b.runtime.availability === 'available' && b.runtime.value.activity, 'Downloading a.pdf')
})

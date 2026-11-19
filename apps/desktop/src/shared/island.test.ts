// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { isWorking, normalizeBot, type BotStatus } from './models.ts'
import {
  focusOf, hitRect, hits, islandFrame, islandMode, islandRect, janelaVista, LONG_TURN_MS, notchOf, observeTurns, othersOf, transitions, workingCount,
  type IslandBot, type IslandNotice, type IslandPermission, type IslandSummary, type IslandTab, type TurnSeen,
} from './island.ts'

const bot = (ref: string, status: BotStatus): IslandBot => ({
  ref, name: ref, shape: 'blob', color: 'blue', status, working: status === 'working', backend: 'Claude Code', folder: 'site', detail: '', steps: [], error: null,
})
const card = (entryId: string): IslandPermission => ({
  ref: 'c/a', entryId, bot: { name: 'a', shape: 'blob', color: 'blue', backend: 'Claude Code' }, headline: 'Wants to run a command', title: 'ls', options: [], answering: null,
})
const summary = (bots: IslandBot[], permissions: IslandPermission[] = []): IslandSummary => ({ bots, permissions, usage: [] })
const state = (over: Partial<{ open: boolean; peek: boolean; tab: IslandTab; deferred: ReadonlySet<string>; notice: IslandNotice | null }> = {}) =>
  ({ open: false, peek: false, tab: 'auto' as IslandTab, deferred: new Set<string>(), notice: null, ...over })

test('closed, the island hides, peeks under the pointer and stays thin while bots work', () => {
  assert.equal(islandMode(summary([bot('a', 'idle')]), state()), 'hidden')
  assert.equal(islandMode(summary([bot('a', 'idle')]), state({ peek: true })), 'peek')
  assert.equal(islandMode(summary([bot('a', 'working')]), state()), 'compact')
  assert.equal(islandMode(summary([bot('a', 'needsInput')]), state()), 'compact')
})

test('open, it shows what needs you first, then the bots', () => {
  assert.equal(islandMode(summary([bot('a', 'working')]), state({ open: true })), 'overview')
  assert.equal(islandMode(summary([bot('a', 'idle')]), state({ open: true })), 'empty')
  assert.equal(islandMode(summary([bot('a', 'idle')]), state({ open: true, notice: { kind: 'finished', ref: 'a' } })), 'finished')
  assert.equal(islandMode(summary([bot('a', 'idle')]), state({ open: true, tab: 'bots', notice: { kind: 'finished', ref: 'a' } })), 'empty')
  assert.equal(islandMode(summary([bot('a', 'working')]), state({ open: true, tab: 'message' })), 'message')
  assert.equal(islandMode(summary([bot('a', 'working')], [card('e1')]), state({ open: true, tab: 'usage' })), 'usage')
})

test('an approval opens the island until it is put off, and opening shows it again', () => {
  const s = summary([bot('a', 'needsInput')], [card('e1')])
  assert.equal(islandMode(s, state()), 'approval')
  assert.equal(islandMode(s, state({ deferred: new Set(['e1']) })), 'compact')
  assert.equal(islandMode(s, state({ open: true, deferred: new Set(['e1']) })), 'approval')
  assert.equal(islandMode(s, state({ open: true, tab: 'bots', deferred: new Set(['e1']) })), 'overview')
  // A new card arrives while the first is put off.
  assert.equal(islandMode(summary([], [card('e1'), card('e2')]), state({ deferred: new Set(['e1']) })), 'approval')
})

test('a bot that stops working is done, or failed when it ended in an error', () => {
  const before = new Map<string, string>([['a', 'working'], ['b', 'idle'], ['c', 'needsInput'], ['d', 'working']])
  const after = [
    { ...bot('a', 'idle'), duracaoMs: LONG_TURN_MS + 1000, watching: false },
    bot('b', 'idle'),
    bot('c', 'error'),
    bot('d', 'working'),
    bot('e', 'idle'),
  ]
  assert.deepEqual(transitions(before, after), [{ kind: 'finished', ref: 'a' }, { kind: 'failed', ref: 'c' }])
})

// MX-251: o banner "Terminou" só interrompe por turno longo (≥ 60 s) fora da vista. A duração é
// medida pela app: âncora monotônica (performance.now) na 1ª observação do turno + o decorrido que
// o Hub já reportava em startedAt naquele instante; tudo congela na transição observada na fonte.
// Qualquer dúvida (sem working observado, startedAt inválido/ausente/futuro) = sem banner.
const relogio = () => {
  const t = { date: 1_000_000, perf: 0 }
  return { t, clocks: { date: () => t.date, perf: () => t.perf } }
}
/** Simula o caminho real: observeTurns congela o fim; o resumo carrega duracaoMs/watching congelados. */
const resumo = (seen: ReadonlyMap<string, TurnSeen>, ref: string): IslandBot =>
  ({ ...bot(ref, 'idle'), duracaoMs: seen.get(ref)?.duracaoMs ?? null, watching: seen.get(ref)?.watching ?? null })

test('turno curto, ou longo com a conversa à vista, não abre "Terminou"; falha continua abrindo', () => {
  const { t, clocks } = relogio()
  const seen = new Map<string, TurnSeen>()
  const startedAt = t.date - 10_000
  observeTurns(seen, [
    { ...bot('a', 'working'), startedAt, watching: false },
    { ...bot('b', 'working'), startedAt: t.date - (LONG_TURN_MS + 10_000), watching: true },
    { ...bot('f', 'working'), startedAt },
  ], clocks)
  t.perf = 5_000 // a: ~15 s no total; b: longo, mas o dono está olhando no fim
  observeTurns(seen, [bot('a', 'idle'), { ...bot('b', 'idle'), watching: true }, bot('f', 'error')], clocks)
  const before = new Map<string, string>([['a', 'working'], ['b', 'working'], ['f', 'working']])
  assert.deepEqual(transitions(before, [resumo(seen, 'a'), resumo(seen, 'b'), bot('f', 'error')]), [{ kind: 'failed', ref: 'f' }])
})

test('tri-estado: só "fora da vista provado" notifica — desconhecido e à vista não notificam', () => {
  const before = new Map([['a', 'working']])
  const longo = LONG_TURN_MS + 1000
  assert.deepEqual(transitions(before, [{ ...bot('a', 'idle'), duracaoMs: longo, watching: false }]), [{ kind: 'finished', ref: 'a' }])
  assert.deepEqual(transitions(before, [{ ...bot('a', 'idle'), duracaoMs: longo, watching: true }]), [])
  assert.deepEqual(transitions(before, [{ ...bot('a', 'idle'), duracaoMs: longo, watching: null }]), [])
  // Resumo sem o campo (origem desconhecida) também não notifica.
  assert.deepEqual(transitions(before, [{ ...bot('a', 'idle'), duracaoMs: longo }]), [])
  // E observeTurns propaga o desconhecido sem coagir: seleção nula (watching: null) congela null.
  const { t, clocks } = relogio()
  const seen = new Map<string, TurnSeen>()
  observeTurns(seen, [{ ...bot('a', 'working'), startedAt: t.date - longo, watching: null }], clocks)
  observeTurns(seen, [{ ...bot('a', 'idle'), watching: null }], clocks)
  assert.equal(seen.get('a')!.watching, null)
  assert.deepEqual(transitions(before, [resumo(seen, 'a')]), [])
})

// MX-251 (revisão): a leitura da janela mapeia para o tri-estado ANTES da decisão. Sem ela, um
// `visibilityState` ausente/desconhecido com foco verdadeiro virava "fora da vista" e notificava.
test('visibilidade fora do conjunto conhecido é "desconhecido": sem prova, sem "Terminou"', () => {
  // Valor inesperado de visibilityState com a janela focada: nunca fora da vista.
  for (const visibilidade of [undefined, null, '', 'unloaded', 'collapsed', 0, {}, 'hidden ']) {
    assert.equal(janelaVista(true, visibilidade), null, `visibilityState=${String(visibilidade)}`)
  }
  // Foco indisponível ou não booleano: sem prova nem de à vista nem de fora — exceto quando a
  // aba oculta já prova "fora da vista" sozinha.
  for (const foco of [undefined, null, 0, 1, 'true']) {
    assert.equal(janelaVista(foco, 'visible'), null, `hasFocus=${String(foco)}`)
    assert.equal(janelaVista(foco, 'hidden'), false)
  }
  // O conjunto conhecido continua mapeando certo.
  assert.equal(janelaVista(true, 'visible'), true)
  assert.equal(janelaVista(false, 'visible'), false)
  assert.equal(janelaVista(true, 'hidden'), false)
  assert.equal(janelaVista(true, 'prerender'), false)
  // Caminho completo: turno longo, janela focada e visibilityState desconhecido no fim = sem banner.
  const { t, clocks } = relogio()
  const seen = new Map<string, TurnSeen>()
  observeTurns(seen, [{ ...bot('a', 'working'), startedAt: t.date - (LONG_TURN_MS + 1000), watching: janelaVista(true, undefined) }], clocks)
  observeTurns(seen, [{ ...bot('a', 'idle'), watching: janelaVista(true, undefined) }], clocks)
  assert.equal(seen.get('a')!.watching, null)
  assert.deepEqual(transitions(new Map([['a', 'working']]), [resumo(seen, 'a')]), [])
})

test('59,999 s não vira banner; 60,000 s fora da vista vira', () => {
  for (const [delta, esperado] of [[LONG_TURN_MS - 1, []], [LONG_TURN_MS, [{ kind: 'finished', ref: 'a' }]]] as const) {
    const { t, clocks } = relogio()
    const seen = new Map<string, TurnSeen>()
    observeTurns(seen, [{ ...bot('a', 'working'), startedAt: t.date, watching: false }], clocks)
    t.perf = delta
    observeTurns(seen, [{ ...bot('a', 'idle'), watching: false }], clocks)
    assert.equal(seen.get('a')!.duracaoMs, delta)
    assert.deepEqual(transitions(new Map([['a', 'working']]), [resumo(seen, 'a')]), esperado)
  }
})

test('startedAt ausente, inválido ou futuro na 1ª observação = duração desconhecida, sem banner', () => {
  const { t, clocks } = relogio()
  for (const startedAt of [null, Number.NaN, Number.POSITIVE_INFINITY, -5, t.date + 1]) {
    const seen = new Map<string, TurnSeen>()
    observeTurns(seen, [{ ...bot('a', 'working'), startedAt, watching: false }], clocks)
    t.perf += LONG_TURN_MS + 10_000 // turno claramente longo — ainda assim não notifica
    observeTurns(seen, [bot('a', 'idle')], clocks)
    assert.equal(seen.get('a')!.duracaoMs, null)
    assert.deepEqual(transitions(new Map([['a', 'working']]), [resumo(seen, 'a')]), [])
  }
})

test('idle com startedAt antigo sem working observado = duração desconhecida, sem banner', () => {
  const { clocks } = relogio()
  const seen = new Map<string, TurnSeen>()
  // O resumo já chega idle carregando um startedAt velho (Hub antigo / app recém-aberta).
  observeTurns(seen, [{ ...bot('a', 'idle'), startedAt: 1, watching: false }], clocks)
  assert.equal(seen.get('a')!.duracaoMs, null)
  assert.deepEqual(transitions(new Map([['a', 'working']]), [resumo(seen, 'a')]), [])
  // E um IslandBot sem duracaoMs nenhum (resumo de origem desconhecida) também não notifica.
  assert.deepEqual(transitions(new Map([['a', 'working']]), [bot('a', 'idle')]), [])
})

test('a duração é monotônica: Date.now posterior não infla nem encolhe o turno', () => {
  const { t, clocks } = relogio()
  const seen = new Map<string, TurnSeen>()
  observeTurns(seen, [{ ...bot('a', 'working'), startedAt: t.date - 30_000, watching: false }], clocks)
  t.date += 10_000_000 // o relógio do sistema salta; o monotônico anda só 30 s
  t.perf = 30_000
  observeTurns(seen, [{ ...bot('a', 'idle'), watching: false }], clocks)
  assert.equal(seen.get('a')!.duracaoMs, 60_000)
  assert.deepEqual(transitions(new Map([['a', 'working']]), [resumo(seen, 'a')]), [{ kind: 'finished', ref: 'a' }])
})

test('o "estou olhando" congela no instante da transição observada', () => {
  const { t, clocks } = relogio()
  const seen = new Map<string, TurnSeen>()
  const startedAt = t.date - (LONG_TURN_MS + 10_000)
  observeTurns(seen, [{ ...bot('a', 'working'), startedAt, watching: false }], clocks)
  // Terminou fora da vista; o dono abre a conversa antes de a ilha ler o resumo.
  observeTurns(seen, [{ ...bot('a', 'idle'), watching: false }], clocks)
  observeTurns(seen, [{ ...bot('a', 'idle'), watching: true }], clocks)
  assert.equal(seen.get('a')!.watching, false, 'abrir a conversa depois do fim não apaga o aviso')
  assert.deepEqual(transitions(new Map([['a', 'working']]), [resumo(seen, 'a')]), [{ kind: 'finished', ref: 'a' }])
  // E o inverso: terminou à vista; sair da conversa depois não cria banner.
  const seen2 = new Map<string, TurnSeen>()
  observeTurns(seen2, [{ ...bot('b', 'working'), startedAt, watching: true }], clocks)
  observeTurns(seen2, [{ ...bot('b', 'idle'), watching: true }], clocks)
  observeTurns(seen2, [{ ...bot('b', 'idle'), watching: false }], clocks)
  assert.equal(seen2.get('b')!.watching, true)
  assert.deepEqual(transitions(new Map([['b', 'working']]), [resumo(seen2, 'b')]), [])
  // Turno novo limpa o congelado.
  observeTurns(seen2, [{ ...bot('b', 'working'), startedAt: t.date, watching: false }], clocks)
  assert.equal(seen2.get('b')!.ended, false)
})

// Não-regressão: falha e aprovação se comportam exatamente como na main — falha sempre aparece,
// com a conversa à vista ou não, sem filtro de duração; aprovação segue pelo caminho das permissões.
test('falha aparece sempre, à vista ou não; aprovação segue inalterada', () => {
  const { t, clocks } = relogio()
  const seen = new Map<string, TurnSeen>()
  observeTurns(seen, [
    { ...bot('a', 'working'), startedAt: t.date - 1000, watching: true }, // falha à vista, turno curto
    { ...bot('b', 'needsInput'), startedAt: t.date - 1000, watching: true },
  ], clocks)
  observeTurns(seen, [bot('a', 'error'), bot('b', 'error')], clocks)
  const before = new Map<string, string>([['a', 'working'], ['b', 'needsInput'], ['c', 'idle']])
  assert.deepEqual(transitions(before, [bot('a', 'error'), bot('b', 'error'), bot('c', 'idle')]), [
    { kind: 'failed', ref: 'a' },
    { kind: 'failed', ref: 'b' },
  ])
  // Aprovação: o caminho é islandMode sobre permissions, sem filtro de duração — como na main.
  assert.equal(islandMode(summary([bot('a', 'idle')], [card('e1')]), state()), 'approval')
})

// MB-1b fix (06/11): a ilha contava a pergunta sem turno como "trabalhando". O contador usa o mesmo isWorking da
// conversa: pergunta sem turno fica no Precisa de você (foco); turno real e permissão (turno aberto) contam.
test('"N trabalhando" não conta o bot que só tem pergunta pendente', () => {
  const q = { entryId: 'e1', chatId: 'q', text: 'Qual pasta?', askedAt: 1 }
  const from = (ref: string, raw: Record<string, unknown>): IslandBot => ({ ...bot(ref, raw.status as BotStatus), working: isWorking(normalizeBot({ id: ref, ...raw })) })
  const bots = [
    from('w', { status: 'working', workingChat: 'w' }),
    from('p', { status: 'needsInput', workingChat: 'p' }),
    from('q', { status: 'needsInput', workingChat: null, pendingQuestions: [q] }),
  ]
  assert.equal(workingCount(bots), 2)
  assert.equal(workingCount([bots[2]]), 0)
  assert.equal(focusOf([bots[0], bots[2]])?.ref, 'q', 'a pergunta fica no Precisa de você')
  assert.equal(islandMode(summary([bots[2]]), state()), 'compact', 'a ilha continua mostrando quem precisa de você')
})

test('the big slot goes to the bot that needs you, the rest to the side', () => {
  const bots = [bot('a', 'idle'), bot('b', 'working'), bot('c', 'needsInput'), bot('d', 'idle'), bot('e', 'working')]
  assert.equal(focusOf(bots)?.ref, 'c')
  assert.deepEqual(othersOf(bots, focusOf(bots), 3).map((b) => b.ref), ['b', 'e', 'a'])
  assert.equal(focusOf([]), null)
})

test('the notch is the screen width minus the menu bar on both sides', () => {
  const screens = [
    { x: 0, width: 1512, top: 32, left: 663.5, right: 663.5 },
    { x: 1512, width: 2560, top: 0, left: 0, right: 0 },
  ]
  assert.deepEqual(notchOf(screens, { x: 0, width: 1512 }), { width: 185, height: 32 })
  assert.equal(notchOf(screens, { x: 1512, width: 2560 }), null)
  assert.equal(notchOf([], { x: 0, width: 1512 }), null)
})

test('shapes follow the notch, and a plain top edge keeps a thin strip that wakes it', () => {
  const notch = { notch: { width: 185, height: 32 } }
  assert.deepEqual(islandFrame('hidden', notch), { width: 185, height: 32, radius: 14 })
  assert.equal(islandFrame('peek', notch).width, 185 + 64)
  assert.equal(islandFrame('compact', notch).width, 185 + 104)
  // Every open view has one size: switching tabs never moves the shape.
  for (const mode of ['overview', 'empty', 'approval', 'finished', 'failed', 'message', 'usage'] as const) {
    assert.deepEqual(islandFrame(mode, notch), { width: 640, height: 196, radius: 30 })
  }
  // A taller notch pushes the content down.
  assert.equal(islandFrame('approval', { notch: { width: 200, height: 38 } }).height, 202)
  const edge = islandRect(islandFrame('hidden', { notch: null }), 720)
  assert.equal(hits(edge, 360, 0), true)
  assert.equal(hits(edge, 360, 12), false)
  assert.equal(hits(edge, 100, 0), false)
  // Linux shapes the window to this: the strip under the top edge, never above the window.
  assert.deepEqual(hitRect(edge), { x: 272, y: 0, width: 176, height: 8 })
  assert.deepEqual(hitRect({ x: 0, y: 0, width: 0, height: 0 }), { x: 0, y: 0, width: 0, height: 0 })
})

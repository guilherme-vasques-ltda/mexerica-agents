import { test } from 'node:test'
import assert from 'node:assert/strict'
import { setImmediate as settle } from 'node:timers/promises'
import { setRole, type TimesGraph } from '../../../../shared/fork-times.ts'
import { TimesSession, type RosterBot, type TimesEvent, type TimesLink, type TimesSource } from './times-session.ts'

// Times (times-03): a fronteira do cliente. Toda resposta é conferida contra a origem (Hub, link, geração, space);
// prévia não grava; conflito conserva o rascunho; Salvar sem resposta reenvia a mesma intenção, nunca outra; outro
// Hub encerra a tela, sem cair nele.

const deferred = <T>() => { let resolve!: (v: T) => void; let reject!: (e: unknown) => void; const promise = new Promise<T>((a, b) => { resolve = a; reject = b }); return { promise, resolve, reject } }
const offline = () => Object.assign(new Error('offline'), { status: 0 })

const PESSOAL = '11111111-1111-4111-8111-111111111111'
const graph = (): TimesGraph => ({
  nodes: [],
  members: [
    { botId: 'donna', teamId: null, parentBotId: null, role: 'manager', reportsTo: { kind: 'owner' }, ownerContact: 'reports' },
    { botId: 'feed', teamId: null, parentBotId: null, role: 'executor', reportsTo: { kind: 'owner' }, ownerContact: 'reports' },
  ],
  routes: [],
})
const snapshot = (revision: number, g: TimesGraph = graph(), spaceId = PESSOAL) => ({
  version: 1, spaceId, revision, catalogVersion: 'v1:cat', primaryBotId: 'donna',
  bots: [
    { id: 'donna', name: 'Donna', spaceId, title: '', avatarColor: 'blue', avatarShape: 'blob', hidden: false },
    { id: 'feed', name: 'Feed', spaceId, title: '', avatarColor: 'blue', avatarShape: 'blob', hidden: false },
  ],
  ...g,
})

type Call = { method: string; body: Record<string, unknown>; answer: ReturnType<typeof deferred<unknown>> }

/** Um link que responde à mão: cada chamada fica pendente até o teste decidir. */
class FakeLink implements TimesLink {
  calls: Call[] = []
  listeners: ((e: TimesEvent) => void)[] = []
  ends: ((error: unknown) => void)[] = []
  call(method: string, body: unknown = {}) {
    const answer = deferred<unknown>()
    this.calls.push({ method, body: body as Record<string, unknown>, answer })
    return answer.promise
  }
  inSpace(_spaceId: string) {
    return { events: (_since: number, _client: string, onEvent: (e: TimesEvent) => void, onEnd: (error: unknown) => void) => { this.listeners.push(onEvent); this.ends.push(onEnd); return () => {} } }
  }
  last(method: string) { return [...this.calls].reverse().find((c) => c.method === method)! }
}

class FakeSource implements TimesSource {
  hello: { hostId: string; rev: number } | null = { hostId: 'hub-a', rev: 1 }
  connectedClient: FakeLink | null = new FakeLink()
  principalEpoch = 1
  spaces = { personalSpaceId: PESSOAL }
  bots: Map<string, RosterBot> = new Map([['donna', { id: 'donna', name: 'Donna' }]])
  private listeners = new Set<() => void>()
  subscribe(l: () => void) { this.listeners.add(l); return () => void this.listeners.delete(l) }
  emit() { for (const l of [...this.listeners]) l() }
}

async function opened() {
  const source = new FakeSource()
  const session = new TimesSession(() => `0000000${++ids}-0000-4000-8000-000000000000`)
  session.open(source, null)
  const link = source.connectedClient!
  assert.deepEqual(link.calls[0]!.body, { spaceId: PESSOAL }, 'o Pessoal padrão é o UUID canônico da store')
  link.calls[0]!.answer.resolve(snapshot(3))
  await settle()
  assert.equal(session.phase.kind, 'ready')
  return { source, session, link }
}
let ids = 0
const makeManager = (g: TimesGraph) => setRole(g, 'feed', 'manager')

test('prévia não grava: o rascunho muda, a revisão do Hub não; só Salvar cria revisão nova', async () => {
  const { session, link } = await opened()
  const proposing = session.propose('Feed como Gerente', makeManager)
  const preview = link.last('donnaTimesPreview')
  assert.equal(preview.body.expectedRevision, 3)
  assert.equal(preview.body.clientRequestId, undefined, 'prévia não leva chave de gravação')
  assert.equal(session.editable, false, 'nada novo enquanto o Hub confere')
  preview.answer.resolve({ ...snapshot(3, makeManager(graph())), preview: true })
  assert.equal(await proposing, true)
  assert.equal(session.snapshot!.revision, 3)
  assert.equal(session.snapshot!.members.find((m) => m.botId === 'feed')!.role, 'executor', 'o estado do Hub segue o salvo')
  assert.equal(session.draft!.members.find((m) => m.botId === 'feed')!.role, 'manager')

  const saving = session.commit()
  const save = link.last('donnaTimesSave')
  assert.equal(save.body.expectedRevision, 3)
  assert.equal(typeof save.body.clientRequestId, 'string')
  save.answer.resolve({ snapshot: snapshot(7, makeManager(graph())), receipt: { clientRequestId: save.body.clientRequestId, revision: 7 } })
  assert.equal(await saving, true)
  assert.equal(session.snapshot!.revision, 7)
  assert.equal(session.draft, null)
})

test('prévia recusada volta só aquela mudança e conserva o rascunho anterior', async () => {
  const { session, link } = await opened()
  const first = session.propose('Feed como Gerente', makeManager)
  link.last('donnaTimesPreview').answer.resolve({ ...snapshot(3, makeManager(graph())), preview: true })
  await first
  const second = session.propose('Feed reporta a Donna', (g) => g)
  link.last('donnaTimesPreview').answer.reject(new Error('report_cycle: ligação cria um ciclo'))
  assert.equal(await second, false)
  assert.equal(session.check.kind, 'refused')
  assert.equal(session.draft!.members.find((m) => m.botId === 'feed')!.role, 'manager', 'a mudança aceita antes continua')
})

test('conflito ao salvar conserva o rascunho, relê o Hub e só salva depois de conferir de novo', async () => {
  const { session, link } = await opened()
  const p = session.propose('Feed como Gerente', makeManager)
  link.last('donnaTimesPreview').answer.resolve({ ...snapshot(3, makeManager(graph())), preview: true })
  await p
  const saving = session.commit()
  link.last('donnaTimesSave').answer.reject(new Error('times_conflict: o quadro mudou'))
  assert.equal(await saving, false)
  assert.equal(session.save.kind, 'conflict')
  assert.ok(session.draft, 'o rascunho continua')
  link.last('donnaTimes').answer.resolve(snapshot(5))
  await settle()
  assert.equal(session.stale, true)
  assert.equal(await session.commit(), false, 'não salva sobre uma versão que não conferiu')
  const again = session.reconfirm()
  const preview = link.last('donnaTimesPreview')
  assert.equal(preview.body.expectedRevision, 5)
  preview.answer.resolve({ ...snapshot(5, makeManager(graph())), preview: true })
  assert.equal(await again, true)
  assert.equal(session.stale, false)
})

test('Salvar sem resposta: Tentar de novo reenvia a mesma intenção, com a mesma chave', async () => {
  const { session, link } = await opened()
  const p = session.propose('Feed como Gerente', makeManager)
  link.last('donnaTimesPreview').answer.resolve({ ...snapshot(3, makeManager(graph())), preview: true })
  await p
  const saving = session.commit()
  const first = link.last('donnaTimesSave')
  first.answer.reject(offline())
  assert.equal(await saving, false)
  assert.equal(session.save.kind, 'unknown')
  assert.equal(session.editable, false, 'o rascunho fica travado até resolver')
  assert.equal(await session.commit(), false, 'nada de outro Salvar')
  const retry = session.retrySave()
  const second = link.last('donnaTimesSave')
  assert.notEqual(second, first)
  assert.deepEqual(second.body, first.body, 'mesmo corpo e mesma clientRequestId')
  second.answer.resolve({ snapshot: snapshot(4, makeManager(graph())), receipt: { clientRequestId: first.body.clientRequestId, revision: 4 } })
  assert.equal(await retry, true)
  assert.equal(link.calls.filter((c) => c.method === 'donnaTimesSave').length, 2)
})

test('reconexão do mesmo Hub: leitura atrasada do link antigo é descartada e um Salvar em curso vira desconhecido', async () => {
  const { source, session, link } = await opened()
  const p = session.propose('Feed como Gerente', makeManager)
  link.last('donnaTimesPreview').answer.resolve({ ...snapshot(3, makeManager(graph())), preview: true })
  await p
  const saving = session.commit()
  const oldSave = link.last('donnaTimesSave')
  // Novo link do mesmo Hub antes da resposta.
  const fresh = new FakeLink()
  source.connectedClient = fresh
  source.principalEpoch = 2
  source.emit()
  assert.equal(fresh.calls[0]!.method, 'donnaTimes', 'o link novo relê')
  fresh.calls[0]!.answer.resolve(snapshot(3))
  oldSave.answer.resolve({ snapshot: snapshot(9), receipt: { clientRequestId: oldSave.body.clientRequestId, revision: 9 } })
  assert.equal(await saving, false)
  await settle()
  assert.equal(session.snapshot!.revision, 3, 'a resposta do link antigo não pinta')
  assert.equal(session.save.kind, 'unknown')
  const retry = session.retrySave()
  const resent = fresh.last('donnaTimesSave')
  assert.deepEqual(resent.body, oldSave.body, 'a mesma intenção, pelo link novo')
  resent.answer.resolve({ snapshot: snapshot(9), receipt: { clientRequestId: oldSave.body.clientRequestId, revision: 9 } })
  assert.equal(await retry, true)
})

test('A→B→A: de duas leituras, só a mais nova pinta', async () => {
  const { session, link } = await opened()
  session.reread()
  const older = link.calls.at(-1)!
  session.reread()
  const newer = link.calls.at(-1)!
  newer.answer.resolve(snapshot(6))
  older.answer.resolve(snapshot(4))
  await settle()
  assert.equal(session.snapshot!.revision, 6)
})

test('outro Hub no mesmo computador encerra a tela, sem ler nem pedir nada a ele', async () => {
  const { source, session } = await opened()
  const other = new FakeLink()
  source.hello = { hostId: 'hub-b', rev: 1 }
  source.connectedClient = other
  source.principalEpoch = 2
  source.emit()
  await settle()
  assert.equal(session.phase.kind, 'moved')
  assert.equal(session.snapshot, null)
  assert.equal(other.calls.length, 0)
  assert.equal(await session.propose('x', (g) => g), false)
})

test('sem link: só leitura, com o último estado; nada é enviado', async () => {
  const { source, session, link } = await opened()
  source.connectedClient = null
  source.emit()
  assert.equal(session.link, 'offline')
  assert.equal(session.editable, false)
  assert.equal(session.snapshot!.revision, 3, 'a última leitura continua à vista')
  const before = link.calls.length
  assert.equal(await session.propose('Feed como Gerente', makeManager), false)
  assert.equal(link.calls.length, before)
})

test('evento de Times mais novo ou mudança no roster relê o snapshot pelo RPC', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const { source, session, link } = await opened()
  const reads = () => link.calls.filter((c) => c.method === 'donnaTimes').length
  link.listeners[0]!({ type: 'times', spaceId: PESSOAL, revision: 3 })
  t.mock.timers.tick(300)
  await settle()
  assert.equal(reads(), 1, 'revisão já vista não relê')
  link.listeners[0]!({ type: 'times', spaceId: 'outro', revision: 8 })
  t.mock.timers.tick(300)
  await settle()
  assert.equal(reads(), 1, 'evento de outro space não relê')
  link.listeners[0]!({ type: 'times', spaceId: PESSOAL, revision: 8 })
  t.mock.timers.tick(300)
  await settle()
  assert.equal(reads(), 2)
  link.last('donnaTimes').answer.resolve(snapshot(8))
  await settle()
  source.bots = new Map([...source.bots, ['nova', { id: 'nova', name: 'Nova' }]])
  source.emit()
  t.mock.timers.tick(300)
  await settle()
  assert.equal(reads(), 3, 'bot novo no roster relê')
  assert.equal(session.snapshot!.revision, 8)
})

test('hello da inscrição relê mudanças de grafo perdidas entre a leitura e o stream, inclusive ao reconectar', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const { source, session, link } = await opened()
  const reads = (client: FakeLink) => client.calls.filter((c) => c.method === 'donnaTimes').length
  // Outro aparelho alterou somente o grafo depois da leitura inicial. O catch-up não traz Times,
  // e nenhum evento de bot ou de Times chega para revelar essa mudança anterior à inscrição.
  link.listeners[0]!({ type: 'hello' })
  t.mock.timers.tick(300)
  await settle()
  assert.equal(reads(link), 2, 'confirmar a inscrição precisa reler o quadro, sem depender de outro evento')
  link.last('donnaTimes').answer.resolve(snapshot(8, makeManager(graph())))
  await settle()
  assert.equal(session.snapshot?.members.find((m) => m.botId === 'feed')?.role, 'manager')

  source.connectedClient = null
  source.emit()
  const next = new FakeLink()
  source.connectedClient = next
  source.principalEpoch++
  source.emit()
  next.last('donnaTimes').answer.resolve(snapshot(8, makeManager(graph())))
  await settle()
  // A mesma janela na conexão nova: uma segunda mudança de grafo precede o hello, sem evento ao vivo.
  next.listeners[0]!({ type: 'hello' })
  t.mock.timers.tick(300)
  await settle()
  assert.equal(reads(next), 2, 'a reconexão também fecha a janela entre leitura e inscrição')
  next.last('donnaTimes').answer.resolve(snapshot(10))
  await settle()
  assert.equal(session.snapshot?.members.find((m) => m.botId === 'feed')?.role, 'executor')
  session.close()
})

test('fechar e abrir de novo conserva o rascunho do mesmo Hub; outro Hub no computador não o herda', async () => {
  const { source, session, link } = await opened()
  const p = session.propose('Feed como Gerente', makeManager)
  link.last('donnaTimesPreview').answer.resolve({ ...snapshot(3, makeManager(graph())), preview: true })
  await p
  session.close()
  session.open(source, null)
  assert.ok(session.draft, 'o rascunho continua depois de reabrir')
  link.last('donnaTimes').answer.resolve(snapshot(3))
  await settle()
  assert.equal(session.stale, false)
  session.close()
  source.hello = { hostId: 'hub-b', rev: 1 }
  source.connectedClient = new FakeLink()
  session.open(source, null)
  assert.equal(session.draft, null)
  assert.equal(session.snapshot, null)
})

const saveKind = (session: TimesSession) => session.save.kind
const nova = { botId: 'nova', teamId: null, parentBotId: null, role: 'executor' as const, reportsTo: { kind: 'owner' as const }, ownerContact: 'reports' as const }
const withNova = (revision: number) => {
  const g = graph()
  g.members.push({ ...nova })
  const base = snapshot(revision, g)
  return { ...base, catalogVersion: 'v1:cat2', bots: [...base.bots, { id: 'nova', name: 'Nova', spaceId: PESSOAL, title: '', avatarColor: 'blue', avatarShape: 'blob', hidden: false }] }
}
async function drafted() {
  const opened_ = await opened()
  const p = opened_.session.propose('Feed como Gerente', makeManager)
  opened_.link.last('donnaTimesPreview').answer.resolve({ ...snapshot(3, makeManager(graph())), preview: true })
  assert.equal(await p, true)
  return opened_
}

test('fechar durante Salvar: reabrir oferece Tentar de novo com a mesma intenção, e a resposta antiga não sobrescreve', async () => {
  const { source, session, link } = await drafted()
  const saving = session.commit()
  const first = link.last('donnaTimesSave')
  session.close()
  assert.equal(session.save.kind, 'unknown', 'fechar não deixa “Salvando…” preso')
  session.open(source, null)
  link.last('donnaTimes').answer.resolve(snapshot(3))
  await settle()
  assert.equal(session.save.kind, 'unknown')
  const retry = session.retrySave()
  const second = link.last('donnaTimesSave')
  assert.notEqual(second, first)
  assert.deepEqual(second.body, first.body, 'mesmo corpo e mesma clientRequestId')
  first.answer.reject(offline())
  assert.equal(await saving, false)
  // Lido por função: o assert estreita o tipo e a leitura seguinte, no mesmo campo, viraria `never`.
  assert.equal(saveKind(session), 'saving', 'a resposta da gravação antiga não mexe no Tentar de novo')
  second.answer.resolve({ snapshot: snapshot(4, makeManager(graph())), receipt: { clientRequestId: first.body.clientRequestId, revision: 4 } })
  assert.equal(await retry, true)
  assert.equal(saveKind(session), 'idle')
})

test('fechar durante a conferência: nada fica preso e a resposta atrasada não muda o rascunho', async () => {
  const { source, session, link } = await opened()
  const proposing = session.propose('Feed como Gerente', makeManager)
  const preview = link.last('donnaTimesPreview')
  session.close()
  assert.equal(session.check.kind, 'idle')
  session.open(source, null)
  preview.answer.resolve({ ...snapshot(3, makeManager(graph())), preview: true })
  assert.equal(await proposing, false)
  assert.equal(session.draft, null)
  link.last('donnaTimes').answer.resolve(snapshot(3))
  await settle()
  assert.equal(session.editable, true)
})

test('recusa de dono na leitura esconde snapshot e rascunho; prévia atrasada não os recoloca', async () => {
  const { session, link } = await drafted()
  const late = session.propose('Feed como Executor', (g) => setRole(g, 'feed', 'executor'))
  const pending = link.last('donnaTimesPreview')
  session.reread()
  link.last('donnaTimes').answer.reject(Object.assign(new Error('Control scope required'), { status: 403 }))
  await settle()
  assert.equal(session.graph, null)
  assert.equal(session.draft, null)
  assert.deepEqual(session.phase.kind === 'error' && session.phase.failure, 'owner')
  pending.answer.resolve({ ...snapshot(3), preview: true })
  assert.equal(await late, false)
  assert.equal(session.graph, null, 'nada volta à vista')
})

test('recusa de dono na prévia ou ao salvar também tira tudo da tela', async () => {
  const one = await opened()
  const p = one.session.propose('Feed como Gerente', makeManager)
  one.link.last('donnaTimesPreview').answer.reject(new Error('not_allowed: só o dono pode usar o editor.'))
  assert.equal(await p, false)
  assert.equal(one.session.graph, null)
  const two = await drafted()
  const saving = two.session.commit()
  two.link.last('donnaTimesSave').answer.reject(new Error('not_allowed: só o dono pode usar o editor.'))
  assert.equal(await saving, false)
  assert.equal(two.session.graph, null)
  assert.equal(two.session.snapshot, null)
})

test('Conferir de novo com bot novo no space: inclui o bot como o Hub projetou e conserva as edições; só Salvar grava', async () => {
  const { session, link } = await drafted()
  session.reread()
  link.last('donnaTimes').answer.resolve(withNova(5))
  await settle()
  assert.equal(session.stale, true)
  const again = session.reconfirm()
  const preview = link.last('donnaTimesPreview')
  const members = preview.body.members as { botId: string; role: string }[]
  assert.deepEqual(members.map((m) => m.botId).sort(), ['donna', 'feed', 'nova'])
  assert.equal(members.find((m) => m.botId === 'feed')!.role, 'manager', 'a edição continua')
  assert.equal(preview.body.expectedRevision, 5)
  assert.equal(preview.body.catalogVersion, 'v1:cat2')
  const g = makeManager(graph())
  g.members.push({ ...nova })
  preview.answer.resolve({ ...withNova(5), ...g, preview: true })
  assert.equal(await again, true)
  assert.equal(session.stale, false)
  assert.equal(link.calls.filter((c) => c.method === 'donnaTimesSave').length, 0)
})

test('Conferir de novo com bot que saiu: não realoca em silêncio e mostra o motivo', async () => {
  const { session, link } = await drafted()
  session.reread()
  const only = graph()
  only.members = only.members.filter((m) => m.botId !== 'feed')
  link.last('donnaTimes').answer.resolve({ ...snapshot(5, only), catalogVersion: 'v1:cat3' })
  await settle()
  const before = link.calls.length
  assert.equal(await session.reconfirm(), false)
  assert.equal(link.calls.length, before, 'nada foi enviado')
  assert.equal(session.check.kind, 'refused')
})

test('fim só do stream do space: fica visível e, na mesma origem, relê e se inscreve de novo', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const { session, link } = await opened()
  const reads = () => link.calls.filter((c) => c.method === 'donnaTimes').length
  link.ends[0]!(null)
  assert.equal(session.events, 'paused')
  t.mock.timers.tick(2_100)
  await settle()
  assert.equal(session.events, 'live')
  assert.equal(link.listeners.length, 2)
  assert.equal(reads(), 2)
})

test('D249 R4: leitura recusada sem código conhecido fala de leitura, não de mudança, e Tentar de novo relê', async () => {
  const source = new FakeSource()
  const session = new TimesSession(() => `0000000${++ids}-0000-4000-8000-000000000000`)
  session.open(source, null)
  const link = source.connectedClient!
  link.calls[0]!.answer.reject(Object.assign(new Error('codigo_novo_do_hub: algo'), { status: 400 }))
  await settle()
  assert.ok(session.phase.kind === 'error' && session.phase.failure === 'refused')
  // D507 e fatia 10b: a mensagem é dado, a chave do catálogo que vem de shared/fork-times (antes, texto cru em pt-BR).
  assert.deepEqual(session.phase.kind === 'error' && session.phase.message, { chave: 'times.regras.naoLeu' })
  const reads = link.calls.filter((c) => c.method === 'donnaTimes').length
  session.reread()
  assert.equal(link.calls.filter((c) => c.method === 'donnaTimes').length, reads + 1, 'Tentar de novo pede uma leitura nova')
  link.last('donnaTimes').answer.resolve(snapshot(4))
  await settle()
  assert.equal(session.phase.kind, 'ready')
})

test('modelo Marketing: sync relê e devolve o quadro; a equipe conferida é a mesma que vai ao Salvar', async () => {
  const { equipeMarketing } = await import('./modelo-marketing.ts')
  const { session, link } = await opened()
  const syncing = session.sync()
  const read = link.last('donnaTimes')
  const novo = graph()
  novo.members.push({ botId: 'hype', teamId: null, parentBotId: null, role: 'executor', reportsTo: { kind: 'owner' }, ownerContact: 'reports' })
  read.answer.resolve({ ...snapshot(4, novo), catalogVersion: 'v1:cat2' })
  const lido = await syncing
  assert.ok(lido && lido.members.some((m) => m.botId === 'hype'), 'sync espera a leitura e devolve o quadro com o bot novo')

  const change = (g: TimesGraph) => equipeMarketing(g, 'mkt', 'hype', ['feed'])
  const proposing = session.propose('Equipe Marketing', change)
  const preview = link.last('donnaTimesPreview')
  assert.equal(preview.body.catalogVersion, 'v1:cat2', 'a conferência parte do catálogo com os bots novos')
  preview.answer.resolve({ ...snapshot(4, change(novo)), catalogVersion: 'v1:cat2', preview: true })
  assert.equal(await proposing, true)
  void session.commit()
  const save = link.last('donnaTimesSave')
  const { nodes, members, routes } = preview.body
  assert.deepEqual({ nodes: save.body.nodes, members: save.body.members, routes: save.body.routes }, { nodes, members, routes }, 'prévia = gravação')
  assert.deepEqual((save.body.nodes as { managerBotId: string }[]).map((n) => n.managerBotId), ['hype'])
})

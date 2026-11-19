import { test } from 'node:test'
import assert from 'node:assert/strict'
import { parseNativeCatalog } from '../../../../shared/fork-native-catalog.ts'
import { LibrarySession, resetLibraryPending, type LibrarySource } from './biblioteca-session.ts'

// BN-03 (D70): só as fronteiras do cliente. Origem capturada (Hub, link, época, space, geração), corpo fechado,
// mesma intenção em retry, motor obrigatório fora do Pessoal, autoridade do Hub e recibo removido sem navegação.

const settle = () => new Promise((r) => setTimeout(r, 0))
const deferred = <T>() => { let resolve!: (v: T) => void; let reject!: (e: unknown) => void; const promise = new Promise<T>((a, b) => { resolve = a; reject = b }); return { promise, resolve, reject } }
const offline = () => Object.assign(new Error('offline'), { status: 0 })

const HUB = 'hub-a'
const PESSOAL = '00000000-0000-4000-8000-000000000001'
const LOJA = 'space-loja'

const profile = (catalogKey: string) => ({
  catalogKey, profileVersion: 2, origin: 'donnaOriginal', name: catalogKey, title: 'função', avatarConfig: { shape: 'oval' },
  description: `Alma de ${catalogKey}`, examples: ['um', 'dois'], suggestedResources: 'arquivos',
})
const link = (catalogKey: string, state: 'available' | 'active' | 'removed', generation = 0, revision = 0) => ({
  catalogKey, generation, revision, botId: state === 'available' ? null : `bot-${catalogKey}-${generation}`, profileVersion: state === 'available' ? null : 2, state,
})
const catalog = (spaceId: string, links = [link('pesquisador', 'available'), link('operador', 'removed', 1, 7)], hubId = HUB) =>
  ({ hubId, spaceId, catalogVersion: 2, profiles: [profile('pesquisador'), profile('operador')], links })

type Call = { method: string; body: Record<string, unknown>; answer: ReturnType<typeof deferred<unknown>> }
class FakeLink {
  calls: Call[] = []
  call(method: string, body: unknown = {}) {
    const answer = deferred<unknown>()
    this.calls.push({ method, body: body as Record<string, unknown>, answer })
    return answer.promise
  }
  last(method: string) { return [...this.calls].reverse().find((c) => c.method === method)! }
}
class FakeSource implements LibrarySource {
  hello: { hostId: string } | null = { hostId: HUB }
  connectedClient: FakeLink | null = new FakeLink()
  principalEpoch = 0
  canManageEngineAccounts = true
  private listeners = new Set<() => void>()
  subscribe(l: () => void) { this.listeners.add(l); return () => void this.listeners.delete(l) }
  emit() { for (const l of [...this.listeners]) l() }
}
const keys = () => { let n = 0; return () => `chave-${++n}` }

async function opened(spaceId: string, personal: boolean, answer = catalog(spaceId)) {
  resetLibraryPending()
  const source = new FakeSource()
  const session = new LibrarySession(spaceId, personal, keys())
  session.open(source)
  const read = source.connectedClient!.last('donnaNativeCatalog')
  assert.deepEqual(read.body, { hubId: HUB, spaceId })
  read.answer.resolve(answer)
  await settle()
  return { source, session, link: source.connectedClient! }
}

test('DTO: resposta de outro space ou com vínculo faltando é recusada inteira', () => {
  assert.throws(() => parseNativeCatalog(catalog(LOJA), HUB, PESSOAL), /cross_space_response/)
  assert.throws(() => parseNativeCatalog(catalog(LOJA, [link('pesquisador', 'available')]), HUB, LOJA), /native_invalid_answer/)
  assert.throws(() => parseNativeCatalog(catalog(LOJA, [link('pesquisador', 'active'), link('operador', 'removed', 1, 7)].map((l) => ({ ...l, botId: null }))), HUB, LOJA), /native_invalid_answer/)
})

test('Pessoal: corpo fechado sem motor, chave do gesto e geração/revisão da prévia', async () => {
  const { session, link: hub } = await opened(PESSOAL, true)
  assert.equal(session.phase.kind, 'ready')
  void session.add('pesquisador', 'codex')
  await settle()
  assert.deepEqual(hub.last('donnaNativeCatalogAdd').body, {
    hubId: HUB, spaceId: PESSOAL, catalogKey: 'pesquisador', profileVersion: 2, clientRequestKey: 'chave-1',
    intent: 'add', expectedGeneration: 0, expectedRevision: 0,
  })
})

test('Fora do Pessoal: sem motor nada é enviado; com motor, readd usa o vínculo removido', async () => {
  const { session, link: hub } = await opened(LOJA, false)
  assert.equal(await session.add('pesquisador', null), null)
  assert.equal(hub.calls.filter((c) => c.method === 'donnaNativeCatalogAdd').length, 0)
  assert.equal(session.op.kind, 'refused')
  void session.add('operador', 'codex')
  await settle()
  const body = hub.last('donnaNativeCatalogAdd').body
  assert.equal(body.intent, 'readd')
  assert.equal(body.expectedGeneration, 1)
  assert.equal(body.expectedRevision, 7)
  assert.equal(body.backend, 'codex')
})

test('Sem resposta: Conferir resultado reenvia a mesma chave e corpo, inclusive depois de fechar e reabrir', async () => {
  const { source, session, link: hub } = await opened(PESSOAL, true)
  void session.add('pesquisador', null)
  await settle()
  const first = hub.last('donnaNativeCatalogAdd')
  first.answer.reject(offline())
  await settle()
  assert.equal(session.op.kind, 'unknown')
  assert.equal(session.canAdd, false)
  session.close()
  const again = new LibrarySession(PESSOAL, true, keys())
  again.open(source)
  assert.equal(again.op.kind, 'unknown')
  hub.last('donnaNativeCatalog').answer.resolve(catalog(PESSOAL))
  await settle()
  void again.retry()
  await settle()
  const second = hub.last('donnaNativeCatalogAdd')
  assert.notEqual(second, first)
  assert.deepEqual(second.body, first.body)
  second.answer.resolve({ hubId: HUB, spaceId: PESSOAL, catalogKey: 'pesquisador', clientRequestKey: 'chave-1', generation: 1, botId: 'bot-1', state: 'active' })
  await settle()
  assert.equal(again.op.kind, 'done')
})

test('Queda e volta do MESMO link: a resposta antiga não confirma nada e a intenção fica para conferir', async () => {
  const { source, session, link: hub } = await opened(PESSOAL, true)
  void session.add('pesquisador', null)
  await settle()
  const pendingAdd = hub.last('donnaNativeCatalogAdd')
  source.connectedClient = null
  source.emit()
  source.connectedClient = hub
  source.emit()
  pendingAdd.answer.resolve({ hubId: HUB, spaceId: PESSOAL, catalogKey: 'pesquisador', clientRequestKey: 'chave-1', generation: 1, botId: 'bot-1', state: 'active' })
  await settle()
  assert.equal(session.op.kind, 'unknown')
  // A leitura nova pelo link de volta é a única aceita.
  hub.last('donnaNativeCatalog').answer.resolve(catalog(PESSOAL, [link('pesquisador', 'active', 1, 3), link('operador', 'removed', 1, 7)]))
  await settle()
  assert.equal(session.catalog?.links[0].state, 'active')
})

test('Prévia atrasada de antes da troca de Hub não pinta; outro Hub esquece tudo', async () => {
  resetLibraryPending()
  const source = new FakeSource()
  const session = new LibrarySession(LOJA, false, keys())
  session.open(source)
  const late = source.connectedClient!.last('donnaNativeCatalog')
  source.hello = { hostId: 'hub-b' }
  source.emit()
  late.answer.resolve(catalog(LOJA))
  await settle()
  assert.equal(session.phase.kind, 'moved')
  assert.equal(session.catalog, null)
})

test('Recibo histórico removido não vira sucesso ativo; sem autoridade de gestão nada é enviado', async () => {
  const { source, session, link: hub } = await opened(LOJA, false)
  void session.add('pesquisador', 'codex')
  await settle()
  hub.last('donnaNativeCatalogAdd').answer.resolve({ hubId: HUB, spaceId: LOJA, catalogKey: 'pesquisador', clientRequestKey: 'chave-1', generation: 1, botId: 'bot-1', state: 'removed' })
  await settle()
  assert.equal(session.op.kind === 'done' && session.op.added.state, 'removed')
  session.discard()
  source.canManageEngineAccounts = false
  source.emit()
  assert.equal(session.canAdd, false)
  assert.equal(await session.add('pesquisador', 'codex'), null)
  assert.equal(hub.calls.filter((c) => c.method === 'donnaNativeCatalogAdd').length, 1, 'nenhum envio sem a capacidade do Hub')
})

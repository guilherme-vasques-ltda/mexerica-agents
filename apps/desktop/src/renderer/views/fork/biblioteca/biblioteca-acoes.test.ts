import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { SpaceConversation } from '../../../../shared/fork-spaces.ts'
import type { Entry } from '../../../../shared/models.ts'
import { openSpaceAt, SpaceContext, type SpaceLink, type SpaceSource } from '../../../store/fork-space-context.ts'
import { libraryActions, libraryNavigation, type LibraryNavigationDeps } from './biblioteca-acoes.ts'
import { LibrarySession, resetLibraryPending, type LibrarySource } from './biblioteca-session.ts'

// BN-03 R1 (D70): composição real das duas fronteiras bloqueadas na R1.
// R1-02: recibo histórico Removido → nenhuma ação/navegação para o sucessor; só “Ver estado atual” separa o recibo do
//        vínculo atual. R1-01: space já aberto na conversa A → biblioteca → conversa B, pelo mesmo caminho da janela
//        (libraryNavigation → openSpaceAt → SpaceContext): nada de A aparece sob B, nem por resposta atrasada.

const settle = () => new Promise((r) => setTimeout(r, 0))
const deferred = <T>() => { let resolve!: (v: T) => void; let reject!: (e: unknown) => void; const promise = new Promise<T>((a, b) => { resolve = a; reject = b }); return { promise, resolve, reject } }
const HUB = 'hub-a'
const LOJA = 'space-loja'

// MARK: navegação gravada (a janela real liga estes ganchos em ChatWindow)

function recorder() {
  const calls: string[] = []
  const deps: LibraryNavigationDeps = {
    close: () => calls.push('close'),
    openEditor: (id) => { calls.push(`editor:${id}`); return true },
    selectPersonal: (id) => calls.push(`pessoal:${id}`),
    closeSpace: () => calls.push('closeSpace'),
    openSpace: (space, id) => calls.push(`space:${space}:${id}`),
  }
  return { calls, deps }
}

// MARK: R1-02

type Call = { method: string; body: Record<string, unknown>; answer: ReturnType<typeof deferred<unknown>> }
class CatalogLink {
  calls: Call[] = []
  call(method: string, body: unknown = {}) {
    const answer = deferred<unknown>()
    this.calls.push({ method, body: body as Record<string, unknown>, answer })
    return answer.promise
  }
  last(method: string) { return [...this.calls].reverse().find((c) => c.method === method)! }
}
const catalog = (state: 'available' | 'active', botId: string | null, generation: number, revision: number) => ({
  hubId: HUB, spaceId: LOJA, catalogVersion: 2,
  profiles: [{ catalogKey: 'pesquisador', profileVersion: 2, origin: 'donnaOriginal', name: 'Pesquisador', title: 'pesquisa', avatarConfig: {}, description: 'Alma', examples: ['um'], suggestedResources: 'arquivos' }],
  links: [{ catalogKey: 'pesquisador', generation, revision, botId, profileVersion: botId ? 2 : null, state }],
})

test('R1-02: retry A → recibo removed/bot1 com releitura active/bot2 não oferece nem navega para bot2; só Ver estado atual', async () => {
  resetLibraryPending()
  const link = new CatalogLink()
  const listeners = new Set<() => void>()
  const source: LibrarySource = { hello: { hostId: HUB }, connectedClient: link, principalEpoch: 0, canManageEngineAccounts: true, subscribe: (l) => { listeners.add(l); return () => void listeners.delete(l) } }
  const session = new LibrarySession(LOJA, false, () => 'chave-A')
  session.open(source)
  link.last('donnaNativeCatalog').answer.resolve(catalog('available', null, 0, 0))
  await settle()
  void session.add('pesquisador', 'codex')
  await settle()
  link.last('donnaNativeCatalogAdd').answer.reject(Object.assign(new Error('offline'), { status: 0 }))
  await settle()
  // Enquanto isso, em outro aparelho: bot1 removido e intenção B criou bot2.
  void session.retry()
  await settle()
  const retry = link.last('donnaNativeCatalogAdd')
  assert.equal(retry.body.clientRequestKey, 'chave-A')
  retry.answer.resolve({ hubId: HUB, spaceId: LOJA, catalogKey: 'pesquisador', clientRequestKey: 'chave-A', generation: 1, botId: 'bot1', state: 'removed' })
  await settle()
  link.last('donnaNativeCatalog').answer.resolve(catalog('active', 'bot2', 2, 9))
  await settle()
  assert.equal(session.catalog?.links[0]?.botId, 'bot2', 'a releitura legítima mostra o slot atual')

  const { calls, deps } = recorder()
  const nav = libraryNavigation({ personal: false, spaceId: LOJA }, deps)
  for (const preview of [true, false]) {
    const actions = libraryActions('pesquisador', { personal: false, link: session.catalog!.links[0]!, op: session.op, preview })
    assert.deepEqual(actions, [{ kind: 'current' }], 'o recibo só sai por Ver estado atual')
    assert.doesNotMatch(JSON.stringify(actions), /bot2|bot1/, 'nenhum destino para o sucessor nem para o bot removido')
    // A UI renderiza exatamente estas ações; qualquer uma com destino chamaria a navegação.
    for (const a of actions) if ('botId' in a) nav.onConversation(a.botId as string)
  }
  assert.deepEqual(calls, [], 'nenhuma navegação a partir do recibo histórico')
  for (const personal of [true]) {
    assert.deepEqual(libraryActions('pesquisador', { personal, link: session.catalog!.links[0]!, op: session.op, preview: true }), [{ kind: 'current' }])
  }

  // Transição explícita e separada: só agora o vínculo atual (bot2) volta a ser oferecido, como ação própria.
  session.showCurrent()
  await settle()
  link.last('donnaNativeCatalog').answer.resolve(catalog('active', 'bot2', 2, 9))
  await settle()
  const current = libraryActions('pesquisador', { personal: false, link: session.catalog!.links[0]!, op: session.op, preview: true })
  assert.deepEqual(current, [{ kind: 'conversation', botId: 'bot2' }])
  nav.onConversation('bot2')
  assert.deepEqual(calls, ['close', `space:${LOJA}:bot2`])
})

test('R1-02: recibo active abre o bot do próprio recibo, não o que a releitura mostrar', () => {
  const op = { kind: 'done' as const, added: { hubId: HUB, spaceId: LOJA, catalogKey: 'pesquisador', clientRequestKey: 'k', generation: 1, botId: 'bot1', state: 'active' as const } }
  const link = { catalogKey: 'pesquisador', generation: 2, revision: 9, botId: 'bot2', profileVersion: 2, state: 'active' as const }
  assert.deepEqual(libraryActions('pesquisador', { personal: true, link, op, preview: true }), [{ kind: 'conversation', botId: 'bot1' }, { kind: 'editor', botId: 'bot1' }])
})

// MARK: R1-01

const conversation = (id: string, botId: string, primary: boolean): SpaceConversation => ({
  id, spaceId: LOJA, botId, title: id, revision: 1, stateRevision: 1, primary, closed: false, readRevision: 0,
  engineAccountId: null, engineSelection: null, accountRevision: null, continuity: null,
})
const entry = (id: string, seq: number, conversationId: string): Entry => ({
  id, seq, botId: conversationId, spaceId: LOJA, conversationId, rev: seq, kind: 'user', turn: 0, data: { text: id }, createdAt: 0, updatedAt: 0,
})

class SpaceFake implements SpaceLink {
  calls: Call[] = []
  call(method: string, body: unknown) {
    const answer = deferred<unknown>()
    this.calls.push({ method, body: body as Record<string, unknown>, answer })
    return answer.promise
  }
  inSpace() { return { events: () => () => {} } }
  pending(method: string, conversationId?: string) {
    return this.calls.filter((c) => c.method === method && (conversationId === undefined || c.body.conversationId === conversationId))
  }
}
const list = () => ({ hubId: HUB, spaceId: LOJA, conversations: [conversation('A', 'botA', true), conversation('B', 'botB', true)], principal: { id: 'botA', name: 'Donna' } })
const page = (conversationId: string, seqs: number[], hasMore: boolean) => ({
  hubId: HUB, context: { version: 1, spaceId: LOJA, conversationId, botId: conversationId, conversationRevision: 1 },
  entries: seqs.map((n) => entry(`${conversationId}-${n}`, n, conversationId)), hasMore,
})

test('R1-01: space já em A → biblioteca → Abrir conversa B: nada de A sob B, nem página atrasada nem piso de A', async () => {
  const link = new SpaceFake()
  const source: SpaceSource = { hello: { hostId: HUB, rev: 1, spacesContract: 1 }, connectedClient: link, principalEpoch: 1, spaces: null, subscribe: () => () => {} }
  const ctx = new SpaceContext()
  openSpaceAt(ctx, source, LOJA)
  link.pending('donnaConversations')[0]!.answer.resolve(list())
  await settle()
  assert.equal(ctx.conversationId, 'A')
  link.pending('donnaConversationHistory', 'A')[0]!.answer.resolve(page('A', [50, 51], true))
  await settle()
  assert.deepEqual(ctx.history.entries.map((e) => e.id), ['A-50', 'A-51'])
  ctx.loadOlder() // página anterior de A em curso, abaixo do piso 50
  const olderA = link.pending('donnaConversationHistory', 'A')[1]!
  assert.equal(olderA.body.beforeSeq, 50)

  // A janela real: libraryNavigation → openSpace (use-space-view → openSpaceAt) no MESMO space.
  const nav = libraryNavigation({ personal: false, spaceId: LOJA }, { ...recorder().deps, openSpace: (space, id) => openSpaceAt(ctx, source, space, id) })
  nav.onConversation('B')
  assert.equal(ctx.conversationId, 'B')
  assert.equal(ctx.history.entries.length, 0, 'o conteúdo de A some antes de B aparecer')
  assert.equal(ctx.history.state, 'loading')
  assert.equal(ctx.history.hasMore, false)

  olderA.answer.resolve(page('A', [40, 41], true))
  await settle()
  assert.equal(ctx.history.entries.length, 0, 'página atrasada de A não repopula B')

  link.pending('donnaConversations')[1]!.answer.resolve(list())
  await settle()
  const firstB = link.pending('donnaConversationHistory', 'B')[0]!
  assert.equal(firstB.body.beforeSeq, undefined, 'B começa da página mais nova, sem o piso de A')
  firstB.answer.resolve(page('B', [7, 8], true))
  await settle()
  assert.deepEqual(ctx.history.entries.map((e) => e.id), ['B-7', 'B-8'])
  ctx.loadOlder()
  assert.equal(link.pending('donnaConversationHistory', 'B')[1]!.body.beforeSeq, 7, 'paginação de B usa o piso de B')

  // Reler a MESMA conversa conserva o que já mostrava (sem piscar vazio).
  ctx.retry()
  assert.deepEqual(ctx.history.entries.map((e) => e.id), ['B-7', 'B-8'])
})

test('navegação: editor só no Pessoal e só com o bot no app; fora do Pessoal só a conversa exata do space', () => {
  const pessoal = recorder()
  const known = new Set(['bot1'])
  const nav = libraryNavigation({ personal: true, spaceId: 'pessoal' }, { ...pessoal.deps, openEditor: (id) => { pessoal.calls.push(`editor:${id}`); return known.has(id) } })
  nav.onEditor('bot-ausente')
  assert.deepEqual(pessoal.calls, ['editor:bot-ausente'], 'sem o bot no app nada abre e a folha fica')
  nav.onEditor('bot1')
  nav.onConversation('bot1')
  assert.deepEqual(pessoal.calls.slice(1), ['editor:bot1', 'close', 'close', 'closeSpace', 'pessoal:bot1'])
  const space = recorder()
  const fora = libraryNavigation({ personal: false, spaceId: LOJA }, space.deps)
  fora.onEditor('bot1')
  assert.deepEqual(space.calls, [], 'bot-dentro-13: nenhum editor fora do Pessoal')
  assert.deepEqual(libraryActions('pesquisador', { personal: false, link: { catalogKey: 'pesquisador', generation: 1, revision: 1, botId: 'bot1', profileVersion: 2, state: 'active' }, op: { kind: 'idle' }, preview: true }),
    [{ kind: 'conversation', botId: 'bot1' }])
})

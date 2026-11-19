import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createRoutingStore, readRoutingReceipt, RoutingStore, type RoutingOperation, type RoutingSource, type RoutingTransport } from '../../../store/fork-roteamento.ts'
import type { RoutingSave, RoutingView } from '../../../../shared/fork-roteamento.ts'
const view = (botId = 'dev', revision = 0): RoutingView => ({ botId, hubId: 'fixture-hub', spaceId: 'personal', policy: { schemaVersion: 1, revision, value: { mode: 'fixed', guide: '', reserves: [] } }, generalGuide: { schemaVersion: 1, revision: 0, value: '' }, effectiveGuide: '', auto: { available: false, reason: 'executor_and_scope_unproven' }, reservesExecution: 'unavailable', execution: 'existing_fixed', fixedCheck: { status: 'checked', reason: 'catalog_only' }, accounts: [] })
const fixedProfile = () => ({ backend: 'codex', model: 'modelo-a', modelOptions: {}, forkEditor: { engineAccountId: 'conta-a' } })
const fixedEditor = () => ({ getSnapshot: () => ({ botId: 'dev', editor: { profile: fixedProfile() }, draft: fixedProfile(), saving: false, loading: false, error: null }), subscribe: () => () => {} })
function setup() {
  let current = true
  let snapshot = view()
  let failure: string | null = null
  let readFailure: unknown = null
  let release: (() => void) | undefined
  let wait: Promise<void> | undefined
  const requests: RoutingSave[] = []
  const transport: RoutingTransport = {
    current: () => current,
    read: async () => { const result = structuredClone(snapshot); await wait; if (readFailure) throw readFailure; return result },
    save: async (r) => { requests.push(r); await wait; if (failure) throw new Error(failure); return { ...view(r.botId, 1), policy: { schemaVersion: 1, revision: 1, value: r.policy } } },
  }
  return { model: new RoutingStore(transport), requests, failRead: (error: unknown) => { readFailure = error }, fail: (error: string) => { failure = error }, replace: () => { current = false }, changed: () => { snapshot = view('dev', 2) }, delay: () => { wait = new Promise<void>((resolve) => { release = resolve }); return () => release?.() } }
}
test('Auto salva somente a intenção e erros/CAS preservam o rascunho', async () => {
  const f = setup(); await f.model.read()
  f.model.edit({ mode: 'auto', guide: 'específico', reserves: [] }, 'geral')
  f.fail('settings_conflict: segredo não deve aparecer')
  await f.model.save()
  assert.equal(f.model.getSnapshot().policy?.mode, 'auto')
  assert.equal(f.model.getSnapshot().generalGuide, 'geral')
  assert.equal(f.model.getSnapshot().dirty, true)
  assert.match(f.model.getSnapshot().error!, /rascunho/)
  assert.ok(!f.model.getSnapshot().error!.includes('segredo'))
  assert.equal(f.requests[0]?.expectedRevision, 0)
  assert.equal(f.requests[0]?.expectedGeneralRevision, 0)
})
test('conferência com rascunho não avança o CAS sem consentimento', async () => {
  const f = setup(); await f.model.read()
  f.model.edit({ mode: 'auto', guide: 'meu texto', reserves: [] })
  f.changed(); await f.model.read(true)
  assert.equal(f.model.getSnapshot().view?.policy.revision, 0)
  assert.equal(f.model.getSnapshot().policy?.guide, 'meu texto')
  await f.model.save()
  assert.equal(f.requests[0]?.expectedRevision, 0)
  assert.equal(f.model.getSnapshot().view?.execution, 'existing_fixed')
  assert.match(f.model.getSnapshot().message!, /continua no Fixo/)
})
test('troca de Hub/bot/Space ou encerramento descarta leitura e save atrasados', async () => {
  for (const operation of ['read', 'save'] as const) {
    for (const closing of [false, true]) {
      const f = setup(); await f.model.read()
      f.model.edit({ mode: 'auto', guide: 'rascunho', reserves: [] })
      const release = f.delay()
      const pending = operation === 'read' ? f.model.read() : f.model.save()
      if (closing) f.model.close(); else f.replace()
      release(); await pending
      assert.equal(f.model.getSnapshot().view?.policy.revision, 0)
      assert.equal(f.model.getSnapshot().policy?.guide, 'rascunho')
      assert.equal(f.model.getSnapshot().message, null)
    }
  }
})
test('reconexão do mesmo Hub conserva rascunho e invalida resposta da conexão anterior', async () => {
  let epoch = 1
  let release: (() => void) | undefined
  let waiting: Promise<void> | undefined
  const operation: RoutingOperation = { current: () => true, read: async () => { const answer = view('dev', epoch); await waiting; return answer }, save: async () => view() }
  const model = new RoutingStore({ ...operation, capture: () => { const captured = epoch; return { ...operation, current: () => captured === epoch } } })
  await model.read()
  model.edit({ mode: 'auto', guide: 'não perder', reserves: [] })
  waiting = new Promise<void>((resolve) => { release = resolve })
  const reading = model.read()
  epoch++
  release?.(); await reading
  assert.equal(model.getSnapshot().view?.policy.revision, 1)
  waiting = undefined
  await model.read()
  assert.equal(model.getSnapshot().policy?.guide, 'não perder')
  assert.equal(model.getSnapshot().view?.policy.revision, 1)
})
test('cadastro ilegível mostra erro e conserva guias, rascunho e referências', async () => {
  const f = setup(); await f.model.read()
  const policy = { mode: 'auto' as const, guide: 'específico preservado', reserves: [{ accountId: 'conta-salva', accountRevision: 1, selection: { backend: 'codex', model: 'modelo-salvo', modelOptions: {} } }] }
  f.model.edit(policy, 'geral preservado')
  f.failRead({ message: 'accounts_unavailable: SENTINELA_PRIVADA' })
  await f.model.read(true)
  assert.match(f.model.getSnapshot().error!, /cadastro de contas/)
  assert.ok(!f.model.getSnapshot().error!.includes('SENTINELA'))
  assert.deepEqual(f.model.getSnapshot().policy, policy)
  assert.equal(f.model.getSnapshot().generalGuide, 'geral preservado')
  assert.equal(f.model.getSnapshot().dirty, true)
  assert.equal(f.model.getSnapshot().view?.policy.revision, 0)
})
test('factory despacha sem await no cliente, Hub e Space originais e descarta a resposta superada', async () => {
  const calls: { client: string; method: string; body: Record<string, unknown> }[] = []
  let waiting: Promise<RoutingView> | null = null
  let release: ((value: RoutingView) => void) | undefined
  const answer = { ...view(), hubId: 'hub-a', spaceId: 'space-a' }
  const client = (name: string): NonNullable<RoutingSource['connectedClient']> => ({ call<T>(method: string, body: Record<string, unknown>) { calls.push({ client: name, method, body: structuredClone(body) }); return (waiting ?? Promise.resolve(structuredClone(answer))) as Promise<T> } })
  const a = client('a'), b = client('b')
  const source = { connectedClient: a, hello: { hostId: 'hub-a' }, spaceId: 'space-a', principalEpoch: 1, bots: new Map([['dev', fixedProfile()]]), subscribe: () => () => {} }
  const model = createRoutingStore(source, { botId: 'dev', hubId: 'hub-a', spaceId: 'space-a' }, fixedEditor())
  const reading = model.read()
  assert.equal(calls.length, 1, 'a chamada acontece antes de devolver a Promise, sem fila/ready')
  assert.deepEqual(calls[0], { client: 'a', method: 'donnaRouting', body: { botId: 'dev', hubId: 'hub-a', spaceId: 'space-a' } })
  await reading
  const checking = model.read(true)
  assert.equal(calls[1]?.method, 'donnaRoutingCheck')
  assert.equal(calls[1]?.client, 'a')
  await checking
  model.edit({ mode: 'auto', guide: 'rascunho', reserves: [] }, 'geral')
  waiting = new Promise<RoutingView>((resolve) => { release = resolve })
  const saving = model.save()
  assert.equal(calls.length, 3)
  assert.deepEqual(calls[2], { client: 'a', method: 'donnaRoutingSave', body: { botId: 'dev', expectedRevision: 0, expectedGeneralRevision: 0, policy: { mode: 'auto', guide: 'rascunho', reserves: [] }, generalGuide: 'geral', hubId: 'hub-a', spaceId: 'space-a' } })
  source.connectedClient = b; source.hello = { hostId: 'hub-b' }; source.spaceId = 'space-b'; source.principalEpoch++
  release?.({ ...answer, policy: { ...answer.policy, revision: 1 } }); await saving
  assert.equal(calls.filter((call) => call.client === 'b').length, 0)
  assert.equal(model.getSnapshot().view?.policy.revision, 0)
  assert.equal(model.getSnapshot().policy?.guide, 'rascunho')
  assert.equal(model.getSnapshot().message, null)
})
test('factory não entrega payload antigo ao cliente substituído antes do dispatch síncrono', async () => {
  const calls: string[] = []
  const client = (name: string): NonNullable<RoutingSource['connectedClient']> => ({ call<T>() { calls.push(name); return Promise.resolve(view() as T) } })
  const source = { connectedClient: client('a'), hello: { hostId: 'hub-a' }, spaceId: 'space-a', principalEpoch: 1, bots: new Map([['dev', fixedProfile()]]), subscribe: () => () => {} }
  const model = createRoutingStore(source, { botId: 'dev', hubId: 'hub-a', spaceId: 'space-a' }, fixedEditor())
  let switched = false
  model.subscribe(() => { if (!switched && model.getSnapshot().busy) { switched = true; source.connectedClient = client('b'); source.principalEpoch++ } })
  await model.read()
  assert.deepEqual(calls, [])
  assert.equal(model.getSnapshot().view, null)
  assert.equal(model.getSnapshot().busy, false)
})

test('Fixo pending e resposta antiga não perdem guia/reservas nem CAS; nome não invalida conferência', async () => {
  const listeners = new Set<() => void>(), mirrorListeners = new Set<() => void>()
  const first = { name: 'Antes', backend: 'codex', model: 'modelo-a', modelOptions: { effort: 'high' }, forkEditor: { engineAccountId: 'conta-a' } }
  let settings = { botId: 'dev', editor: { profile: first }, draft: first, saving: false, loading: false, error: null as string | null }
  const editor = { getSnapshot: () => settings, subscribe: (fn: () => void) => { listeners.add(fn); return () => { listeners.delete(fn) } } }
  const change = (patch: Partial<typeof settings>) => { settings = { ...settings, ...patch }; for (const fn of listeners) fn() }
  const calls: { method: string; body: Record<string, unknown> }[] = []
  let answer = { ...view(), hubId: 'hub-a', spaceId: 'space-a' }
  let waiting: Promise<RoutingView> | undefined, release: ((value: RoutingView) => void) | undefined
  const source = {
    connectedClient: { call<T>(method: string, body: Record<string, unknown>) { calls.push({ method, body: structuredClone(body) }); return (waiting ?? Promise.resolve(structuredClone(answer))) as Promise<T> } },
    hello: { hostId: 'hub-a' }, principalEpoch: 1, spaceId: 'space-a', bots: new Map([['dev', first]]),
    subscribe: (fn: () => void) => { mirrorListeners.add(fn); return () => { mirrorListeners.delete(fn) } },
  }
  const mirror = (profile: typeof first) => { source.bots.set('dev', profile); for (const fn of mirrorListeners) fn() }
  let fixedPending = false
  const model = createRoutingStore(source, { botId: 'dev', hubId: 'hub-a', spaceId: 'space-a', fixedPending: () => fixedPending }, editor)
  const stop = model.subscribe(() => {})
  try {
    await model.read()
    assert.equal(model.getSnapshot().fixedCurrent, true)
    change({ draft: { ...first, name: 'Nome não invalida Fixo' } })
    assert.equal(model.getSnapshot().fixedReady, true)
    assert.equal(model.getSnapshot().fixedCurrent, true)
    fixedPending = true; model.fixedChanged()
    const beforePending = calls.length
    await model.read(true)
    assert.equal(calls.length, beforePending)
    fixedPending = false; model.fixedChanged()
    assert.equal(model.getSnapshot().fixedReady, true, 'fim da intenção pendente libera nova conferência')
    const policy = { mode: 'auto' as const, guide: 'enviado', reserves: [{ accountId: 'reserva', accountRevision: 1, selection: { backend: 'codex', model: 'reserva-a', modelOptions: {} } }] }
    model.edit(policy, 'geral enviado')
    waiting = new Promise<RoutingView>((resolve) => { release = resolve })
    const saving = model.save()
    const next = { ...first, backend: 'claude', model: 'modelo-b', modelOptions: { effort: 'low' }, forkEditor: { engineAccountId: 'conta-b' } }
    change({ draft: next, saving: true })
    model.edit({ ...policy, guide: 'texto durante save' }, 'geral durante save')
    assert.equal(model.getSnapshot().fixedReady, false)
    assert.equal(model.getSnapshot().fixedCurrent, false)
    release?.({ ...answer, policy: { ...answer.policy, revision: 1, value: policy }, generalGuide: { ...answer.generalGuide, revision: 1, value: 'geral enviado' } })
    await saving
    assert.equal(model.getSnapshot().view?.policy.revision, 1, 'save válido confirma CAS apesar da troca de Fixo')
    assert.equal(model.getSnapshot().view?.generalGuide.revision, 1)
    assert.equal(model.getSnapshot().policy?.guide, 'texto durante save')
    assert.equal(model.getSnapshot().generalGuide, 'geral durante save')
    assert.deepEqual(model.getSnapshot().policy?.reserves, policy.reserves)
    assert.match(model.getSnapshot().message!, /Preferência guardada/)
    const sent = calls.length
    await model.read(true)
    assert.equal(calls.length, sent, 'não despacha conferência enquanto Fixo está pendente')
    change({ editor: { profile: next }, draft: next, saving: false })
    assert.equal(model.getSnapshot().fixedReady, false, 'aguarda editor e mirror concordarem')
    mirror(next)
    assert.equal(model.getSnapshot().fixedReady, true)
    assert.equal(model.getSnapshot().fixedCurrent, false)
    waiting = undefined
    answer = { ...answer, policy: { ...answer.policy, revision: 99 }, generalGuide: { ...answer.generalGuide, revision: 99 } }
    await model.read(true)
    assert.equal(model.getSnapshot().fixedCurrent, true)
    assert.equal(model.getSnapshot().view?.policy.revision, 1, 'conferir não avança CAS do rascunho')
    waiting = new Promise<RoutingView>((resolve) => { release = resolve })
    const checking = model.read(true)
    const latest = { ...next, modelOptions: { effort: 'high' } }
    mirror(latest)
    release?.(answer); await checking
    assert.equal(model.getSnapshot().fixedCurrent, false, 'outra combinação não restaura indicação')
    assert.equal(model.getSnapshot().message, null)
    assert.equal(model.getSnapshot().view?.policy.revision, 1)
    change({ editor: { profile: latest }, draft: latest })
    waiting = undefined
    await model.read(true)
    assert.equal(model.getSnapshot().fixedCurrent, true)
    answer = { ...answer, policy: { ...answer.policy, revision: 2, value: model.getSnapshot().policy! }, generalGuide: { ...answer.generalGuide, revision: 2, value: model.getSnapshot().generalGuide } }
    await model.save()
    assert.equal(calls.at(-1)?.body.expectedRevision, 1)
    assert.equal(calls.at(-1)?.body.expectedGeneralRevision, 1)
    assert.equal(model.getSnapshot().view?.policy.revision, 2)
    assert.equal(model.getSnapshot().dirty, false)
  } finally { stop() }
})
test('recibo: chamada só no Hub e Space do contexto, sem campo extra', async () => {
  const calls: { method: string; body: Record<string, unknown> }[] = []
  const client = { call<T>(method: string, body: Record<string, unknown>) { calls.push({ method, body }); return Promise.resolve({ hubId: 'hub-a', state: 'pending', receipt: null } as T) } }
  const source = { connectedClient: client, hello: { hostId: 'hub-a' }, spaceId: 'space-a', principalEpoch: 1, bots: new Map(), subscribe: () => () => {} }
  const context = { botId: 'dev', hubId: 'hub-a', spaceId: 'space-a' }
  assert.equal((await readRoutingReceipt(source, context, { conversationId: 'c1', sessionId: 's1', entryId: 'e1' })).state, 'pending')
  assert.deepEqual(calls, [{ method: 'donnaRoutingReceipt', body: { conversationId: 'c1', sessionId: 's1', entryId: 'e1', botId: 'dev', hubId: 'hub-a', spaceId: 'space-a' } }])
  source.hello = { hostId: 'hub-b' }
  await assert.rejects(readRoutingReceipt(source, context, { conversationId: 'c1', sessionId: 's1', entryId: 'e1' }))
  assert.equal(calls.length, 1)
})

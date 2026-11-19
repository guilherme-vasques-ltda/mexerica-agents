import assert from 'node:assert/strict'
import { test } from 'node:test'
import { BackgroundStore, accountOptions, grupoDe, canBind, codexRefused, readFunctions, savedAccountId, readHistory, readSpaces, reasonText, shownReason, suggestedAccount, reasonCodeText, taskKey, accountDisconnected, spaceChoiceIssue, spaceChoiceText, selecaoDoSpace, spaceDaPagina, computadorDaPagina, type Transport } from './model.ts'
import { installBackgroundFixture } from './demo.ts'

function fixture() {
  const host = { async call(_method: string, _body: Record<string, unknown>): Promise<unknown> { throw new Error('ausente') }, emit(_event: object) {} }
  const fake = installBackgroundFixture(host)
  const client: Transport = { async call<T>(method: string, body: unknown = {}) { return await host.call(method, body as Record<string, unknown>) as T } }
  const model = new BackgroundStore(async () => client)
  return { model, fake, host, client }
}

test('abrir usa só APIs entregues; histórico ausente fica indisponível sem fallback ou simulação', async () => {
  const { model, fake } = fixture()
  await model.load()
  assert.ok(model.getSnapshot().data)
  assert.equal(model.getSnapshot().spaces, null)
  assert.equal(model.getSnapshot().spacesError, 'indisponivel')
  assert.equal(model.getSnapshot().history, null)
  assert.equal(model.getSnapshot().historyError, 'indisponivel')
  assert.deepEqual(fake.requests.map((r) => r.method), ['backgroundHistory', 'functionModels', 'backgroundSpaceSelections'])
})

test('salvar preserva conta/modelo/opção nativa e revisão; evento de outra edição não autoaceita rascunho', async () => {
  const { model, fake } = fixture()
  await model.load()
  model.change({ modelOptions: { reasoning_effort: 'max' } })
  fake.data.document!.revision = 2
  await model.load()
  assert.equal(model.getSnapshot().conflict, true)
  assert.equal(model.getSnapshot().draft!.modelOptions.reasoning_effort, 'max')
  await model.save()
  assert.equal(fake.requests.some((r) => r.method === 'saveFunctionModel'), false)
  await model.load(true)
  model.change({ modelOptions: { reasoning_effort: 'max' } })
  await model.save()
  const request = fake.requests.find((r) => r.method === 'saveFunctionModel')!
  assert.equal(request.body.expectedRevision, 2)
  assert.deepEqual(request.body.selection, { mode: 'custom', accountId: fake.data.accounts[0]!.id, modelId: 'haiku', modelOptions: { reasoning_effort: 'max' } })
})

test('Space referencia seleção sem Binding duplicado e remoção funciona com current null', async () => {
  const { model, fake } = fixture()
  fake.setSpaceAvailable(true)
  await model.load()
  await model.saveSpace(model.getSnapshot().spaces!.selections[0]!, true)
  const enabled = fake.requests.find((r) => r.method === 'saveBackgroundSpaceSelection')!.body
  assert.equal(Object.hasOwn(enabled, 'modelId'), false)
  assert.equal(enabled.expectedRevision, 0)
  assert.equal(enabled.functionRevision, 1)
  fake.spaces.selections[0]!.current = null
  fake.spaces.selections[0]!.state = 'stale'
  await model.load()
  await model.saveSpace(model.getSnapshot().spaces!.selections[0]!, false)
  const disabled = fake.requests.filter((r) => r.method === 'saveBackgroundSpaceSelection').at(-1)!.body
  assert.equal(disabled.enabled, false)
  assert.equal(Object.hasOwn(disabled, 'accountId'), false)
  assert.equal(disabled.expectedSpaceRevision, 1)
})

test('mudança de computador/conexão antes de ready não envia intenção antiga ao novo destino', async () => {
  const { client, fake } = fixture()
  let scope = 'old'
  let resolve!: (client: Transport) => void
  const model = new BackgroundStore(() => new Promise<Transport>((done) => { resolve = done }), () => scope)
  const load = model.load()
  scope = 'new'
  resolve(client)
  await load
  assert.equal(fake.requests.length, 0)
  assert.equal(model.getSnapshot().data, null)
})

test('respostas sem documento/resolução ou audiência correta não habilitam configuração', () => {
  const { fake } = fixture()
  assert.throws(() => readFunctions({ ...fake.data, document: {} }))
  assert.throws(() => readFunctions({ ...fake.data, functions: [{ ...fake.data.functions[0], resolution: { status: 'available' } }] }))
  assert.throws(() => readSpaces({ ...fake.spaces, selections: [{ ...fake.spaces.selections[0], audience: 'public' }] }))
})

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: Error) => void
  const promise = new Promise<T>((ok, error) => { resolve = ok; reject = error })
  return { promise, resolve, reject }
}

/** Drena as continuações independentes do histórico, sem esperar sua conclusão em load. */
const nextTurn = () => new Promise<void>((resolve) => setImmediate(resolve))

test('evento lê autorização mais nova antes do retorno antigo da mutação; só refetch publica o estado final', async () => {
  const { fake, client } = fixture()
  fake.setSpaceAvailable(true)
  const response = deferred<unknown>()
  const sent = deferred<void>()
  const transport: Transport = { async call<T>(method: string, body: unknown = {}) {
    if (method === 'saveBackgroundSpaceSelection') { sent.resolve(); return await response.promise as T }
    return client.call<T>(method, body)
  } }
  const model = new BackgroundStore(async () => transport)
  await model.load()
  const mutation = model.saveSpace(model.getSnapshot().spaces!.selections[0]!, true)
  await sent.promise
  const oldResponse = structuredClone(fake.spaces)
  oldResponse.selections[0]!.revision = 1
  oldResponse.selections[0]!.state = 'configured'
  fake.spaces.selections[0]!.revision = 3
  fake.spaces.selections[0]!.state = 'stale'
  await model.load() // evento emitido pelo Host antes de responder ao RPC
  assert.equal(model.getSnapshot().spaces!.selections[0]!.revision, 3)
  const published: number[] = []
  const unsubscribe = model.subscribe(() => { const rev = model.getSnapshot().spaces?.selections[0]?.revision; if (rev !== undefined) published.push(rev) })
  response.resolve(oldResponse)
  await mutation
  unsubscribe()
  assert.equal(published.includes(1), false, 'resposta da mutação não publica snapshot antigo nem por um instante')
  assert.equal(model.getSnapshot().spaces!.selections[0]!.state, 'stale')
  assert.equal(model.getSnapshot().spaces!.selections[0]!.revision, 3)
  assert.equal(fake.requests.filter((r) => r.method === 'backgroundSpaceSelections').length, 3, 'leitura inicial, evento e reconciliação pós-RPC')
})

test('retorno antigo inicia refetch, e leitura ainda mais nova vence a reconciliação em voo', async () => {
  const { fake, client } = fixture()
  fake.setSpaceAvailable(true)
  const reconcile = deferred<unknown>()
  const started = deferred<void>()
  let spaceReads = 0
  const transport: Transport = { async call<T>(method: string, body: unknown = {}) {
    if (method === 'backgroundSpaceSelections' && ++spaceReads === 2) { started.resolve(); return await reconcile.promise as T }
    return client.call<T>(method, body)
  } }
  const model = new BackgroundStore(async () => transport)
  await model.load()
  const mutation = model.saveSpace(model.getSnapshot().spaces!.selections[0]!, true)
  await started.promise
  const earlier = structuredClone(fake.spaces)
  fake.spaces.selections[0]!.revision = 4
  fake.spaces.selections[0]!.state = 'stale'
  await model.load()
  reconcile.resolve(earlier)
  await mutation
  assert.equal(model.getSnapshot().spaces!.selections[0]!.revision, 4)
  assert.equal(model.getSnapshot().spaces!.selections[0]!.state, 'stale')
})

test('erro tardio da mutação reconcilia sem apagar a leitura mais nova nem repetir a intenção', async () => {
  const { fake, client } = fixture()
  fake.setSpaceAvailable(true)
  const response = deferred<unknown>(), sent = deferred<void>()
  let mutations = 0
  const transport: Transport = { async call<T>(method: string, body: unknown = {}) {
    if (method === 'saveBackgroundSpaceSelection') { mutations++; sent.resolve(); return await response.promise as T }
    return client.call<T>(method, body)
  } }
  const model = new BackgroundStore(async () => transport)
  await model.load()
  const mutation = model.saveSpace(model.getSnapshot().spaces!.selections[0]!, true)
  await sent.promise
  fake.spaces.selections[0]!.revision = 5
  await model.load()
  response.reject(new Error('confirmação perdida'))
  await mutation
  assert.equal(model.getSnapshot().spaces!.selections[0]!.revision, 5)
  assert.equal(mutations, 1)
})

test('releitura por evento invalida check disponível, inclusive se a resolução fica indisponível na mesma revisão', async () => {
  const { fake, client } = fixture()
  const available = { status: 'available', resolved: { effective: { accountId: fake.data.accounts[0]!.id, modelId: 'haiku', modelOptions: {} } } }
  const transport: Transport = { async call<T>(method: string, body: unknown = {}) {
    return method === 'checkFunctionModel' ? available as T : client.call<T>(method, body)
  } }
  const model = new BackgroundStore(async () => transport)
  await model.load()
  await model.check()
  assert.equal(model.getSnapshot().check!.status, 'available')
  fake.data.functions.find((f) => f.definition.id === 'memory_consolidation')!.resolution = { status: 'unavailable', unavailable: { reason: 'account_disconnected' } }
  const reading = model.load()
  assert.equal(model.getSnapshot().check, null, 'check cai assim que começa a releitura')
  await reading
  assert.equal(model.getSnapshot().check, null)
  const resolution = model.getSnapshot().data!.functions.find((f) => f.definition.id === 'memory_consolidation')!.resolution
  assert.equal(resolution.status, 'unavailable')
  assert.equal(model.getSnapshot().data!.document!.revision, 1)
})

test('check em voo anterior a um refetch não recoloca available sobre a nova resolução unavailable', async () => {
  const { fake, client } = fixture()
  const response = deferred<unknown>(), sent = deferred<void>()
  const transport: Transport = { async call<T>(method: string, body: unknown = {}) {
    if (method === 'checkFunctionModel') { sent.resolve(); return await response.promise as T }
    return client.call<T>(method, body)
  } }
  const model = new BackgroundStore(async () => transport)
  await model.load()
  const check = model.check()
  await sent.promise
  fake.data.functions.find((f) => f.definition.id === 'memory_consolidation')!.resolution = { status: 'unavailable', unavailable: { reason: 'quota_unknown' } }
  await model.load()
  response.resolve({ status: 'available', resolved: { effective: { accountId: fake.data.accounts[0]!.id, modelId: 'haiku', modelOptions: {} } } })
  await check
  assert.equal(model.getSnapshot().check, null)
  assert.equal(model.getSnapshot().data!.functions.find((f) => f.definition.id === 'memory_consolidation')!.resolution.status, 'unavailable')
})

test('queda segurada deixa connection online mas connectedClient nulo: nenhuma intenção aguarda reconexão', async () => {
  const { fake, client } = fixture()
  fake.setSpaceAvailable(true)
  let usable: Transport | null = client
  const source = { connection: { kind: 'online' }, get connectedClient() { return usable } }
  let queuedReady = 0
  const model = new BackgroundStore(async () => { queuedReady++; if (!source.connectedClient) throw new Error('sem link'); return source.connectedClient }, () => 'hub/epoch', () => source.connectedClient)
  await model.load()
  const row = model.getSnapshot().spaces!.selections[0]!
  const callsBeforeDrop = fake.requests.length, readyBeforeDrop = queuedReady
  usable = null // contrato público de BotSync.live durante heldDrop, antes dos 5 s
  assert.equal(source.connection.kind, 'online')
  await model.check()
  model.change({ modelOptions: { reasoning_effort: 'max' } })
  await model.save()
  await model.saveSpace(row, false)
  await model.load()
  assert.equal(queuedReady, readyBeforeDrop, 'não chama ready nem espera o próximo link')
  assert.equal(fake.requests.length, callsBeforeDrop)
  usable = client
  await Promise.resolve()
  assert.equal(fake.requests.length, callsBeforeDrop, 'reconectar não dispara intenção antiga')
})

test('dono remoto com functionModels recusado usa projeção Space para vincular, reautorizar e remover por CAS', async () => {
  const { fake, client } = fixture()
  fake.setSpaceAvailable(true)
  const writes: Record<string, unknown>[] = []
  const transport: Transport = { async call<T>(method: string, body: unknown = {}) {
    if (method === 'functionModels') throw new Error('403: Local only')
    if (method === 'saveBackgroundSpaceSelection') {
      const request = body as Record<string, unknown>
      const expected = request.enabled ? ['accountId', 'accountRevision', 'enabled', 'expectedRevision', 'expectedSpaceRevision', 'functionId', 'functionRevision', 'spaceId']
        : ['enabled', 'expectedRevision', 'expectedSpaceRevision', 'functionId', 'spaceId']
      assert.deepEqual(Object.keys(request).sort(), expected.sort(), 'deny_unknown_fields não recebe projeção ou catálogo')
      writes.push(structuredClone(request))
    }
    return client.call<T>(method, body)
  } }
  const model = new BackgroundStore(async () => transport)
  await model.load()
  assert.equal(model.getSnapshot().data, null)
  assert.equal(model.getSnapshot().error, 'leitura')
  let row = model.getSnapshot().spaces!.selections[0]!
  assert.deepEqual(spaceChoiceText(null, row), { label: 'Claude de exemplo · haiku' })
  assert.equal(canBind(null, row), true)
  await model.saveSpace(row, true)
  assert.equal(model.getSnapshot().spaces!.selections[0]!.state, 'configured')
  fake.spaces.selections[0]!.state = 'stale'
  fake.spaces.selections[0]!.current!.functionRevision = 2
  fake.spaces.selections[0]!.current!.accountRevision = 3
  await model.load()
  row = model.getSnapshot().spaces!.selections[0]!
  await model.saveSpace(row, true)
  assert.equal(writes[1]!.expectedRevision, 1)
  assert.equal(writes[1]!.functionRevision, 2)
  assert.equal(writes[1]!.accountRevision, 3)
  await model.saveSpace(model.getSnapshot().spaces!.selections[0]!, false)
  assert.equal(writes[2]!.expectedRevision, 2)
  assert.equal(writes[2]!.enabled, false)
  assert.equal(model.getSnapshot().spaces!.selections[0]!.state, 'not_authorized')
  assert.equal(fake.requests.some((r) => ['saveFunctionModel', 'checkFunctionModel'].includes(r.method)), false)
})

test('documento global acessível divergente de revisão, conta ou modelo bloqueia vínculo sem misturar escolhas', async () => {
  for (const patch of [{ functionRevision: 2 }, { accountId: `ac_${'b'.repeat(64)}` }, { modelId: 'outro-modelo' }]) {
    const { model, fake } = fixture()
    fake.setSpaceAvailable(true)
    Object.assign(fake.spaces.selections[0]!.current!, patch)
    await model.load()
    const { data, spaces } = model.getSnapshot(), row = spaces!.selections[0]!
    assert.equal(canBind(data, row), false)
    assert.equal(spaceChoiceIssue(data, row), 'fundo.espaco.divergem')
    assert.deepEqual(spaceChoiceText(data, row), { key: 'fundo.espaco.aguardandoReleitura' })
    await model.saveSpace(row, true)
    assert.equal(fake.requests.some((r) => r.method === 'saveBackgroundSpaceSelection'), false)
  }
})

test('projeção Space sem rótulo/modelo válidos falha fechada', () => {
  const { fake } = fixture(), row = fake.spaces.selections[0]!
  for (const patch of [{ accountLabel: undefined }, { accountLabel: '' }, { modelId: undefined }, { modelId: '' }]) {
    assert.throws(() => readSpaces({ ...fake.spaces, selections: [{ ...row, current: { ...row.current, ...patch } }] }))
  }
})


test('clicar na função selecionada preserva rascunho e conflito sem descarte implícito', async () => {
  const { model, fake } = fixture()
  await model.load()
  model.change({ modelOptions: { reasoning_effort: 'max' } })
  const before = structuredClone(model.getSnapshot())
  model.select(before.selectedId)
  assert.deepEqual(model.getSnapshot().draft, before.draft)
  assert.equal(model.getSnapshot().dirty, true)
  assert.equal(model.getSnapshot().conflict, false)
  fake.data.document!.revision++
  await model.load()
  const conflict = structuredClone(model.getSnapshot())
  assert.equal(conflict.conflict, true)
  model.select(conflict.selectedId)
  assert.deepEqual(model.getSnapshot().draft, conflict.draft)
  assert.equal(model.getSnapshot().dirty, true)
  assert.equal(model.getSnapshot().conflict, true)
})

const historyEntry = (patch: Record<string, unknown> = {}) => ({ id: 'history-id', botId: 'bot', function: 'memory', status: 'applied', at: 1793718000000, effectiveModels: [], failureStage: null, ...patch })

test('histórico exige status tipado; modelo vazio e metadados opcionais não viram seleção nem zero', () => {
  for (const status of ['response_received', 'applied', 'unchanged', 'proposed', 'deferred', 'failed']) {
    const [row] = readHistory([historyEntry({ status })])
    assert.equal(row!.status, status)
    assert.deepEqual(row!.effectiveModels, [])
    assert.equal(row!.usage, null)
    assert.equal(row!.failureStage, null)
  }
  assert.deepEqual(readHistory([]), [])
  assert.equal(readHistory([historyEntry({ failureStage: undefined, usage: null, at: 0 })])[0]!.at, 0)
  assert.throws(() => readHistory([historyEntry({ status: undefined })]))
  assert.throws(() => readHistory([historyEntry({ status: 'unknown' })]))
})

test('historyMalformed/missing falha fechado: array limitado, função, data, modelos e etapa', () => {
  for (const raw of [undefined, null, {}, { entries: [] }, Array.from({ length: 51 }, () => historyEntry())]) assert.throws(() => readHistory(raw))
  for (const patch of [{ function: 'other' }, { id: undefined }, { botId: undefined }, { at: -1 }, { at: NaN }, { at: 9e15 }, { effectiveModels: undefined }, { effectiveModels: [null] }, { effectiveModels: [''] }, { failureStage: 'raw-error' }]) {
    assert.throws(() => readHistory([historyEntry(patch)]))
  }
})

test('histórico projeta somente os quatro tokens; omite conta/preço/erro livre e rejeita uso incompleto ou inválido', () => {
  const usage = { inputTokens: 0, outputTokens: 12, cachedInputTokens: 3, cachedWriteTokens: 4 }
  const raw = historyEntry({ effectiveModels: ['modelo-real'], status: 'failed', failureStage: 'revalidation', usage: { ...usage, estimatedCost: 15, currency: 'BRL', account: 'privada' }, account: 'privada', error: 'ERRO LIVRE', cost: 99 })
  const [row] = readHistory([raw])
  assert.deepEqual(row!.usage, usage)
  assert.deepEqual(row!.effectiveModels, ['modelo-real'])
  assert.equal(row!.failureStage, 'revalidation')
  assert.deepEqual(Object.keys(row!).sort(), ['id', 'botId', 'function', 'status', 'at', 'effectiveModels', 'usage', 'failureStage'].sort())
  assert.equal(JSON.stringify(row).includes('privada'), false)
  assert.equal(JSON.stringify(row).includes('ERRO LIVRE'), false)
  for (const patch of [{ inputTokens: -1 }, { outputTokens: 1.5 }, { cachedInputTokens: '3' }, { cachedWriteTokens: Number.MAX_SAFE_INTEGER + 1 }, { outputTokens: undefined }]) {
    assert.throws(() => readHistory([historyEntry({ usage: { ...usage, ...patch } })]))
  }
})

test('histórico 403/API ausente e resposta malformada ocultam dados antigos sem fallback nem erro bruto', async () => {
  const { model, fake } = fixture()
  fake.setHistory([historyEntry()])
  await model.load()
  assert.equal(model.getSnapshot().history!.length, 1)
  fake.setHistoryAvailable(false)
  const pending = model.load()
  assert.equal(model.getSnapshot().history, null)
  assert.equal(model.getSnapshot().historyLoading, true)
  await pending
  assert.equal(model.getSnapshot().history, null)
  assert.equal(model.getSnapshot().historyError, 'indisponivel')
  fake.setHistory([historyEntry({ status: undefined })])
  await model.load()
  assert.equal(model.getSnapshot().history, null)
  assert.equal(model.getSnapshot().historyError, 'invalido')
  assert.equal(fake.requests.every((r) => ['backgroundHistory', 'functionModels', 'backgroundSpaceSelections'].includes(r.method)), true)
  const remote = new BackgroundStore(async () => ({ async call<T>(method: string) {
    if (method === 'backgroundHistory') throw new Error('403: DETALHE PRIVADO')
    return await fakeHostCall(method) as T
  } }))
  const fakeHostCall = async (method: string) => method === 'functionModels' ? fake.data : fake.spaces
  await remote.load()
  assert.equal(remote.getSnapshot().history, null)
  assert.equal(remote.getSnapshot().historyError, 'indisponivel')
})

test('histórico atrasado não bloqueia publicação das escolhas nem substitui releitura mais nova', async () => {
  const { fake, client } = fixture()
  fake.setHistory([historyEntry()])
  const responses = [deferred<unknown>(), deferred<unknown>()], started = [deferred<void>(), deferred<void>()]
  let historyReads = 0
  const transport: Transport = { async call<T>(method: string, body: unknown = {}) {
    if (method === 'backgroundHistory') { const index = historyReads++; started[index]!.resolve(); return await responses[index]!.promise as T }
    return client.call<T>(method, body)
  } }
  const model = new BackgroundStore(async () => transport)
  const choices = deferred<void>()
  const unsubscribe = model.subscribe(() => { if (model.getSnapshot().data && !model.getSnapshot().loading) choices.resolve() })
  const first = model.load()
  await started[0]!.promise
  await choices.promise
  assert.ok(model.getSnapshot().data)
  assert.equal(model.getSnapshot().historyLoading, true)
  assert.equal(model.getSnapshot().history, null)
  const second = model.load()
  await started[1]!.promise
  responses[1]!.resolve([historyEntry({ id: 'new', status: 'failed', failureStage: null })])
  await second
  await nextTurn()
  responses[0]!.resolve([historyEntry({ id: 'old' })])
  await first
  await nextTurn()
  unsubscribe()
  assert.equal(model.getSnapshot().history![0]!.id, 'new')
  assert.equal(model.getSnapshot().history![0]!.status, 'failed')
})

test('histórico em voo não atravessa troca de scope/link ou fechamento; queda elimina consulta anterior', async () => {
  for (const change of ['scope', 'link', 'close']) {
    const { client } = fixture()
    const response = deferred<unknown>(), started = deferred<void>()
    let scope = 'first', usable: Transport | null
    const transport: Transport = { async call<T>(method: string, body: unknown = {}) {
      if (method === 'backgroundHistory') { started.resolve(); return await response.promise as T }
      return client.call<T>(method, body)
    } }
    usable = transport
    const model = new BackgroundStore(async () => transport, () => scope, () => usable)
    const pending = model.load()
    await started.promise
    if (change === 'scope') scope = 'second'
    else if (change === 'link') usable = null
    else model.close()
    response.resolve([historyEntry({ id: 'old-scope' })])
    await pending
    await nextTurn()
    assert.equal(model.getSnapshot().history, null)
  }
  const { client, fake } = fixture()
  fake.setHistory([historyEntry()])
  let link: Transport | null = client
  const model = new BackgroundStore(async () => client, () => 'same', () => link)
  await model.load()
  assert.equal(model.getSnapshot().history!.length, 1)
  link = null
  const before = fake.requests.length
  await model.load()
  assert.equal(model.getSnapshot().history, null)
  assert.equal(fake.requests.length, before)
})

test('salvar escolha conclui e libera busy enquanto o histórico pós-mutação continua retido', async () => {
  const { fake, client } = fixture()
  fake.setHistory([historyEntry({ id: 'before' })])
  const response = deferred<unknown>(), started = deferred<void>()
  let hold = false, completed = false
  const transport: Transport = { async call<T>(method: string, body: unknown = {}) {
    if (method === 'backgroundHistory' && hold) { started.resolve(); return await response.promise as T }
    return client.call<T>(method, body)
  } }
  const model = new BackgroundStore(async () => transport)
  await model.load()
  hold = true
  model.change({ modelOptions: { reasoning_effort: 'max' } })
  const saving = model.save().then(() => { completed = true })
  await started.promise
  await nextTurn()
  try {
    assert.equal(model.getSnapshot().data!.document!.revision, 2)
    assert.equal(model.getSnapshot().loading, false)
    assert.equal(completed, true, 'a mutação termina sem esperar o RPC histórico')
    assert.equal(model.getSnapshot().busy, false)
    assert.equal(model.getSnapshot().historyLoading, true)
    assert.equal(model.getSnapshot().history, null, 'não reapresenta o histórico anterior')
  } finally {
    response.reject(new Error('403: histórico local indisponível'))
    await saving
    await nextTurn()
  }
  assert.equal(model.getSnapshot().busy, false)
  assert.equal(model.getSnapshot().history, null)
  assert.equal(model.getSnapshot().historyError, 'indisponivel')
  assert.equal(fake.requests.filter((r) => r.method === 'saveFunctionModel').length, 1)
})

test('salvar Space libera busy sem aguardar histórico; leitura nova vence seu retorno tardio', async () => {
  const { fake, client } = fixture()
  fake.setSpaceAvailable(true)
  fake.setHistory([historyEntry({ id: 'before' })])
  const response = deferred<unknown>(), started = deferred<void>()
  let hold = false, completed = false
  const transport: Transport = { async call<T>(method: string, body: unknown = {}) {
    if (method === 'backgroundHistory' && hold) { started.resolve(); return await response.promise as T }
    return client.call<T>(method, body)
  } }
  const model = new BackgroundStore(async () => transport)
  await model.load()
  hold = true
  const saving = model.saveSpace(model.getSnapshot().spaces!.selections[0]!, true).then(() => { completed = true })
  await started.promise
  await nextTurn()
  try {
    assert.equal(model.getSnapshot().spaces!.selections[0]!.state, 'configured')
    assert.equal(model.getSnapshot().loading, false)
    assert.equal(completed, true, 'CAS e refetch das autorizações não aguardam o histórico')
    assert.equal(model.getSnapshot().busy, false)
    assert.equal(model.getSnapshot().historyLoading, true)
    assert.equal(model.getSnapshot().history, null)
    hold = false
    fake.setHistory([historyEntry({ id: 'newer' })])
    await model.load()
    await nextTurn()
  } finally {
    response.resolve([historyEntry({ id: 'late-old' })])
    await saving
    await nextTurn()
  }
  assert.equal(model.getSnapshot().history![0]!.id, 'newer')
  assert.equal(model.getSnapshot().busy, false)
  assert.equal(fake.requests.filter((r) => r.method === 'saveBackgroundSpaceSelection').length, 1)
})

test('automático sem escolha e sem conta elegível orienta Memória e Títulos sem alterar seleção ou revisão', async () => {
  const { model, fake } = fixture()
  fake.data.accounts[0]!.state = 'unverified'
  fake.data.accounts[0]!.models = []
  for (const id of ['memory_consolidation', 'bot_naming']) {
    const row = fake.data.functions.find((row) => row.definition.id === id)!
    row.selection = { mode: 'automatic', accountId: null, binding: null }
    row.resolution = { status: 'unavailable', unavailable: { reason: 'account_missing' } }
    fake.data.document!.functions[id] = row.selection
  }
  const before = structuredClone(fake.data)
  await model.load()
  for (const id of ['memory_consolidation', 'bot_naming']) {
    const row = model.getSnapshot().data!.functions.find((row) => row.definition.id === id)!
    assert.equal(row.resolution.status, 'unavailable')
    assert.equal(reasonText('account_missing', row), 'fundo.motivo.semContaMemoria')
  }
  const other = { ...before.functions.find((row) => row.definition.id === 'vision')!, selection: { mode: 'automatic' as const, accountId: null, binding: null } }
  assert.equal(reasonText('account_missing', other), 'fundo.motivo.semConta')
  assert.deepEqual(model.getSnapshot().data, before)
  assert.equal(fake.requests.some((request) => request.method.startsWith('save') || request.method.startsWith('restore')), false)
})

test('conta explícita ausente continua distinta, inclusive no automático sem vínculo', () => {
  const { fake } = fixture()
  const row = fake.data.functions.find((row) => row.definition.id === 'memory_consolidation')!
  const accountId = `ac_${'f'.repeat(64)}`
  assert.equal(fake.data.accounts.some((account) => account.id === accountId), false)
  for (const selection of [
    { mode: 'automatic' as const, accountId, binding: null },
    { mode: 'custom' as const, accountId, modelId: 'haiku', modelOptions: {} },
  ]) {
    row.selection = selection
    assert.equal(reasonText('account_missing', row), 'fundo.motivo.account_missing')
    assert.deepEqual(row.selection, selection)
  }
})

test('contas do seletor (Bot de fundo e Memória): nome de Contas de IA, nunca dois textos iguais, conectadas com assinatura primeiro', () => {
  const words = { legacy: 'Perfil antigo do Hub', subscription: 'Assinatura confirmada', refused: 'Ainda não funciona', state: (s: string) => s === 'connected' ? 'Conectada' : 'Desconectada' }
  const base = { backend: 'claude', models: [] }
  // O caso do dono: dois perfis antigos do mesmo motor e login, um desconectado e um conectado com assinatura.
  const accounts = [
    { ...base, id: 'old-off', label: 'Claude — login deste computador', origin: 'legacy', state: 'disconnected', billing: 'subscription' },
    { ...base, id: 'empresa', label: 'Empresa', origin: 'managed', state: 'disconnected', billing: 'unverified' },
    { ...base, id: 'codex', label: 'Codex — login deste computador', origin: 'legacy', backend: 'codex', state: 'connected', billing: 'unverified' },
    { ...base, id: 'old-on', label: 'Claude — login deste computador', origin: 'legacy', state: 'connected', billing: 'subscription' },
  ]
  // Função que aceita Codex (Rascunho de bot usa Codex por padrão): a ordem só depende de conexão e assinatura.
  const options = accountOptions(accounts, '', words, 'bot_draft')
  assert.deepEqual(options.map((o) => o.id), ['old-on', 'codex', 'old-off', 'empresa'], 'assinatura confirmada, outras conectadas, não conectadas; estável em cada grupo')
  assert.deepEqual(options.map((o) => o.label), [
    'Perfil antigo do Hub · Claude Code #ldon · Conectada · Assinatura confirmada',
    'Perfil antigo do Hub · Codex · Conectada',
    'Perfil antigo do Hub · Claude Code #doff · Desconectada',
    'Empresa · Claude Code · Desconectada',
  ])
  assert.deepEqual(options.map((o) => o.disabled), [false, false, true, true], 'não conectada fica marcada e não é escolha nova')
  // A escolha salva desconectada continua visível e escolhível.
  assert.equal(accountOptions(accounts, 'old-off', words, 'bot_draft').find((o) => o.id === 'old-off')?.disabled, false)
  // Mesmo nome e estado: o número separa; nenhum texto se repete.
  const twins = accountOptions([accounts[3]!, { ...accounts[3]!, id: 'old-on-2' }], '', words, 'bot_draft')
  assert.deepEqual(twins.map((o) => o.label), ['Perfil antigo do Hub · Claude Code #ldon · Conectada · Assinatura confirmada', 'Perfil antigo do Hub · Claude Code #don2 · Conectada · Assinatura confirmada'])
  assert.equal(new Set(accountOptions([...accounts, { ...accounts[0]!, id: 'x' }, { ...accounts[1]!, id: 'y' }], '', words, 'bot_draft').map((o) => o.label)).size, 6)
  // A escolha salva vem da seleção do Hub (personalizada, automática ou a alternativa Haiku).
  assert.equal(savedAccountId({ mode: 'custom', accountId: 'old-off', modelId: 'haiku', modelOptions: {} }), 'old-off')
  assert.equal(savedAccountId({ mode: 'automatic', accountId: null, binding: { accountId: 'old-on', modelId: 'haiku', modelOptions: {} }, alternativeAccountId: 'empresa' }), 'old-on')
  assert.equal(savedAccountId({ mode: 'automatic', accountId: null, binding: null, alternativeAccountId: 'empresa' }, true), 'empresa')
  assert.equal(savedAccountId(null), '')
})

test('Codex na memória e nos títulos: espelho de background_selection.rs (marcada, desabilitada, antes das desconectadas)', () => {
  const words = { legacy: 'Perfil antigo do Hub', subscription: 'Assinatura confirmada', refused: 'Ainda não funciona para a memória', state: (s: string) => s === 'connected' ? 'Conectada' : 'Desconectada' }
  const base = { models: [], origin: 'legacy' }
  const claudeOn = { ...base, id: 'claude-on', label: 'Claude — login deste computador', backend: 'claude', state: 'connected', billing: 'subscription' }
  const claudeOff = { ...base, id: 'claude-off', label: 'Claude — login deste computador', backend: 'claude', state: 'disconnected', billing: 'subscription' }
  const codex = { ...base, id: 'codex', label: 'Codex — login deste computador', backend: 'codex', state: 'connected', billing: 'subscription' }
  for (const fn of ['memory_consolidation', 'bot_naming']) {
    assert.equal(codexRefused(fn, codex), true, fn)
    const options = accountOptions([claudeOff, codex, claudeOn], '', words, fn)
    assert.deepEqual(options, [
      { id: 'claude-on', label: 'Perfil antigo do Hub · Claude Code #deon · Conectada · Assinatura confirmada', disabled: false },
      { id: 'codex', label: 'Perfil antigo do Hub · Codex · Conectada · Ainda não funciona para a memória', disabled: true },
      { id: 'claude-off', label: 'Perfil antigo do Hub · Claude Code #eoff · Desconectada', disabled: true },
    ], 'Codex conectada fica marcada, desabilitada e antes das desconectadas; Claude igual')
    // O caso do dono: Codex já salva continua visível, marcada e escolhível como está.
    const saved = accountOptions([claudeOff, codex, claudeOn], 'codex', words, fn).find((o) => o.id === 'codex')
    assert.deepEqual(saved, { id: 'codex', label: 'Perfil antigo do Hub · Codex · Conectada · Ainda não funciona para a memória', disabled: false })
  }
  // Funções que não passam pelo trabalhador de fundo continuam aceitando Codex.
  for (const fn of ['vision', 'context_compression', 'bot_draft']) {
    assert.equal(codexRefused(fn, codex), false, fn)
    assert.equal(accountOptions([codex], '', words, fn)[0]?.disabled, false, fn)
  }
  assert.equal(codexRefused('memory_consolidation', claudeOn), false)
})

test('R1-02: Codex desconectado vai para o último grupo; o grupo recusado é só de Codex conectado', () => {
  const words = { legacy: 'Perfil antigo do Hub', subscription: 'Assinatura confirmada', refused: 'Ainda não funciona para a memória', state: (s: string) => s === 'connected' ? 'Conectada' : 'Desconectada' }
  const base = { models: [], origin: 'managed', backend: 'codex', billing: 'subscription' }
  const off = { ...base, id: 'codex-off', label: 'Codex A', state: 'disconnected' }
  const on = { ...base, id: 'codex-on', label: 'Codex B', state: 'connected' }
  const claudeOff = { ...base, id: 'claude-off', label: 'Claude', backend: 'claude', state: 'disconnected' }
  // Chegam na ordem inversa: o desconectado primeiro.
  const options = accountOptions([off, claudeOff, on], '', words, 'memory_consolidation')
  assert.deepEqual(options.map((o) => o.id), ['codex-on', 'codex-off', 'claude-off'])
  assert.deepEqual(options.map((o) => o.label), ['Codex B · Codex · Conectada · Ainda não funciona para a memória', 'Codex A · Codex · Desconectada', 'Claude · Claude Code · Desconectada'])
  assert.deepEqual(options.map((o) => o.disabled), [true, true, true])
})

test('Entrega 4: grupo da função pelo `consumer` do Hub; sem o campo ou com valor desconhecido, o grupo neutro', () => {
  const def = (id: string, managedInVoice = false) => ({ id, name: id, managedInVoice, recipe: '' })
  assert.equal(grupoDe({ definition: def('memory_consolidation'), consumer: 'background_worker' }), 'emUso')
  assert.equal(grupoDe({ definition: def('vision'), consumer: 'awaiting_implementation' }), 'emPreparo')
  assert.equal(grupoDe({ definition: def('vision') }), 'funcoes')
  assert.equal(grupoDe({ definition: def('memory_consolidation'), consumer: 'outro_valor' }), 'funcoes')
  assert.equal(grupoDe({ definition: def('voice_live', true), consumer: 'awaiting_implementation' }), 'voz')
})

test('revisão R1: leitura que falhou (sem dados) volta pelo Recarregar, sem resto do erro', async () => {
  const { model, host } = fixture()
  const original = host.call.bind(host)
  let falhar = true
  host.call = async (method, body) => { if (falhar && method === 'functionModels') throw new Error('timeout'); return original(method, body) }
  await model.load()
  assert.equal(model.getSnapshot().data, null)
  assert.equal(model.getSnapshot().error, 'leitura')
  falhar = false
  await model.load(true)
  assert.ok(model.getSnapshot().data)
  assert.equal(model.getSnapshot().error, null)
  assert.equal(model.getSnapshot().loading, false)
})

test('Hub sem o método (Hub antigo): vira "semRecurso", não falha de leitura; Recarregar volta quando o Hub atualiza', async () => {
  const { model, host } = fixture()
  const original = host.call.bind(host)
  let antigo = true
  host.call = async (method, body) => { if (antigo && method === 'functionModels') throw new Error('unknown method: functionModels'); return original(method, body) }
  await model.load()
  assert.equal(model.getSnapshot().data, null)
  assert.equal(model.getSnapshot().error, 'semRecurso')
  antigo = false
  await model.load(true)
  assert.ok(model.getSnapshot().data)
  assert.equal(model.getSnapshot().error, null)
})

test('conta desconectada: motivo simples, sugestão da primeira pronta e código do Hub (MEX-1355)', () => {
  const words = { legacy: 'Perfil antigo do Hub', subscription: 'Assinatura confirmada', refused: 'x', state: (s: string) => s === 'connected' ? 'Conectada' : 'Desconectada' }
  const base = { models: [{ id: 'haiku', options: [] }], origin: 'legacy', backend: 'claude', billing: 'subscription' }
  const off = { ...base, id: 'off-aaaa', label: 'Claude — login deste computador', state: 'disconnected' }
  const on = { ...base, id: 'on-bbbb', label: 'Claude — login deste computador', state: 'connected' }
  const row = { selection: { mode: 'automatic' as const, accountId: 'off-aaaa', binding: null } }
  // O Hub diz "modelo padrão não informado" para conta desconectada: a tela diz desconectada.
  assert.equal(shownReason('default_model_unknown', row, [off, on]), 'account_disconnected')
  // Conta conectada que realmente não informou modelo: o motivo do Hub segue.
  assert.equal(shownReason('default_model_unknown', { selection: { mode: 'automatic', accountId: 'on-bbbb', binding: null } }, [off, on]), 'default_model_unknown')
  assert.equal(shownReason('account_missing', row, [off, on]), 'account_missing')
  assert.equal(suggestedAccount([off, on], 'memory_consolidation')?.id, 'on-bbbb')
  assert.equal(suggestedAccount([off], 'memory_consolidation'), null)
  assert.equal(suggestedAccount([{ ...base, id: 'cx', label: 'c', backend: 'codex', state: 'connected' }], 'memory_consolidation'), null, 'Codex é recusado na memória')
  // Mesmo nome, uma conectada e outra não: o identificador curto separa.
  const labels = accountOptions([off, on], '', words, 'memory_consolidation').map((o) => o.label)
  assert.deepEqual(labels, ['Perfil antigo do Hub · Claude Code #bbbb · Conectada · Assinatura confirmada', 'Perfil antigo do Hub · Claude Code #aaaa · Desconectada'])
  assert.equal(reasonCodeText('account_disconnected'), 'fundo.atividade.motivo.account_disconnected')
  assert.equal(reasonCodeText('outro'), null)
  assert.equal(taskKey('memory_consolidation'), 'fundo.tarefa.memory_consolidation')
  assert.equal(taskKey('nova'), 'fundo.tarefa.generica')
})

test('conta desconectada: a resolução do Hub e o estado cadastral "unavailable" também valem (P2)', () => {
  const base = { models: [{ id: 'haiku', options: [] }], origin: 'legacy', backend: 'claude', billing: 'subscription', label: 'Claude' }
  const indisp = { ...base, id: 'in-aaaa', state: 'unavailable' }
  const ok = { ...base, id: 'ok-bbbb', state: 'connected' }
  const nega = { status: 'unavailable' as const, unavailable: { reason: 'account_disconnected' } }
  const row = (accountId: string, resolution: never) => ({ selection: { mode: 'automatic' as const, accountId, binding: null }, resolution })
  // Estado cadastral unavailable resolve para account_disconnected no Hub.
  assert.equal(accountDisconnected([indisp, ok], 'in-aaaa', row('in-aaaa', nega as never)), true)
  assert.equal(shownReason('default_model_unknown', row('in-aaaa', nega as never), [indisp, ok]), 'account_disconnected')
  // Resolução autoritativa do Hub vale para a conta vinculada, mesmo com o estado "connected" na lista.
  assert.equal(accountDisconnected([ok], 'ok-bbbb', row('ok-bbbb', nega as never)), true)
  // Não vale para outra conta do rascunho, nem para o principal conectado quando o motivo é outro.
  assert.equal(accountDisconnected([indisp, ok], 'ok-bbbb', row('in-aaaa', nega as never)), false)
  assert.equal(accountDisconnected([ok], 'ok-bbbb', row('ok-bbbb', { status: 'unavailable', unavailable: { reason: 'default_model_unknown' } } as never)), false)
  assert.equal(accountDisconnected([ok], '', row('', nega as never)), false)
})

test('sugestão "Usar …": só conta que o Hub aceita (com modelo; sem API paga em memória/nomes) (P3)', () => {
  const base = { origin: 'legacy', backend: 'claude', label: 'Claude', state: 'connected', billing: 'subscription' }
  const semModelo = { ...base, id: 'sm', models: [] }
  const comModelo = { ...base, id: 'cm', models: [{ id: 'haiku', options: [] }] }
  const api = { ...base, id: 'api', billing: 'paid_api', models: [{ id: 'haiku', options: [] }] }
  assert.equal(suggestedAccount([semModelo, comModelo], 'memory_consolidation')?.id, 'cm')
  assert.equal(suggestedAccount([semModelo], 'memory_consolidation'), null)
  assert.equal(suggestedAccount([api], 'bot_naming'), null, 'API paga é recusada em memória e nomes')
  assert.equal(suggestedAccount([api, comModelo], 'memory_consolidation')?.id, 'cm')
  assert.equal(suggestedAccount([api], 'bot_draft'), null, 'o Hub recusa API paga em todas as funções (`ApiNotEnabled`)')
  assert.equal(suggestedAccount([{ ...api, id: 'u', billing: 'unknown' }], 'vision'), null, 'cobrança desconhecida também (`BillingUnknown`)')
  const soSonnet = { ...base, id: 'ss', models: [{ id: 'sonnet', options: [] }] }
  assert.equal(suggestedAccount([soSonnet, comModelo], 'memory_consolidation', true)?.id, 'cm', 'no Automático, só conta com receita resolvível')
  assert.equal(suggestedAccount([soSonnet], 'memory_consolidation', true), null)
  assert.equal(suggestedAccount([soSonnet], 'memory_consolidation')?.id, 'ss', 'com modelo escolhido à mão, qualquer conta que serve')
  // O rótulo do botão vem do conjunto completo: duplicatas mantêm o identificador curto.
  const words = { legacy: 'Perfil', subscription: 'Assinatura', refused: 'x', state: () => 'Conectada' }
  const dup = { ...comModelo, id: 'ac_zzzz' }
  const rotulo = accountOptions([comModelo, dup], '', words, 'bot_draft').find((o) => o.id === 'cm')!.label
  assert.match(rotulo, /#/)
  assert.notEqual(rotulo, accountOptions([comModelo], '', words, 'bot_draft')[0]!.label)
})

test('Neste Space: só a seleção do Space ativo aparece, as dos outros ficam fora', () => {
  const a = { spaceId: 'ativo', spaceName: 'Ativo', spaceRevision: 1, functionId: 'memory_consolidation', revision: 0, state: 'configured', current: null, audience: 'owner_private', execution: 'awaiting_executor' } as const
  const lista = [a, { ...a, spaceId: 'outro-space', spaceName: 'Outro' }]
  assert.equal(selecaoDoSpace(lista, 'outro-space')?.spaceName, 'Outro')
  assert.equal(selecaoDoSpace(lista, a.spaceId), a)
  assert.equal(selecaoDoSpace(lista, 'sem-selecao'), null)
  assert.equal(selecaoDoSpace(lista, null), null)
  assert.equal(lista.length, 2)
})

test('Neste Space: a origem é o Space ativo no computador/Hub dele; troca de Space ou de computador nunca cai no Pessoal de outro', () => {
  const ativo = { computerId: 'A', hostId: 'hub-A', spaceId: 'space-A' }
  assert.equal(computadorDaPagina(ativo, 'B'), 'A', 'o seletor não redireciona o cartão')
  assert.equal(computadorDaPagina(null, 'B'), 'B')
  assert.equal(spaceDaPagina(ativo, 'A', 'hub-A', 'pessoal-A'), 'space-A')
  assert.equal(spaceDaPagina(ativo, 'B', 'hub-B', 'pessoal-B'), null, 'computador B com ativo de A: sem Pessoal de B')
  assert.equal(spaceDaPagina(ativo, 'A', 'hub-X', 'pessoal-A'), null, 'Hub divergente: aguarda')
  assert.equal(spaceDaPagina(ativo, 'A', undefined, 'pessoal-A'), null, 'Hub sem identidade: aguarda')
  assert.equal(spaceDaPagina(null, 'B', 'hub-B', 'pessoal-B'), 'pessoal-B', 'Pessoal aberto')
  assert.equal(spaceDaPagina(null, 'B', 'hub-B', undefined), null)
  // Troca do topo de A para B: o cartão passa a B e a gravação usaria a linha de B, nunca a de A/Pessoal.
  const linhas = [{ spaceId: 'space-A' }, { spaceId: 'pessoal-B' }, { spaceId: 'space-B' }] as never
  const ativoB = { computerId: 'B', hostId: 'hub-B', spaceId: 'space-B' }
  const c = computadorDaPagina(ativoB, 'A')
  assert.equal(selecaoDoSpace(linhas, spaceDaPagina(ativoB, c, 'hub-B', 'pessoal-B'))?.spaceId, 'space-B')
  assert.equal(selecaoDoSpace(linhas, spaceDaPagina(ativoB, 'A', 'hub-A', 'pessoal-A')), null, 'resposta tardia de A não casa com o ativo B')
})

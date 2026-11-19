import assert from 'node:assert/strict'
import { test } from 'node:test'
import { BackgroundStore } from '../bot-de-fundo/model.ts'
import { LearningStore, modelFamily, modelNotices, modelPatch, readLearning } from './model.ts'

test('D389 lê só os três modos e um contador inteiro', () => {
  assert.deepEqual(readLearning({ mode: 'suggest', pending: 2 }), { mode: 'suggest', pending: 2 })
  for (const bad of [null, { mode: 'never', pending: 0 }, { mode: 'auto', pending: -1 }, { mode: 'off' }]) assert.throws(() => readLearning(bad))
})

test('D389 salvar o modo grava no Hub; falha relê o estado em vez de presumir a escolha', async () => {
  const calls: [string, unknown][] = []
  let saved = 'auto', fail = false
  const store = new LearningStore(async () => ({
    call: async <T,>(method: string, body?: unknown) => {
      calls.push([method, body])
      if (method === 'saveMemoryLearning') { if (fail) throw new Error('timeout'); saved = (body as { mode: string }).mode }
      return { mode: saved, pending: 1 } as T
    },
  }))
  await store.load()
  assert.equal(store.value?.mode, 'auto')
  await store.save('suggest')
  assert.deepEqual(calls[1], ['saveMemoryLearning', { mode: 'suggest' }])
  assert.equal(store.value?.mode, 'suggest')
  fail = true
  await store.save('off')
  assert.equal(store.issue, 'save')
  assert.equal(store.value?.mode, 'suggest', 'o estado mostrado é o relido do Hub')
})

test('D389 o cartão grava a mesma seleção memory_consolidation do Bot de fundo', async () => {
  const account = { id: 'claude-1', label: 'Claude', backend: 'claude', state: 'connected', billing: 'subscription', models: [{ id: 'claude-haiku-5-5', options: [] }, { id: 'claude-opus-5-5', options: [] }] }
  const doc = { schemaVersion: 1, revision: 4, functions: { memory_consolidation: { mode: 'automatic', accountId: 'claude-1', binding: null } } }
  const writes: unknown[] = []
  const store = new BackgroundStore(async () => ({
    call: async <T,>(method: string, body?: unknown) => {
      if (method === 'functionModels') return { document: doc, accounts: [account], accountsIssue: null, execution: 'partial',
        functions: [{ definition: { id: 'memory_consolidation', name: 'Organizar memória', managedInVoice: false, recipe: '' }, selection: doc.functions.memory_consolidation, resolution: { status: 'unavailable', unavailable: { reason: 'quota_unknown' } } }] } as T
      if (method === 'saveFunctionModel') { writes.push(body); return { document: { ...doc, revision: 5 } } as T }
      throw new Error('unavailable')
    },
  }))
  await store.load()
  for (const patch of modelPatch('claude-opus-5-5')) store.change(patch)
  await store.save()
  assert.deepEqual(writes, [{ expectedRevision: 4, functionId: 'memory_consolidation', selection: { mode: 'custom', accountId: 'claude-1', modelId: 'claude-opus-5-5', modelOptions: {} } }])
  assert.deepEqual(modelPatch(''), [{ mode: 'automatic' }])
})

test('D389 família do modelo', () => {
  assert.equal(modelFamily('claude-haiku-5-5'), 'haiku')
  assert.equal(modelFamily('Opus'), 'opus')
  assert.equal(modelFamily('gpt-6.1-sol'), null)
})

test('D439: salvo com escolha disponível promete uso; indisponível de verdade mostra o motivo do Hub, sem "vale a partir"', () => {
  const saved = { dirty: false, conflict: false, loading: false, aviso: 'salvo' as const }
  const available = { status: 'available' as const, resolved: { effective: { accountId: 'a', modelId: 'haiku', modelOptions: {} } } }
  const unavailable = (reason: string) => ({ status: 'unavailable' as const, unavailable: { reason } })
  assert.deepEqual(modelNotices(available, saved), { saved: 'memoria.modelo.salvo', warning: null })
  assert.deepEqual(modelNotices(unavailable('codex_subscription_unavailable'), saved), { saved: 'memoria.modelo.salvoSemUso', warning: 'memoria.modelo.indisponivel.codex' })
  assert.deepEqual(modelNotices(unavailable('api_coming_soon'), saved), { saved: 'memoria.modelo.salvoSemUso', warning: 'memoria.modelo.indisponivel.api' })
  // Sem salvar agora: só o aviso; motivo sem texto próprio (inclusive nome do protótipo) usa o genérico.
  const idle = { dirty: false, conflict: false, loading: false, aviso: null }
  assert.deepEqual(modelNotices(unavailable('account_missing'), idle), { saved: null, warning: 'memoria.modelo.indisponivel' })
  assert.deepEqual(modelNotices(unavailable('toString'), idle), { saved: null, warning: 'memoria.modelo.indisponivel' })
  assert.deepEqual(modelNotices(available, idle), { saved: null, warning: null })
  // Rascunho aberto ou conflito: nada de "salvo".
  assert.deepEqual(modelNotices(unavailable('codex_subscription_unavailable'), { ...saved, dirty: true }), { saved: null, warning: null })
  assert.equal(modelNotices(available, { ...saved, conflict: true }).saved, null)
})

test('R1-01: com a releitura pendente depois de salvar, nenhum aviso usa a resolução da escolha anterior (nos dois sentidos)', async () => {
  const available = { status: 'available', resolved: { effective: { accountId: 'claude-1', modelId: 'claude-haiku-5-5', modelOptions: {} } } }
  const codex = { status: 'unavailable', unavailable: { reason: 'codex_subscription_unavailable' } }
  const accounts = [
    { id: 'claude-1', label: 'Claude', backend: 'claude', state: 'connected', billing: 'subscription', models: [{ id: 'claude-haiku-5-5', options: [] }] },
    { id: 'codex-1', label: 'Codex', backend: 'codex', state: 'connected', billing: 'subscription', models: [{ id: 'gpt-6-luna', options: [] }] },
  ]
  for (const [before, after, account, modelId, expected] of [
    [available, codex, 'codex-1', 'gpt-6-luna', { saved: 'memoria.modelo.salvoSemUso', warning: 'memoria.modelo.indisponivel.codex' }],
    [codex, available, 'claude-1', 'claude-haiku-5-5', { saved: 'memoria.modelo.salvo', warning: null }],
  ] as const) {
    let revision = 4, resolution: unknown = before
    let release!: () => void
    let reading!: () => void
    const reread = new Promise<void>((resolve) => { reading = resolve })
    const store = new BackgroundStore(async () => ({
      call: async <T,>(method: string) => {
        if (method === 'functionModels') {
          if (revision === 5) { reading(); await new Promise<void>((resolve) => { release = resolve }) }
          const doc = { schemaVersion: 1, revision, functions: { memory_consolidation: { mode: 'automatic', accountId: null, binding: null } } }
          return { document: doc, accounts, accountsIssue: null, execution: 'partial',
            functions: [{ definition: { id: 'memory_consolidation', name: 'Memória', managedInVoice: false, recipe: '' }, selection: doc.functions.memory_consolidation, resolution }] } as T
        }
        if (method === 'saveFunctionModel') { revision = 5; resolution = after; return { document: { schemaVersion: 1, revision: 5, functions: { memory_consolidation: { mode: 'custom', accountId: account, modelId, modelOptions: {} } } } } as T }
        throw new Error('unavailable')
      },
    }))
    await store.load()
    store.change({ accountId: account })
    for (const patch of modelPatch(modelId)) store.change(patch)
    const saving = store.save()
    await reread
    const row = () => store.getSnapshot().data!.functions[0]!
    assert.deepEqual(row().resolution, before, 'a releitura ainda não voltou')
    assert.deepEqual(modelNotices(row().resolution, store.getSnapshot()), { saved: null, warning: null }, 'sem frase enganosa enquanto relê')
    release()
    await saving
    assert.deepEqual(modelNotices(row().resolution, store.getSnapshot()), expected, 'depois da releitura, o aviso da escolha salva')
  }
})

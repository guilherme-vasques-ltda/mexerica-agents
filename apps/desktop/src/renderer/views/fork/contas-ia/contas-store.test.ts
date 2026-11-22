import assert from 'node:assert/strict'
import test from 'node:test'
import type { HostClient } from '../../../client/host-client'
import type { CreateAccount, ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import { ContasStore, USO_VALIDADE_MS, type ContasApi } from './contas-store.ts'
import { errorMessage } from './regras.ts'

/** DON-917: o erro sai cru do store (código do Hub); a tela o traduz. */
const falhaEm = (idioma: 'pt-BR' | 'en') => (e: unknown) => { throw new Error(errorMessage(e, idioma)) }

const client = () => Promise.resolve({} as HostClient)
const id = (c: string) => `ac_${c.repeat(64)}`
const account = (patch: Partial<ManagedAccount> = {}): ManagedAccount => ({
  id: id('a'), label: 'Pessoal', brand: 'OpenAI', backend: 'codex', billing: 'unverified', state: 'disconnected',
  models: [], revision: 1, origin: 'managed', botIds: [], ...patch,
})

/** Hub falso com lista em memória; registra cada pedido. Nenhuma conta real. */
function fakeHub(initial: ManagedAccount[]) {
  let accounts = initial
  const calls: { method: string; body: unknown }[] = []
  let failNext: Error | null = null
  const run = async <T>(method: string, body: unknown, result: () => T): Promise<T> => {
    calls.push({ method, body })
    if (failNext) { const e = failNext; failNext = null; throw e }
    return result()
  }
  const api: ContasApi = {
    accounts: (_c, query) => run('accounts', query, () => ({ accounts })),
    create: (_c, request: CreateAccount) => run('create', request, () => {
      const created = account({ id: id('n'), label: request.label, backend: request.backend })
      accounts = [...accounts, created]
      return created
    }),
    rename: (_c, request) => run('rename', request, () => {
      const renamed = { ...accounts.find((a) => a.id === request.engineAccountId)!, label: request.label, revision: request.expectedRevision + 1 }
      accounts = accounts.map((a) => (a.id === renamed.id ? renamed : a))
      return renamed
    }),
    usage: (_c, engineAccountId) => run('usage', engineAccountId, () => ({ engineAccountId, state: 'unavailable' as const, source: null, updatedAt: null, windows: [] })),
    disconnect: (_c, request) => run('disconnect', request, () => ({ ...accounts.find((a) => a.id === request.engineAccountId)!, state: 'disconnected' as const })),
    setEnabled: (_c, request) => run('setEnabled', request, () => {
      const updated = { ...accounts.find((a) => a.id === request.engineAccountId)!, enabled: request.enabled, revision: request.expectedRevision + 1 }
      accounts = accounts.map((a) => (a.id === updated.id ? updated : a))
      return updated
    }),
    remove: (_c, request) => run('remove', request, () => {
      accounts = accounts.filter((a) => a.id !== request.engineAccountId)
      return {}
    }),
    setApiEnv: (_c, request) => run('setApiEnv', request, () => ({})),
    modelVisibility: (_c, request) => run('modelVisibility', request, () => {
      // Como o Hub: idempotente e sem mexer na revisão da conta.
      const before = accounts.find((a) => a.id === request.engineAccountId)!
      const hidden = new Set(before.hiddenModelIds ?? [])
      if (request.hidden) hidden.add(request.modelId)
      else hidden.delete(request.modelId)
      const changed = { ...before, hiddenModelIds: [...hidden] }
      accounts = accounts.map((a) => (a.id === changed.id ? changed : a))
      return changed
    }),
  }
  return { api, calls, fail: (e: Error) => { failNext = e }, set: (next: ManagedAccount[]) => { accounts = next } }
}

test('lista gerenciadas antes do perfil antigo e força a sondagem só quando pedido', async () => {
  const hub = fakeHub([account({ id: id('l'), origin: 'legacy', label: 'Codex — perfil do Hub' }), account({ id: id('z'), label: 'Trabalho' }), account({ label: 'Pessoal' })])
  const contas = new ContasStore(client, hub.api)
  await contas.load()
  assert.deepEqual(contas.accounts?.map((a) => a.label), ['Pessoal', 'Trabalho', 'Codex — perfil do Hub'])
  await contas.load(true)
  assert.deepEqual(hub.calls.map((c) => c.body), [{}, { refresh: true }])
})

test('criar guarda o tipo só em memória; um pedido por vez; chave vai só para a conta, sem guardar valores', async () => {
  const hub = fakeHub([])
  const contas = new ContasStore(client, hub.api)
  const created = await contas.create({ backend: 'claude', label: 'Chave', kind: 'api', clientRequestId: 'ui-1' })
  assert.equal(contas.kindOf(created.id), 'api')
  assert.equal(contas.accounts?.length, 1)
  const first = contas.saveKey(created, { ANTHROPIC_API_KEY: 'FICTICIA' })
  await assert.rejects(contas.rename(created, 'Outro').catch(falhaEm('pt-BR')), /Espere/)
  await first
  const sent = hub.calls.find((c) => c.method === 'setApiEnv')!.body as Record<string, unknown>
  assert.deepEqual(sent, { engineAccountId: created.id, expectedRevision: 1, backend: 'claude', vars: { ANTHROPIC_API_KEY: 'FICTICIA' } })
  assert.ok(!JSON.stringify(contas).includes('FICTICIA'), 'a chave não fica na lista')
})

test('conflito de revisão relê a lista e explica; remover tira da lista e esquece o login lembrado', async () => {
  const original = account()
  const hub = fakeHub([original])
  const contas = new ContasStore(client, hub.api)
  await contas.load()
  hub.set([{ ...original, label: 'Renomeada em outra janela', revision: 2 }])
  hub.fail(new Error('account_conflict: releia a conta antes de continuar.'))
  await assert.rejects(contas.rename(original, 'Nova').catch(falhaEm('pt-BR')), /mudou em outra janela/)
  assert.equal(contas.account(original.id)?.label, 'Renomeada em outra janela')
  contas.rememberOperation(original.id, { operationId: 'op', generation: 1, step: 'login' })
  await contas.remove(contas.account(original.id)!)
  assert.equal(contas.accounts?.length, 0)
  assert.equal(contas.operationFor(original.id), null)
  const removed = hub.calls.find((c) => c.method === 'remove')!.body
  assert.deepEqual(removed, { engineAccountId: original.id, expectedRevision: 2 })
})

test('DON-917: a falha de leitura fica crua no store (estado é dado), e a tela a traduz no idioma de agora', async () => {
  const api = { accounts: async () => { throw new Error('accounts_unavailable: indisponível.') } } as unknown as ContasApi
  const contas = new ContasStore(async () => ({}) as HostClient, api)
  await contas.load(true).catch(() => {})
  assert.ok(contas.error instanceof Error)
  assert.match(errorMessage(contas.error, 'pt-BR'), /Não foi possível consultar as contas/)
  assert.match(errorMessage(contas.error, 'en'), /Couldn’t check the accounts/)
})

test('MX-6: ocultar manda um modelo por gesto, com o estado novo e a revisão; repetir não muda nada', async () => {
  const hub = fakeHub([account({ hiddenModelIds: ['m1'] })])
  const contas = new ContasStore(client, hub.api)
  await contas.load()
  assert.equal(contas.canHideModels(contas.accounts![0]!), true)
  await contas.setModelHidden(contas.accounts![0]!, 'm2', true)
  await contas.setModelHidden(contas.accounts![0]!, 'm2', true)
  await contas.setModelHidden(contas.accounts![0]!, 'm1', false)
  assert.deepEqual(hub.calls.filter((c) => c.method === 'modelVisibility').map((c) => c.body), [
    { engineAccountId: id('a'), expectedRevision: 1, modelId: 'm2', hidden: true },
    { engineAccountId: id('a'), expectedRevision: 1, modelId: 'm2', hidden: true },
    { engineAccountId: id('a'), expectedRevision: 1, modelId: 'm1', hidden: false },
  ])
  assert.deepEqual(contas.accounts![0]!.hiddenModelIds, ['m2'])
  // Conta que sumiu no Hub: relê a lista e devolve o erro cru para a tela traduzir.
  hub.set([])
  hub.fail(new Error('account_not_found: conta inexistente.'))
  await assert.rejects(contas.setModelHidden(contas.accounts![0]!, 'm3', true), /account_not_found/)
  assert.equal(contas.accounts?.length, 0)
})

test('Entrega 3: Hub sem `hiddenModelIds` na resposta não oculta: recusa sem pedido e nada fica guardado no aparelho', async () => {
  const hub = fakeHub([account()])
  const contas = new ContasStore(client, hub.api)
  await contas.load()
  const conta = contas.accounts![0]!
  assert.equal(contas.canHideModels(conta), false)
  await assert.rejects(contas.setModelHidden(conta, 'm1', true), /operation_forbidden/)
  assert.deepEqual(hub.calls.map((c) => c.method), ['accounts'])
  assert.equal(contas.accounts![0]!.hiddenModelIds, undefined)
  assert.equal(contas.busy, null)
  // Lista vazia é suporte: o Hub mandou o campo.
  assert.equal(contas.canHideModels(account({ hiddenModelIds: [] })), true)
})

test('MX-6: conflito de revisão ao ocultar relê as contas e tenta de novo uma vez; conta antiga nunca oculta', async () => {
  const original = account({ hiddenModelIds: [] })
  const hub = fakeHub([original])
  const contas = new ContasStore(client, hub.api)
  await contas.load()
  hub.set([{ ...original, label: 'Renomeada em outra janela', revision: 2 }])
  hub.fail(new Error('account_conflict: releia a conta antes de continuar.'))
  await contas.setModelHidden(original, 'fora-do-catalogo', true)
  assert.deepEqual(hub.calls.slice(1).map((c) => [c.method, (c.body as { expectedRevision?: number }).expectedRevision]), [['modelVisibility', 1], ['accounts', undefined], ['modelVisibility', 2]])
  assert.deepEqual(contas.accounts![0]!.hiddenModelIds, ['fora-do-catalogo'])
  assert.equal(contas.accounts![0]!.label, 'Renomeada em outra janela')
  // Outro erro não repete o pedido.
  hub.fail(new Error('invalid_field: modelId inválido.'))
  await assert.rejects(contas.setModelHidden(contas.accounts![0]!, 'x', true), /invalid_field/)
  assert.equal(hub.calls.filter((c) => c.method === 'modelVisibility').length, 3)
  // Conta antiga, mesmo com o campo: sem Ocultar e sem pedido (o Hub devolveria legacy_preserved).
  const antiga = account({ origin: 'legacy', hiddenModelIds: [] })
  assert.equal(contas.canHideModels(antiga), false)
  await assert.rejects(contas.setModelHidden(antiga, 'm1', true), /operation_forbidden/)
  assert.equal(hub.calls.filter((c) => c.method === 'modelVisibility').length, 3)
})

test('Testar força a sondagem no Hub, devolve a conta como ela ficou e nunca chama mutação de conta', async () => {
  const hub = fakeHub([account({ state: 'connected', billing: 'subscription' })])
  const contas = new ContasStore(client, hub.api)
  await contas.load()
  hub.set([account({ state: 'unavailable', billing: 'subscription' })])
  const fresh = await contas.verify(contas.account(id('a'))!)
  assert.equal(fresh.state, 'unavailable')
  assert.equal(contas.account(id('a'))?.state, 'unavailable', 'a lista já mostra o resultado')
  assert.deepEqual(hub.calls.map((c) => [c.method, c.body]), [['accounts', {}], ['accounts', { refresh: true }]])
  assert.equal(contas.busy, null)
  // Conta que sumiu no Hub: erro com código, para a linha traduzir.
  hub.set([])
  await assert.rejects(contas.verify(fresh), (e: unknown) => errorMessage(e, 'pt-BR').length > 0 && /account_not_found/.test(String(e)))
  // Falha do Hub aparece; nada fica preso.
  hub.set([account()])
  hub.fail(new Error('offline'))
  await assert.rejects(contas.verify(account()))
  assert.equal(contas.busy, null)
})

test('Testar durante uma leitura em andamento não deixa a lista presa em carregando e a leitura mais nova vence', async () => {
  const hub = fakeHub([account({ state: 'connected', billing: 'subscription' })])
  const contas = new ContasStore(client, hub.api)
  const lendo = contas.load()
  const testando = contas.verify(account({ state: 'connected' }))
  await Promise.all([lendo, testando])
  assert.equal(contas.loading, false)
  assert.equal(contas.busy, null)
})

test('MEX-1294/F6-A: o uso é lido uma vez por conta e Space; vale 60 s; falha não fica guardada', async () => {
  const hub = fakeHub([account({ state: 'connected' })])
  const store = new ContasStore(client, hub.api)
  const doUso = () => hub.calls.filter((c) => c.method === 'usage').length
  // Dez linhas pedindo juntas dividem a mesma leitura.
  const juntas = await Promise.all(Array.from({ length: 10 }, () => store.usage(id('a'), undefined, 1_000)))
  assert.equal(doUso(), 1)
  assert.ok(juntas.every((u) => u === juntas[0]))
  // Trocar de conversa dentro da validade não pede de novo; passou a validade, pede.
  await store.usage(id('a'), undefined, 1_000 + USO_VALIDADE_MS - 1)
  assert.equal(doUso(), 1)
  await store.usage(id('a'), undefined, 1_000 + USO_VALIDADE_MS)
  assert.equal(doUso(), 2)
  // Outro Space é outra leitura; outra conta também.
  await store.usage(id('a'), 'trabalho', 1_000 + USO_VALIDADE_MS)
  await store.usage(id('b'), undefined, 1_000 + USO_VALIDADE_MS)
  assert.equal(doUso(), 4)
  // Mudar a conta (conectar, sair, renomear) esquece o que foi lido.
  await store.rename(account({ state: 'connected' }), 'Outro')
  await store.usage(id('a'), undefined, 1_000 + USO_VALIDADE_MS)
  assert.equal(doUso(), 5)
  // Falha não fica guardada: a próxima tentativa lê de novo.
  hub.fail(new Error('usage_unavailable'))
  await assert.rejects(store.usage(id('c'), undefined, 5_000_000))
  await store.usage(id('c'), undefined, 5_000_001)
  assert.equal(doUso(), 7)
})

test('perfil inválido ao desconectar relê a lista: o Hub já gravou a conta desconectada', async () => {
  const original = account({ state: 'connected' })
  const hub = fakeHub([original])
  const contas = new ContasStore(client, hub.api)
  await contas.load()
  hub.set([{ ...original, state: 'disconnected', revision: 2 }])
  hub.fail(new Error('profile_invalid: referência inválida.'))
  await assert.rejects(contas.disconnect(original).catch(falhaEm('pt-BR')), /perfil/)
  assert.equal(contas.account(original.id)?.state, 'disconnected', 'a lista relida já mostra a conta desconectada')
  assert.equal(hub.calls.filter((c) => c.method === 'accounts').length, 2, 'a lista foi relida')
})

test('falha de desconexão sem código conhecido também relê a lista: o Hub já gravou o estado', async () => {
  const original = account({ state: 'connected' })
  const hub = fakeHub([original])
  const contas = new ContasStore(client, hub.api)
  await contas.load()
  hub.set([{ ...original, state: 'disconnected', revision: 2 }])
  hub.fail(new Error('erro cru de leitura do perfil'))
  await assert.rejects(contas.disconnect(original).catch(falhaEm('pt-BR')))
  assert.equal(contas.account(original.id)?.state, 'disconnected', 'a lista relida mostra a conta já desconectada')
  assert.equal(hub.calls.filter((c) => c.method === 'accounts').length, 2, 'a lista foi relida mesmo sem código conhecido')
})

test('pausar manda o estado novo ao Hub e relê a lista em qualquer falha', async () => {
  const original = account({ state: 'connected', enabled: true })
  const hub = fakeHub([original])
  const contas = new ContasStore(client, hub.api)
  await contas.load()
  hub.set([{ ...original, state: 'disconnected', enabled: false, revision: 2 }])
  hub.fail(new Error('erro cru'))
  await assert.rejects(contas.setEnabled(original, false).catch(falhaEm('pt-BR')))
  assert.deepEqual(hub.calls.find((c) => c.method === 'setEnabled')?.body, { engineAccountId: original.id, expectedRevision: 1, enabled: false })
  const pausada = contas.account(original.id)
  assert.equal(pausada?.enabled, false, 'a lista relida mostra a pausa já gravada no Hub')
  assert.equal(pausada?.state, 'disconnected')
  assert.equal(hub.calls.filter((c) => c.method === 'accounts').length, 2, 'relê o cadastro mesmo sem código conhecido')
})

test('reativar devolve a conta do Hub com a revisão nova; o estado de conexão é sempre do Hub', async () => {
  const pausada = account({ state: 'disconnected', enabled: false, revision: 2 })
  const hub = fakeHub([pausada])
  const contas = new ContasStore(client, hub.api)
  await contas.load()
  const reativada = await contas.setEnabled(pausada, true)
  assert.deepEqual(hub.calls.find((c) => c.method === 'setEnabled')?.body, { engineAccountId: pausada.id, expectedRevision: 2, enabled: true })
  assert.equal(reativada.revision, 3)
  assert.equal(contas.account(pausada.id)?.enabled, true)
})

import assert from 'node:assert/strict'
import test from 'node:test'
import { aoMudarContaOpen, aplicarModeloOpen, contasEscolhiveis, modelosDaConta, padraoDaConta } from './fork-open-modelo.ts'

const conta = (over: Record<string, unknown> = {}) => ({
  backend: 'codex' as const,
  state: 'connected' as const,
  models: [{ id: 'gpt-padrao' }, { id: 'gpt-mini' }, { id: 'gpt-oculto' }],
  hiddenModelIds: ['gpt-oculto'],
  ...over,
}) as never

test('modelos da conta seguem a ordem do Hub, sem os ocultos, e o padrão é o primeiro', () => {
  assert.deepEqual(modelosDaConta(conta()), ['gpt-padrao', 'gpt-mini'])
  assert.equal(padraoDaConta(conta()), 'gpt-padrao')
  assert.deepEqual(modelosDaConta(null), [])
  assert.equal(padraoDaConta(conta({ models: [] })), null)
})

test('só conta Codex conectada e com catálogo comprovado pode receber escolha', () => {
  const ok = conta()
  const outras = [conta({ backend: 'claude' }), conta({ state: 'needs_login' }), conta({ models: [] }), conta({ models: [{ id: 'x' }], hiddenModelIds: ['x'] })]
  assert.deepEqual(contasEscolhiveis([ok, ...outras]), [ok])
  assert.deepEqual(contasEscolhiveis(null), [])
})

test('aplicar sem bot não manda botId; com bot manda; e avisa quem mostra prontidão', async () => {
  const chamadas: Array<[string, unknown]> = []
  const client = { call: async <T>(method: string, body?: unknown) => { chamadas.push([method, body]); return { applied: ['berga'] } as T } }
  let avisos = 0
  const parar = aoMudarContaOpen(() => { avisos += 1 })
  assert.deepEqual(await aplicarModeloOpen(client, { accountId: 'a1', model: 'gpt-mini', botId: undefined }), ['berga'])
  await aplicarModeloOpen(client, { accountId: 'a1', model: 'gpt-mini', botId: 'b2' })
  parar()
  await aplicarModeloOpen(client, { accountId: 'a1', model: 'gpt-mini' })
  assert.deepEqual(chamadas.map(([, b]) => b), [
    { accountId: 'a1', model: 'gpt-mini' },
    { accountId: 'a1', model: 'gpt-mini', botId: 'b2' },
    { accountId: 'a1', model: 'gpt-mini' },
  ])
  assert.ok(chamadas.every(([m]) => m === 'donnaOpenModelSet'))
  assert.equal(avisos, 2)
})

test('falha do Hub não avisa mudança', async () => {
  let avisos = 0
  const parar = aoMudarContaOpen(() => { avisos += 1 })
  const client = { call: async () => { throw new Error('model_unavailable') } }
  await assert.rejects(aplicarModeloOpen(client, { accountId: 'a1', model: 'x' }), /model_unavailable/)
  parar()
  assert.equal(avisos, 0)
})

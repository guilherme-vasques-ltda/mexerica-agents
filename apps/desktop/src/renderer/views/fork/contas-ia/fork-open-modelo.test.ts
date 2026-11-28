import assert from 'node:assert/strict'
import test from 'node:test'
import { alvoDaTecla, aoMudarContaOpen, aplicarModeloOpen, contasEscolhiveis, modelosDaConta, opcoesDoModelo, padraoDaConta, valorDaOpcao } from './fork-open-modelo.ts'

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

test('opções do modelo vêm só do catálogo da conta e o valor mostrado respeita o que ela aceita', () => {
  const opcoes = [{ id: 'reasoning_effort', values: ['low', 'high'], default: 'low' }, { id: 'service_tier', values: ['fast'], default: null }, { id: 'context', values: ['1m'], default: null }, { id: 'vazia', values: [], default: null }]
  const c = { models: [{ id: 'gpt-mini', options: opcoes }, { id: 'gpt-padrao', options: [] }] } as never
  const lista = opcoesDoModelo(c, 'gpt-mini')
  assert.deepEqual(lista.map((o) => o.id), ['reasoning_effort', 'service_tier'])
  assert.deepEqual(opcoesDoModelo(c, 'gpt-padrao'), [])
  assert.deepEqual(opcoesDoModelo(c, 'outro'), [])
  assert.equal(valorDaOpcao(lista[0]!, { reasoning_effort: 'high' }), 'high')
  assert.equal(valorDaOpcao(lista[0]!, { reasoning_effort: 'ultra' }), 'low')
  assert.equal(valorDaOpcao(lista[1]!, undefined), null)
})

test('opções vão ao Hub só com bot', async () => {
  const chamadas: unknown[] = []
  const client = { call: async <T>(_m: string, body?: unknown) => { chamadas.push(body); return { applied: [] } as T } }
  await aplicarModeloOpen(client, { accountId: 'a1', model: 'm', botId: 'b', options: { service_tier: 'fast' } })
  await aplicarModeloOpen(client, { accountId: 'a1', model: 'm', options: { service_tier: 'fast' } })
  assert.deepEqual(chamadas, [{ accountId: 'a1', model: 'm', botId: 'b', options: { service_tier: 'fast' } }, { accountId: 'a1', model: 'm' }])
})

test('falha do Hub não avisa mudança', async () => {
  let avisos = 0
  const parar = aoMudarContaOpen(() => { avisos += 1 })
  const client = { call: async () => { throw new Error('model_unavailable') } }
  await assert.rejects(aplicarModeloOpen(client, { accountId: 'a1', model: 'x' }), /model_unavailable/)
  parar()
  assert.equal(avisos, 0)
})

test('teclas do painel andam em círculo e vão às pontas', () => {
  assert.equal(alvoDaTecla('ArrowDown', 4, 3), 0)
  assert.equal(alvoDaTecla('ArrowUp', 4, 0), 3)
  assert.equal(alvoDaTecla('ArrowDown', 4, -1), 0)
  assert.equal(alvoDaTecla('Home', 4, 2), 0)
  assert.equal(alvoDaTecla('End', 4, 1), 3)
  assert.equal(alvoDaTecla('a', 4, 1), null)
  assert.equal(alvoDaTecla('End', 0, 0), null)
})

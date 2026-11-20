import { test } from 'node:test'
import assert from 'node:assert/strict'
import { CampoInstrucao, LIMITE, chaveDaFalha } from './campo.ts'

// Hub falso com a mesma regra de revisão (CAS) do Hub real.
function hub() {
  const state = { geral: { texto: '', revision: 0 }, space: { texto: '', revision: 0 } }
  const calls: { method: string; body: Record<string, unknown> }[] = []
  const snapshot = () => ({ limite: LIMITE, geral: { ...state.geral }, space: { spaceId: 'p', ...state.space } })
  const link = {
    async call(method: string, body: Record<string, unknown> = {}) {
      calls.push({ method, body })
      if (method === 'instrucaoGeral') return snapshot()
      const alvo = body.escopo === 'space' ? state.space : state.geral
      if (body.expectedRevision !== alvo.revision) throw Object.assign(new Error('instrucao_geral_revision_conflict'), { status: 409 })
      alvo.texto = body.texto as string
      alvo.revision += 1
      return snapshot()
    },
  }
  return { state, calls, link }
}

test('o campo geral lê e grava pela RPC do dono, com a revisão esperada', async () => {
  const h = hub()
  const campo = new CampoInstrucao(h.link, { tipo: 'geral' })
  await campo.ler()
  assert.equal(campo.estado, 'instrucoes.estado.vazio')
  campo.editar('  Respostas curtas.\r\n')
  assert.equal(campo.estado, 'instrucoes.estado.naoSalvo')
  await campo.salvar()
  const save = h.calls.find((c) => c.method === 'instrucaoGeralSalvar')!
  assert.deepEqual(save.body, { escopo: 'geral', texto: 'Respostas curtas.', expectedRevision: 0 })
  assert.equal(h.state.geral.texto, 'Respostas curtas.')
  assert.equal(campo.estado, 'instrucoes.estado.salvo')
  assert.equal(campo.revisao, 1)
})

test('o campo do Space lê e grava o próprio escopo, sem tocar no geral', async () => {
  const h = hub()
  const campo = new CampoInstrucao(h.link, { tipo: 'space', spaceId: 'p' })
  await campo.ler()
  assert.deepEqual(h.calls[0], { method: 'instrucaoGeral', body: { spaceId: 'p' } })
  campo.editar('Tom formal.')
  await campo.salvar()
  assert.deepEqual(h.calls[1]!.body, { escopo: 'space', spaceId: 'p', texto: 'Tom formal.', expectedRevision: 0 })
  assert.equal(h.state.space.texto, 'Tom formal.')
  assert.equal(h.state.geral.texto, '')
})

test('texto acima do limite não é enviado e conta por caractere Unicode', async () => {
  const h = hub()
  const campo = new CampoInstrucao(h.link, { tipo: 'geral' })
  await campo.ler()
  campo.editar('é'.repeat(LIMITE))
  assert.equal(campo.excede, false)
  campo.editar('é'.repeat(LIMITE + 1))
  assert.equal(campo.excede, true)
  assert.equal(campo.estado, 'instrucoes.falha.limite')
  await campo.salvar()
  assert.equal(h.calls.filter((c) => c.method === 'instrucaoGeralSalvar').length, 0)
})

test('conflito: tentar de novo relê a revisão nova e grava o texto da pessoa', async () => {
  const h = hub()
  const campo = new CampoInstrucao(h.link, { tipo: 'geral' })
  await campo.ler()
  h.state.geral = { texto: 'Vindo de outro lugar.', revision: 5 }
  campo.editar('Meu texto.')
  await campo.salvar()
  assert.equal(campo.fase, 'conflito')
  assert.equal(campo.estado, 'instrucoes.estado.conflito')
  await campo.tentarDeNovo()
  assert.equal(h.state.geral.texto, 'Meu texto.')
  assert.equal(campo.estado, 'instrucoes.estado.salvo')
  assert.equal(campo.revisao, 6)
})

test('falha de leitura mantém o rascunho e Tentar de novo só relê', async () => {
  const h = hub()
  let cai = true
  const link = { call: async (m: string, b?: unknown) => { if (cai) throw Object.assign(new Error('x'), { status: 0 }); return h.link.call(m, b as Record<string, unknown>) } }
  const campo = new CampoInstrucao(link, { tipo: 'geral' })
  await campo.ler()
  assert.equal(campo.estado, 'instrucoes.falha.leitura')
  campo.editar('Rascunho.')
  cai = false
  await campo.tentarDeNovo()
  assert.equal(campo.texto, 'Rascunho.')
  assert.equal(campo.fase, 'pronto')
  assert.equal(h.calls.filter((c) => c.method === 'instrucaoGeralSalvar').length, 0)
})

test('as falhas do Hub viram chaves de texto', () => {
  assert.equal(chaveDaFalha(Object.assign(new Error('unknown method'), { status: 404 })), 'instrucoes.falha.hubAntigo')
  assert.equal(chaveDaFalha(Object.assign(new Error('forbidden'), { status: 403 })), 'instrucoes.falha.soDono')
  assert.equal(chaveDaFalha(new Error('limit_exceeded: o campo')), 'instrucoes.falha.limite')
  assert.equal(chaveDaFalha(new Error('invalid_field: invisível')), 'instrucoes.falha.invalido')
  assert.equal(chaveDaFalha(new Error('space_not_found')), 'instrucoes.falha.spaceSumiu')
  assert.equal(chaveDaFalha(new Error('qualquer outra')), 'instrucoes.falha.envio')
})

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
  const campo = new CampoInstrucao(() => h.link, { tipo: 'geral' })
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
  const campo = new CampoInstrucao(() => h.link, { tipo: 'space', spaceId: 'p' })
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
  const campo = new CampoInstrucao(() => h.link, { tipo: 'geral' })
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
  const campo = new CampoInstrucao(() => h.link, { tipo: 'geral' })
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
  const campo = new CampoInstrucao(() => link, { tipo: 'geral' })
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

test('falha de leitura por Hub antigo ou sem acesso: Tentar de novo só relê, nunca grava vazio sobre o que existe', async () => {
  const h = hub()
  h.state.geral = { texto: 'Já existia.', revision: 3 }
  let nega = true
  const link = { call: async (m: string, b?: unknown) => { if (nega) throw Object.assign(new Error('forbidden'), { status: 403 }); return h.link.call(m, b as Record<string, unknown>) } }
  const campo = new CampoInstrucao(() => link, { tipo: 'geral' })
  await campo.ler()
  assert.equal(campo.estado, 'instrucoes.falha.soDono')
  nega = false
  await campo.tentarDeNovo()
  assert.equal(campo.texto, 'Já existia.')
  assert.equal(h.state.geral.texto, 'Já existia.')
  assert.equal(h.calls.filter((c) => c.method === 'instrucaoGeralSalvar').length, 0)
})

test('rascunho com caractere de controle não é enviado; tabulação e quebra de linha valem', async () => {
  const h = hub()
  const campo = new CampoInstrucao(() => h.link, { tipo: 'geral' })
  await campo.ler()
  campo.editar('a\u0007b')
  assert.equal(campo.invalido, true)
  assert.equal(campo.estado, 'instrucoes.falha.invalido')
  await campo.salvar()
  assert.equal(h.calls.filter((c) => c.method === 'instrucaoGeralSalvar').length, 0)
  campo.editar('- item\n\t- recuado')
  assert.equal(campo.invalido, false)
})

test('a reconexão troca o link sem perder o rascunho: a fonte é lida a cada chamada', async () => {
  const antigo = hub()
  const novo = hub()
  let atual: typeof antigo.link | null = antigo.link
  const campo = new CampoInstrucao(() => atual, { tipo: 'geral' })
  await campo.ler()
  campo.editar('Rascunho que sobrevive.')
  atual = null
  await campo.salvar()
  assert.equal(campo.fase, 'falha')
  assert.equal(campo.texto, 'Rascunho que sobrevive.')
  atual = novo.link
  await campo.tentarDeNovo()
  assert.equal(novo.state.geral.texto, 'Rascunho que sobrevive.')
  assert.equal(campo.estado, 'instrucoes.estado.salvo')
})

test('reabrir com rascunho e o Hub mudado por fora avisa o conflito em vez de gravar por cima', async () => {
  const h = hub()
  const campo = new CampoInstrucao(() => h.link, { tipo: 'geral' })
  await campo.ler()
  campo.editar('Meu rascunho.')
  h.state.geral = { texto: 'Mudou em outra janela.', revision: 4 }
  await campo.ler(campo.sujo)
  assert.equal(campo.fase, 'conflito')
  assert.equal(campo.estado, 'instrucoes.estado.conflito')
  assert.equal(campo.texto, 'Meu rascunho.')
  assert.equal(h.calls.filter((c) => c.method === 'instrucaoGeralSalvar').length, 0)
  await campo.tentarDeNovo()
  assert.equal(h.state.geral.texto, 'Meu rascunho.')
  assert.equal(campo.estado, 'instrucoes.estado.salvo')
})

test('reabrir com rascunho e o Hub igual mantém o rascunho sem conflito', async () => {
  const h = hub()
  const campo = new CampoInstrucao(() => h.link, { tipo: 'geral' })
  await campo.ler()
  campo.editar('Rascunho.')
  await campo.ler(campo.sujo)
  assert.equal(campo.fase, 'pronto')
  assert.equal(campo.texto, 'Rascunho.')
})

test('reabrir a página com conflito pendente não o apaga: só Tentar de novo decide', async () => {
  const h = hub()
  const campo = new CampoInstrucao(() => h.link, { tipo: 'geral' })
  await campo.ler()
  h.state.geral = { texto: 'Mudou em outra janela.', revision: 4 }
  campo.editar('Meu rascunho.')
  await campo.salvar()
  assert.equal(campo.fase, 'conflito')
  await campo.ler(campo.sujo)
  assert.equal(campo.fase, 'conflito', 'a releitura passiva mantém o aviso')
  assert.equal(h.state.geral.texto, 'Mudou em outra janela.')
  await campo.tentarDeNovo()
  assert.equal(h.state.geral.texto, 'Meu rascunho.')
})

test('o que a pessoa digita enquanto a leitura volta não é apagado', async () => {
  const h = hub()
  let solta: (() => void) | null = null
  const link = { call: (m: string, b?: unknown) => new Promise((resolve, reject) => { solta = () => h.link.call(m, b as Record<string, unknown>).then(resolve, reject) }) }
  const campo = new CampoInstrucao(() => link, { tipo: 'geral' })
  const leitura = campo.ler()
  campo.editar('Digitado durante a leitura.')
  solta!()
  await leitura
  assert.equal(campo.texto, 'Digitado durante a leitura.')
})

test('sem leitura bem-sucedida o campo não é editável (não há revisão para gravar por cima)', async () => {
  const h = hub()
  const link = { call: async () => { throw Object.assign(new Error('x'), { status: 0 }) } }
  const campo = new CampoInstrucao(() => link, { tipo: 'geral' })
  await campo.ler()
  assert.equal(campo.lido, false)
  const bom = new CampoInstrucao(() => h.link, { tipo: 'geral' })
  await bom.ler()
  assert.equal(bom.lido, true)
})

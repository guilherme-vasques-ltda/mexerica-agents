import assert from 'node:assert/strict'
import { test } from 'node:test'
import { decidir, editar, lerPedido, lerTentativa, tela, type Contexto, type ReciboConector } from './modelo.ts'
import { textoDe, cru, ErroFrase } from '../../../lib/fork-i18n/frase.ts'

const recibo = (state = 'aguardando', extra: object = {}) => ({
  tentativaId: 't1', snapshotId: 's1', cardId: 'c1', cardRev: 42, requestDigest: 'a'.repeat(64), state, delivery: 'nao_liberada',
  t0: 1, cardDeadline: 2_000_000_000_000, globalDeadline: 2_000_000_000_000, admittedAt: null, completedAt: null, decisionProtocol: 2,
  evidence: 'sem_conclusao_externa', ...extra,
})
const apresentacao = {
  efeito: 'Guardar rascunho no Gmail', ferramenta: 'create_draft', argumentos: { to: 'bruno@cliente.exemplo.com', body: 'Oi, Bruno.' },
  rotulos: { to: 'Para', body: 'Mensagem' }, conta: { rotulo: 'Gmail da Donna (exemplo)', identidade: 'donna@exemplo.com' },
}
const aberto: Contexto = { suportado: true, apresentacao: true, conta: true, vencido: false, substituido: false }

function hub(resposta: (metodo: string, corpo: object) => unknown) {
  const chamadas: { metodo: string; corpo: object }[] = []
  return { chamadas, chamar: async (metodo: string, corpo: object) => { chamadas.push({ metodo, corpo }); return resposta(metodo, corpo) } }
}

test('lê as duas formas do Hub: pedido aberto com apresentação e recibo encerrado sem conteúdo', () => {
  const vivo = lerTentativa({ receipt: recibo(), presentation: apresentacao })
  assert.equal(vivo.recibo.cardRev, 42)
  assert.deepEqual(vivo.apresentacao, apresentacao)
  const fim = lerTentativa(recibo('incerta'))
  assert.equal(fim.recibo.state, 'incerta')
  assert.equal(fim.apresentacao, null, 'encerrado: nada reconstruído no app')
  assert.equal(lerTentativa({ receipt: recibo(), presentation: null }).apresentacao, null)
  assert.equal(lerTentativa({ receipt: recibo(), presentation: { ...apresentacao, argumentos: { n: 1 } } }).apresentacao, null, 'argumento fora da forma V1 não vira cartão')
  assert.throws(() => lerTentativa({ receipt: { ...recibo(), cardRev: '42' } }), (e: unknown) => e instanceof ErroFrase && /incompleto/.test(textoDe(e.frase, 'pt-BR')))
  assert.throws(() => lerTentativa(recibo('inventado')), (e: unknown) => e instanceof ErroFrase && /incompleto/.test(textoDe(e.frase, 'pt-BR')))
})

test('estado do Hub → tela: só mostra botões com pedido aberto, atual, no prazo e com conteúdo', () => {
  assert.equal(tela('aguardando', aberto).decide, true)
  for (const c of [{ vencido: true }, { substituido: true }, { apresentacao: false }, { conta: false }, { suportado: false }]) {
    assert.equal(tela('aguardando', { ...aberto, ...c }).decide, false, JSON.stringify(c))
  }
  assert.equal(textoDe(tela('aguardando', { ...aberto, vencido: true }).selo, 'pt-BR'), 'Venceu')
  const esperado: Record<string, string> = {
    autorizada: 'Aprovado por você', em_envio: 'Enviando', concluida: 'Concluído', falha_comprovada: 'Falhou',
    incerta: 'Não confirmado', recusada: 'Recusado por você', expirada: 'Venceu', cancelada: 'Cancelado',
  }
  for (const [estado, selo] of Object.entries(esperado)) {
    const t = tela(estado, aberto)
    assert.equal(textoDe(t.selo, 'pt-BR'), selo)
    assert.equal(t.decide, false, `${estado} não decide de novo`)
  }
  assert.match(textoDe(tela('incerta', aberto).nota!, 'pt-BR'), /^Não sabemos se saiu; nada foi repetido\./)
  assert.equal(tela('futuro', aberto).decide, false, 'estado desconhecido não ganha botão')
  const recusa = tela('aguardando', { ...aberto, apresentacao: false, recusaLeitura: cru('Conectores curados exigem o operador Local') })
  assert.equal(recusa.decide, false)
  assert.match(textoDe(recusa.nota!, 'pt-BR'), /operador Local/, 'a recusa do Hub aparece, não uma frase genérica')
  assert.equal(textoDe(tela('incerta', { ...aberto, recusaLeitura: cru('x') }).selo, 'pt-BR'), 'Não confirmado', 'estado encerrado vem da entrada pública mesmo sem leitura')
})

test('aprovar e recusar chamam donnaConectorDecidir com os IDs do recibo, protocolo 2 e origem app', async () => {
  const r = lerTentativa(recibo()).recibo
  const h = hub(() => recibo('autorizada', { cardRev: 43 }))
  const novo = await decidir(h.chamar, r, 'aprovar_uma_vez')
  await decidir(h.chamar, r, 'recusar')
  assert.equal(novo.state, 'autorizada')
  assert.deepEqual(h.chamadas.map((c) => c.metodo), ['donnaConectorDecidir', 'donnaConectorDecidir'])
  assert.deepEqual(h.chamadas[0]!.corpo, { protocol: 2, tentativaId: 't1', snapshotId: 's1', cardId: 'c1', cardRev: 42, requestDigest: 'a'.repeat(64), decisao: 'aprovar_uma_vez', source: 'app' })
  assert.equal((h.chamadas[1]!.corpo as { decisao: string }).decisao, 'recusar')
})

test('editar manda só o que mudou por donnaConectorEditar; sem mudança, não chama o Hub', async () => {
  const r: ReciboConector = lerTentativa(recibo()).recibo
  const h = hub(() => recibo('aguardando', { cardId: 'c2', snapshotId: 's2', cardRev: 44 }))
  assert.equal(await editar(h.chamar, r, apresentacao.argumentos, { ...apresentacao.argumentos }), null)
  assert.equal(h.chamadas.length, 0)
  const novo = await editar(h.chamar, r, apresentacao.argumentos, { ...apresentacao.argumentos, body: 'Oi, Bruno!', extra: 'x' })
  assert.equal(novo?.cardId, 'c2')
  assert.deepEqual(h.chamadas, [{ metodo: 'donnaConectorEditar', corpo: { protocol: 2, tentativaId: 't1', snapshotId: 's1', cardId: 'c1', cardRev: 42, requestDigest: 'a'.repeat(64), source: 'app', patch: { body: 'Oi, Bruno!' } } }])
})

test('ler usa donnaConectorTentativa e repassa a recusa do Hub como erro, sem cartão', async () => {
  const h = hub(() => { throw new Error('Conectores curados exigem o operador Local') })
  await assert.rejects(lerPedido(h.chamar, 't1'), /operador Local/)
  assert.deepEqual(h.chamadas, [{ metodo: 'donnaConectorTentativa', corpo: { tentativaId: 't1' } }])
})

test('conta de saída e rótulos vêm só do Hub: sem conta não aprova; sem rótulo curado não há cartão', () => {
  const semConta = lerTentativa({ receipt: recibo(), presentation: { ...apresentacao, conta: undefined } }).apresentacao
  assert.equal(semConta?.conta, null, 'resposta sem conta não ganha conta padrão')
  assert.equal(lerTentativa({ receipt: recibo(), presentation: { ...apresentacao, conta: { rotulo: 'Gmail', identidade: '' } } }).apresentacao?.conta, null)
  const t = tela('aguardando', { ...aberto, conta: false })
  assert.equal(t.decide, false)
  assert.match(textoDe(t.nota!, 'pt-BR'), /conta de saída não foi identificada/)
  assert.equal(lerTentativa({ receipt: recibo(), presentation: { ...apresentacao, rotulos: { to: 'Para' } } }).apresentacao, null, 'campo sem rótulo curado não vira nome técnico')
  assert.equal(lerTentativa({ receipt: recibo(), presentation: { ...apresentacao, rotulos: undefined } }).apresentacao, null, 'apresentação do protocolo 1 não basta')
  assert.deepEqual(lerTentativa({ receipt: recibo(), presentation: apresentacao }).apresentacao?.conta, apresentacao.conta)
})

test('app só decide no protocolo 2; recibo antigo encerrado continua legível, sem botão', () => {
  assert.equal(textoDe(tela('aguardando', { ...aberto, suportado: false }).selo, 'pt-BR'), 'Não dá para decidir aqui')
  const antigo = tela('concluida', { ...aberto, suportado: false })
  assert.equal(textoDe(antigo.selo, 'pt-BR'), 'Concluído')
  assert.equal(antigo.decide, false)
  assert.equal(tela('autorizada', { ...aberto, suportado: false }).decide, false)
})

import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  CHAVE_ARMAZENAMENTO_LADO_A_LADO, MAX_PAINEIS, MIN_LARGURA_PAINEL, atualizarSessaoPainel, abrirNoQuadro,
  calcularEncaixeLargura, ciclarFocoPainel, criarPainelId, definirAtivo, escopoDoSpace, estadoVazio,
  focarPainel, mesmaRefLadoALado, moverPainel, paineisVisiveis, parseLadoALadoMapa, pertenceAoEscopo,
  reconciliarEscopo, removerPainel, trocarPainel,
  type ConversaDisponivel, type LadoALadoEscopo, type LadoALadoEscopoEstado, type LadoALadoRef,
} from './fork-lado-a-lado.ts'

// Quadro Lado a lado: regras puras. Escopo conferido antes de abrir; painel repetido foca em vez
// de duplicar; limite não perde o que já está aberto; reconciliar sem mudança devolve a mesma
// referência; dado gravado inválido nunca vira painel.

const ref = (extra: Partial<LadoALadoRef> = {}): LadoALadoRef => ({ computerId: 'comp-1', botId: 'bot-1', ...extra })
const escopo = (extra: Partial<LadoALadoEscopo> = {}): LadoALadoEscopo => ({ computerId: 'comp-1', spaceId: 'esp-1', ...extra })

let contador = 0
const id = () => `p${++contador}`
const abrir = (estado: LadoALadoEscopoEstado, r: LadoALadoRef, esc: LadoALadoEscopo | null, opcoes?: Parameters<typeof abrirNoQuadro>[3]) =>
  abrirNoQuadro(estado, r, esc, { criarId: id, ...opcoes })

function quadro(n: number, esc: LadoALadoEscopo | null = null, limite = 8): LadoALadoEscopoEstado {
  let estado = estadoVazio()
  for (let i = 0; i < n; i++) {
    const r = esc
      ? ref({ botId: `bot-${i}`, spaceId: esc.spaceId, hubId: esc.hostId })
      : ref({ botId: `bot-${i}` })
    estado = abrir(estado, r, esc, { limite }).estado
  }
  return estado
}

test('escopo: chave pessoal sem escopo e composta com espaço', () => {
  assert.equal(escopoDoSpace(null), 'pessoal')
  assert.equal(escopoDoSpace(escopo()), 'space:comp-1:esp-1')
  assert.equal(escopoDoSpace(escopo({ hostId: 'hub-9' })), 'space:hub-9:esp-1')
  assert.equal(escopoDoSpace(escopo({ computerId: 'comp-2' })), 'space:comp-2:esp-1')
})

test('estado vazio: fechado, sem foco, sem painéis', () => {
  assert.deepEqual(estadoVazio(), { ativo: false, focoId: null, paineis: [] })
})

test('mesma ref: nula não casa; espaço e hub ausentes valem iguais', () => {
  assert.equal(mesmaRefLadoALado(null, null), false)
  assert.equal(mesmaRefLadoALado(ref(), null), false)
  assert.equal(mesmaRefLadoALado(null, ref()), false)
  assert.equal(mesmaRefLadoALado(ref(), ref()), true)
  assert.equal(mesmaRefLadoALado(ref(), ref({ botId: 'bot-2' })), false)
  assert.equal(mesmaRefLadoALado(ref(), ref({ computerId: 'comp-2' })), false)
  assert.equal(mesmaRefLadoALado(ref({ spaceId: undefined }), ref()), true)
  assert.equal(mesmaRefLadoALado(ref(), ref({ spaceId: 'esp-1' })), false)
  assert.equal(mesmaRefLadoALado(ref({ spaceId: 'esp-1' }), ref({ spaceId: 'esp-1' })), true)
  assert.equal(mesmaRefLadoALado(ref({ hubId: 'hub-1' }), ref()), false)
  assert.equal(mesmaRefLadoALado(ref({ hubId: 'hub-1' }), ref({ hubId: 'hub-1' })), true)
})

test('pertence: sem escopo só conversa sem espaço; com escopo espaço e computador iguais e hub conferido', () => {
  assert.equal(pertenceAoEscopo(ref(), null), true)
  assert.equal(pertenceAoEscopo(ref({ spaceId: 'esp-1' }), null), false)
  const e = escopo()
  assert.equal(pertenceAoEscopo(ref({ spaceId: 'esp-1' }), e), true)
  assert.equal(pertenceAoEscopo(ref({ spaceId: 'esp-2' }), e), false)
  assert.equal(pertenceAoEscopo(ref({ spaceId: 'esp-1', computerId: 'comp-2' }), e), false)
  assert.equal(pertenceAoEscopo(ref({ spaceId: 'esp-1', hubId: 'hub-1' }), escopo({ hostId: 'hub-1' })), true)
  assert.equal(pertenceAoEscopo(ref({ spaceId: 'esp-1', hubId: 'hub-2' }), escopo({ hostId: 'hub-1' })), false)
  // Hub declarado de um lado só: não há conflito a conferir.
  assert.equal(pertenceAoEscopo(ref({ spaceId: 'esp-1', hubId: 'hub-2' }), e), true)
  assert.equal(pertenceAoEscopo(ref({ spaceId: 'esp-1' }), escopo({ hostId: 'hub-1' })), true)
})

test('id de painel: do gerador quando dado; único sem gerador', () => {
  assert.equal(criarPainelId(() => 'fixo'), 'fixo')
  const a = criarPainelId()
  const b = criarPainelId()
  assert.equal(typeof a, 'string')
  assert.ok(a.length > 0)
  assert.notEqual(a, b)
})

test('abrir: ref fora do escopo, sem computador ou sem bot é recusada sem mudar nada', () => {
  const estado = quadro(1)
  for (const ruim of [ref({ spaceId: 'outro' }), ref({ computerId: '', spaceId: 'esp-1' }), ref({ botId: '', spaceId: 'esp-1' })]) {
    const r = abrir(estado, ruim, escopo())
    assert.equal(r.resultado, 'escopo_invalido')
    assert.equal(r.painelId, null)
    assert.equal(r.estado, estado)
  }
  const r = abrir(estado, ref({ spaceId: 'esp-1' }), null)
  assert.equal(r.resultado, 'escopo_invalido')
  assert.equal(r.estado, estado)
})

test('abrir: painel igual foca em vez de duplicar; ativar false não liga o quadro', () => {
  const base = abrir(estadoVazio(), ref({ spaceId: 'esp-1' }), escopo(), { sessionId: 'sess-1' }).estado
  const deNovo = abrir(base, ref({ spaceId: 'esp-1' }), escopo(), { sessionId: 'sess-1' })
  assert.equal(deNovo.resultado, 'focado')
  assert.equal(deNovo.painelId, base.focoId)
  assert.equal(deNovo.estado.paineis.length, 1)

  // Focado reativa o quadro por padrão, mesmo já estando no foco.
  const desligado = definirAtivo(deNovo.estado, false)
  const religado = abrir(desligado, ref({ spaceId: 'esp-1' }), escopo(), { sessionId: 'sess-1' })
  assert.equal(religado.resultado, 'focado')
  assert.equal(religado.estado.ativo, true)

  const semAtivar = abrir(desligado, ref({ spaceId: 'esp-1' }), escopo(), { sessionId: 'sess-1', ativar: false })
  assert.equal(semAtivar.resultado, 'focado')
  assert.equal(semAtivar.estado.ativo, false)
  assert.equal(semAtivar.estado.focoId, base.focoId)
})

test('abrir: mesmo bot com sessão diferente são dois painéis; sessão nula e cheia diferem', () => {
  const e = escopo()
  const semSessao = abrir(estadoVazio(), ref({ spaceId: 'esp-1' }), e).estado
  const comSessao = abrir(semSessao, ref({ spaceId: 'esp-1' }), e, { sessionId: 'sess-1' }).estado
  assert.equal(comSessao.paineis.length, 2)
  assert.deepEqual(comSessao.paineis.map(p => p.sessionId), [null, 'sess-1'])
  const outraSessao = abrir(comSessao, ref({ spaceId: 'esp-1' }), e, { sessionId: 'sess-2' })
  assert.equal(outraSessao.resultado, 'adicionado')
  assert.equal(outraSessao.estado.paineis.length, 3)
})

test('abrir: adicionado foca e ativa por padrão; ativar false deixa o quadro como está', () => {
  const r = abrir(estadoVazio(), ref({ spaceId: 'esp-1' }), escopo())
  assert.equal(r.resultado, 'adicionado')
  assert.equal(r.estado.ativo, true)
  assert.equal(r.estado.focoId, r.painelId)
  assert.deepEqual(r.estado.paineis, [{ id: r.painelId, ref: ref({ spaceId: 'esp-1' }), sessionId: null }])

  const desligado = abrir(definirAtivo(quadro(1), false), ref({ spaceId: 'esp-1', botId: 'bot-2' }), escopo(), { ativar: false })
  assert.equal(desligado.resultado, 'adicionado')
  assert.equal(desligado.estado.ativo, false)
})

test('abrir: cheio não adiciona e devolve o mesmo estado; limite próprio é respeitado', () => {
  const e = escopo()
  const cheio = quadro(MAX_PAINEIS, e)
  const r = abrir(cheio, ref({ spaceId: 'esp-1', botId: 'bot-9' }), e)
  assert.equal(r.resultado, 'limite')
  assert.equal(r.painelId, null)
  assert.equal(r.estado, cheio)
  assert.equal(r.estado.paineis.length, MAX_PAINEIS)

  // Painel que já existe no quadro cheio continua sendo focado, não vira limite.
  const repetido = abrir(cheio, ref({ spaceId: 'esp-1', botId: 'bot-1' }), e)
  assert.equal(repetido.resultado, 'focado')

  const apertado = quadro(2, e)
  const negado = abrir(apertado, ref({ spaceId: 'esp-1', botId: 'bot-9' }), e, { limite: 2 })
  assert.equal(negado.resultado, 'limite')
  assert.equal(negado.estado, apertado)
  const cabeca = abrir(apertado, ref({ spaceId: 'esp-1', botId: 'bot-9' }), e, { limite: 3 })
  assert.equal(cabeca.resultado, 'adicionado')
})

test('remover: foco do painel tirado vai ao vizinho anterior (ou ao próximo); último fecha o quadro', () => {
  const e = escopo()
  const tres = quadro(3, e)
  const ids = tres.paineis.map(p => p.id)

  // O do meio não é o foco: tirar deixa o foco onde está.
  const tirandoMeio = removerPainel(tres, ids[1])
  assert.equal(tirandoMeio.focoId, tres.focoId)
  assert.deepEqual(tirandoMeio.paineis.map(p => p.id), [ids[0], ids[2]])

  // O foco é o último: cai no vizinho anterior.
  const tirandoFoco = removerPainel(tres, ids[2])
  assert.equal(tirandoFoco.focoId, ids[1])
  assert.deepEqual(tirandoFoco.paineis.map(p => p.id), [ids[0], ids[1]])

  // O foco é o primeiro: vai ao próximo que sobrou.
  const focoPrimeiro = focarPainel(tres, ids[0])
  const tirandoPrimeiro = removerPainel(focoPrimeiro, ids[0])
  assert.equal(tirandoPrimeiro.focoId, ids[1])

  const zero = removerPainel(removerPainel(tirandoFoco, ids[1]), ids[0])
  assert.deepEqual(zero, { ativo: false, focoId: null, paineis: [] })
  assert.equal(removerPainel(zero, 'p999'), zero)
})

test('trocar: só dentro do escopo; troca ref e sessão (nula por padrão) e foca', () => {
  const e = escopo()
  const estado = quadro(2, e)
  const alvo = estado.paineis[0].id
  const nova = ref({ spaceId: 'esp-1', botId: 'bot-9' })

  const fora = trocarPainel(estado, alvo, ref({ spaceId: 'outro' }), e)
  assert.equal(fora, estado)

  const trocado = trocarPainel(estado, alvo, nova, e, 'sess-7')
  assert.equal(trocado.focoId, alvo)
  assert.deepEqual(trocado.paineis[0], { id: alvo, ref: nova, sessionId: 'sess-7' })
  assert.deepEqual(trocado.paineis[1], estado.paineis[1])

  const semSessao = trocarPainel(estado, alvo, nova, e)
  assert.equal(semSessao.paineis[0].sessionId, null)

  assert.equal(trocarPainel(estado, 'p999', nova, e), estado)
})

test('sessão: atualiza só o painel pedido, gêmeos de outro painel ficam', () => {
  const e = escopo()
  let estado = quadro(1, e)
  estado = abrir(estado, estado.paineis[0].ref, e, { sessionId: 'sess-1' }).estado
  const [semSessao, comSessao] = estado.paineis
  const novo = atualizarSessaoPainel(estado, semSessao.id, 'sess-2')
  assert.equal(novo.paineis.find(p => p.id === semSessao.id)?.sessionId, 'sess-2')
  assert.equal(novo.paineis.find(p => p.id === comSessao.id)?.sessionId, 'sess-1')
  assert.equal(atualizarSessaoPainel(novo, 'p999', 'sess-3'), novo)
  const limpo = atualizarSessaoPainel(novo, semSessao.id, null)
  assert.equal(limpo.paineis.find(p => p.id === semSessao.id)?.sessionId, null)
})

test('mover: uma posição nas duas direções, limites seguram, foco não muda', () => {
  const e = escopo()
  const estado = quadro(3, e)
  const ids = estado.paineis.map(p => p.id)
  const foco = estado.focoId

  const paraTras = moverPainel(estado, ids[2], -1)
  assert.deepEqual(paraTras.paineis.map(p => p.id), [ids[0], ids[2], ids[1]])
  assert.equal(paraTras.focoId, foco)

  const paraFrente = moverPainel(paraTras, ids[2], 1)
  assert.deepEqual(paraFrente.paineis.map(p => p.id), ids)

  assert.equal(moverPainel(estado, ids[0], -1), estado)
  assert.equal(moverPainel(estado, ids[2], 1), estado)
  assert.equal(moverPainel(estado, 'p999', 1), estado)
})

test('focar: painel existente foca; inexistente ou já focado devolve o mesmo estado', () => {
  const e = escopo()
  const estado = quadro(2, e)
  assert.equal(focarPainel(estado, estado.paineis[0].id).focoId, estado.paineis[0].id)
  assert.equal(focarPainel(estado, estado.focoId as string), estado)
  assert.equal(focarPainel(estado, 'p999'), estado)
})

test('ciclar foco: circular nas duas direções; vazio e único não giram', () => {
  const e = escopo()
  const estado = quadro(3, e)
  const ids = estado.paineis.map(p => p.id)
  const inicial = estado.focoId as string
  const idx = ids.indexOf(inicial)

  const frente = ciclarFocoPainel(estado, 1)
  assert.equal(frente.focoId, ids[(idx + 1) % 3])
  const tras = ciclarFocoPainel(estado, -1)
  assert.equal(tras.focoId, ids[(idx + 2) % 3])

  const dois = removerPainel(estado, ids[1])
  const unico = removerPainel(dois, dois.paineis[1].id)
  assert.equal(ciclarFocoPainel(unico, 1), unico)
  assert.equal(ciclarFocoPainel(unico, -1), unico)
  const vazio = estadoVazio()
  assert.equal(ciclarFocoPainel(vazio, 1), vazio)

  // Foco inválido gira a partir do primeiro.
  const semFoco = { ...estado, focoId: 'p999' }
  assert.equal(ciclarFocoPainel(semFoco, 1).focoId, ids[1])
})

test('ativo: liga e desliga sem tocar nos painéis; valor repetido devolve o mesmo estado', () => {
  const estado = quadro(1)
  const ligado = definirAtivo(estado, true)
  assert.equal(ligado.ativo, true)
  assert.equal(ligado.paineis, estado.paineis)
  assert.equal(definirAtivo(ligado, true), ligado)
  const desligado = definirAtivo(ligado, false)
  assert.equal(desligado.ativo, false)
})

test('reconciliar: painel de outro escopo sai; computador fora de prova fica como está', () => {
  const e = escopo()
  const estado = quadro(2, e)
  const fora = abrir(estado, ref({ botId: 'bot-x' }), null).estado
  const semMudanca = reconciliarEscopo(fora, e, [], new Set(['comp-1']))
  // Nenhum painel sobrou do escopo: quadro fecha e foco some.
  assert.deepEqual(semMudanca.estado, { ativo: false, focoId: null, paineis: [] })

  const comFora = reconciliarEscopo(fora, null, [], new Set())
  assert.equal(comFora.estado.paineis.length, 1)
  assert.equal(comFora.estado.paineis[0].ref.botId, 'bot-x')
})

test('reconciliar: conversa que sumiu sai e conta; sessão que não existe mais é limpa', () => {
  const e = escopo()
  const estado = quadro(2, e)
  const [p1, p2] = estado.paineis
  const comSessao = atualizarSessaoPainel(estado, p2.id, 'sess-velha')
  const disponiveis: ConversaDisponivel[] = [
    { ref: p1.ref, sessionIds: new Set(['sess-outra']) },
    { ref: p2.ref, sessionIds: new Set(['sess-nova']) },
  ]
  const r = reconciliarEscopo(comSessao, e, disponiveis, new Set(['comp-1']))
  assert.equal(r.removidos, 0)
  assert.equal(r.estado.paineis.length, 2)
  assert.equal(r.estado.paineis.find(p => p.id === p1.id)?.sessionId, null)
  assert.equal(r.estado.paineis.find(p => p.id === p2.id)?.sessionId, null)

  const semP1 = reconciliarEscopo(comSessao, e, [{ ref: p2.ref, sessionIds: new Set(['sess-nova']) }], new Set(['comp-1']))
  assert.equal(semP1.removidos, 1)
  assert.deepEqual(semP1.estado.paineis.map(p => p.id), [p2.id])
  assert.equal(semP1.estado.focoId, p2.id)

  // `sessionIds` ausente não afirma nada sobre a sessão: o painel fica com a que tinha.
  const opaco = reconciliarEscopo(comSessao, e, [{ ref: p1.ref }, { ref: p2.ref }], new Set(['comp-1']))
  assert.equal(opaco.estado, comSessao)
  assert.equal(opaco.removidos, 0)
})

test('reconciliar: foco que sai aponta ao primeiro que ficou; sem mudança devolve a mesma referência', () => {
  const e = escopo()
  const estado = quadro(3, e)
  const [p1, p2, p3] = estado.paineis
  const focoNoTerceiro = focarPainel(estado, p3.id)
  const r = reconciliarEscopo(focoNoTerceiro, e, [{ ref: p1.ref }, { ref: p2.ref }], new Set(['comp-1']))
  assert.equal(r.estado.focoId, p1.id)
  assert.equal(r.removidos, 1)

  const semNada = reconciliarEscopo(estado, e, [{ ref: p1.ref }, { ref: p2.ref }, { ref: p3.ref }], new Set(['comp-1']))
  assert.equal(semNada.estado, estado)
  assert.equal(semNada.removidos, 0)
})

test('encaixe: todos, parcial ou um só, nunca zero', () => {
  assert.deepEqual(calcularEncaixeLargura(0, 0), { cabem: 1, estreita: false, modo: 'todos' })
  assert.deepEqual(calcularEncaixeLargura(100, 1), { cabem: 1, estreita: false, modo: 'todos' })
  assert.deepEqual(calcularEncaixeLargura(1080, 3), { cabem: 3, estreita: false, modo: 'todos' })
  assert.deepEqual(calcularEncaixeLargura(1000, 3), { cabem: 2, estreita: true, modo: 'parcial' })
  assert.deepEqual(calcularEncaixeLargura(720, 3), { cabem: 2, estreita: true, modo: 'parcial' })
  assert.deepEqual(calcularEncaixeLargura(360, 3), { cabem: 1, estreita: true, modo: 'unico' })
  assert.deepEqual(calcularEncaixeLargura(359, 3), { cabem: 1, estreita: true, modo: 'unico' })
  assert.deepEqual(calcularEncaixeLargura(-10, 2), { cabem: 1, estreita: true, modo: 'unico' })
  assert.deepEqual(calcularEncaixeLargura(400, 2, 200), { cabem: 2, estreita: false, modo: 'todos' })
  assert.equal(calcularEncaixeLargura(0, 1).cabem, Math.max(1, Math.floor(0 / MIN_LARGURA_PAINEL)))
})

test('visíveis: cabe tudo devolve cópia; janela contígua segue o foco e mantém a ordem', () => {
  const e = escopo()
  const estado = quadro(5, e)
  const ids = estado.paineis.map(p => p.id)

  const tudo = paineisVisiveis(estado.paineis, estado.focoId, 5)
  assert.deepEqual(tudo.map(p => p.id), ids)
  assert.notEqual(tudo, estado.paineis)

  const curto = paineisVisiveis(estado.paineis.slice(0, 2), null, 3)
  assert.equal(curto.length, 2)

  const focoNoFim = paineisVisiveis(estado.paineis, ids[4], 3)
  assert.deepEqual(focoNoFim.map(p => p.id), [ids[2], ids[3], ids[4]])
  const focoNoMeio = paineisVisiveis(estado.paineis, ids[2], 3)
  assert.deepEqual(focoNoMeio.map(p => p.id), [ids[0], ids[1], ids[2]])
  const focoNoInicio = paineisVisiveis(estado.paineis, ids[0], 3)
  assert.deepEqual(focoNoInicio.map(p => p.id), [ids[0], ids[1], ids[2]])
  const semFoco = paineisVisiveis(estado.paineis, 'p999', 2)
  assert.deepEqual(semFoco.map(p => p.id), [ids[0], ids[1]])
  const zero = paineisVisiveis(estado.paineis, ids[4], 0)
  assert.deepEqual(zero.map(p => p.id), [ids[4]])
})

test('parse: entrada inválida é ignorada; painel ruim cai; passa do máximo não entra; foco inválido vira o primeiro', () => {
  assert.deepEqual(parseLadoALadoMapa(null), {})
  assert.deepEqual(parseLadoALadoMapa('x'), {})
  assert.deepEqual(parseLadoALadoMapa([]), {})
  assert.deepEqual(parseLadoALadoMapa(3), {})
  assert.deepEqual(parseLadoALadoMapa({}), {})
  assert.deepEqual(parseLadoALadoMapa({ a: null, b: 1, c: 'x' }), {})

  const refOk = { computerId: 'comp-1', botId: 'bot-1', spaceId: 'esp-1', hubId: 'hub-1' }
  const mapa = parseLadoALadoMapa({
    pessoal: { ativo: true, focoId: 'p2', paineis: [
      { id: 'p1', ref: refOk, sessionId: null },
      { id: 'p2', ref: { ...refOk, botId: 'bot-2' }, sessionId: 's' },
      { id: 'p3', ref: refOk }, // sessão ausente vale nula
      'x', // cai
      { id: '', ref: refOk }, // sem id cai
      { id: 'p4', ref: { computerId: '', botId: 'b' } }, // sem computador cai
      { id: 'p5', ref: refOk, sessionId: 7 }, // sessão de tipo errado cai
    ] },
    ruim: { ativo: true, focoId: 'p1' },
    vazio: { ativo: true, focoId: null, paineis: [] },
  })
  assert.deepEqual(Object.keys(mapa).sort(), ['pessoal', 'vazio'])
  assert.deepEqual(mapa.pessoal.paineis.map(p => p.id), ['p1', 'p2', 'p3'])
  assert.equal(mapa.pessoal.focoId, 'p2')
  assert.equal(mapa.pessoal.ativo, true)
  assert.equal(mapa.pessoal.paineis[2].sessionId, null)
  assert.deepEqual(mapa.vazio, { ativo: false, focoId: null, paineis: [] })

  const focoRuim = parseLadoALadoMapa({ pessoal: { ativo: true, focoId: 'p999', paineis: [{ id: 'p1', ref: refOk }] } })
  assert.equal(focoRuim.pessoal.focoId, 'p1')
  assert.equal(focoRuim.pessoal.ativo, true)

  const semAtivo = parseLadoALadoMapa({ pessoal: { focoId: 'p1', paineis: [{ id: 'p1', ref: refOk }] } })
  assert.equal(semAtivo.pessoal.ativo, false)

  const estourado = parseLadoALadoMapa({ pessoal: { ativo: true, focoId: 'p1', paineis: [1, 2, 3, 4, 5].map(i => ({ id: `p${i}`, ref: refOk })) } })
  assert.equal(estourado.pessoal.paineis.length, MAX_PAINEIS)
})

test('parse: spaceId ou hubId fornecido com tipo errado rejeita a referência inteira', () => {
  const base = { computerId: 'comp-1', botId: 'bot-1' }
  // Fail-closed: um opcional com tipo errado não pode sumir e virar conversa do escopo pessoal.
  const rejeitados = parseLadoALadoMapa({ pessoal: { ativo: true, focoId: 'p1', paineis: [
    { id: 'p1', ref: { ...base, spaceId: 7 } },
    { id: 'p2', ref: { ...base, hubId: false } },
    { id: 'p3', ref: { ...base, spaceId: '' } },
  ] } })
  assert.deepEqual(rejeitados.pessoal.paineis, [])
  assert.equal(rejeitados.pessoal.ativo, false)
  // Omitido (undefined ou null) segue válido, como antes.
  const boas = parseLadoALadoMapa({ pessoal: { ativo: true, focoId: 'p1', paineis: [
    { id: 'p1', ref: base },
    { id: 'p2', ref: { ...base, spaceId: null, hubId: null } },
  ] } })
  assert.equal(boas.pessoal.paineis.length, 2)
  assert.deepEqual(boas.pessoal.paineis[0].ref, base)
  assert.deepEqual(boas.pessoal.paineis[1].ref, base)
})

test('constantes: máximo de painéis, largura mínima e chave de gravação', () => {
  assert.equal(MAX_PAINEIS, 3)
  assert.equal(MIN_LARGURA_PAINEL, 360)
  assert.equal(CHAVE_ARMAZENAMENTO_LADO_A_LADO, 'fork.ladoALado.v1')
})

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { advanceContiguous, advanceMark, afterChange, countBelow, forgetSessions, isLive, jumpPill, markSaver, newIds, nextVisit, openAt, openMark, openVisit, withArrivals, type Seen } from './leitura.ts'

// D432 (docs/reference/design/barrinha-novas-20261107): o "lido até aqui" deste aparelho e a fonte única das novas.
const conversa: Seen[] = [
  { id: 'u1', at: 1, mine: true, rev: 1 }, { id: 'b1', at: 2, mine: false, rev: 2 }, { id: 'u2', at: 3, mine: true, rev: 3 },
  { id: 'b2', at: 4, mine: false, rev: 4 }, { id: 'b3', at: 5, mine: false, rev: 5 }, { id: 'u3', at: 6, mine: true, rev: 6 },
]
// 12 respostas novas depois da marca 0.
const doze: Seen[] = Array.from({ length: 12 }, (_, i) => ({ id: `b${i + 1}`, at: i + 1, mine: false, rev: i + 1 }))

test('marca ao abrir: a guardada; sem ela, a principal parte do bot.unread e a extra de tudo lido', () => {
  assert.equal(openMark(conversa, 2, true, 9), 2)
  assert.equal(openMark(conversa, undefined, true, 2), 3)
  assert.equal(openMark(conversa, undefined, false, 2), 5)
  assert.equal(openMark(conversa, undefined, true, 0), 5)
})

test('as suas nunca contam; a mesma lista dá a linha, as marcas em accent e o N novas', () => {
  const unseen = newIds(conversa, 1)
  assert.deepEqual([...unseen], ['b1', 'b2', 'b3'])
  assert.equal(conversa.find((m) => unseen.has(m.id))?.id, 'b1')
  // O botão: novas ainda não vistas, abaixo da última à vista.
  assert.equal(countBelow(unseen, new Set(['b1']), conversa, 2), 2)
  assert.equal(countBelow(unseen, new Set(['b1', 'b2', 'b3']), conversa, 5), 0)
})

// R1-01 do #511.
test('pular do topo para o fim pela barrinha não marca o meio como lido', () => {
  const pulou = new Set(['b1', 'b12'])
  const lido = advanceContiguous(0, doze, pulou, false)
  assert.equal(lido, 1)
  assert.equal(newIds(doze, lido).size, 11)
})

test('ler em ordem marca até onde leu; as suas não interrompem a sequência', () => {
  const comPedido: Seen[] = [...doze.slice(0, 3), { id: 'u', at: 3.5, mine: true, rev: 99 }, ...doze.slice(3)]
  assert.equal(advanceContiguous(0, comPedido, new Set(['b1', 'b2', 'b3', 'b4']), false), 4)
  assert.equal(advanceContiguous(0, doze, new Set(doze.map((m) => m.id)), false), 12)
})

test('com histórico por carregar entre a marca e o carregado, a marca não avança', () => {
  assert.equal(advanceContiguous(0, doze.slice(6), new Set(doze.map((m) => m.id)), true), 0)
})

// R1-04 e R2-01 do #511: carga e catch-up pela revisão do stream, nunca pelo relógio. O Hub abre o stream no rev 50
// (hello); o cache tem o seu pedido; o catch-up traz 12 respostas (rev até 50); depois chega uma ao vivo (rev 51).
const HELLO = 50
const abertura = 1_000_000
for (const [nome, desvio] of [['+2 s', 2_000], ['−2 s', -2_000], ['+2 min', 120_000], ['−2 min', -120_000]] as const) {
  test(`relógio do Hub ${nome}: catch-up é carga, ao vivo no fim não vira nova nem faz a tela pular`, () => {
    // createdAt do Hub com o desvio: as respostas do catch-up ficam perto da abertura, a ao vivo logo depois.
    const cache: Seen[] = [{ id: 'u', at: abertura - 60_000 + desvio, mine: true, rev: 10 }]
    const catchUp: Seen[] = Array.from({ length: 12 }, (_, i) => ({ id: `c${i}`, at: abertura - 1_000 + i * 100 + desvio, mine: false, rev: 38 + i }))
    const aoVivo: Seen = { id: 'v', at: abertura + 500 + desvio, mine: false, rev: HELLO + 1 }
    // Abre com o cache antes do hello (streamRev null) e sem marca.
    let visita = openVisit(cache, undefined, true, 0)
    assert.equal(visita.unseen.size, 0)
    // O catch-up chega com você no fim: é carga, as 12 são novas e a linha aparece na 1ª.
    visita = nextVisit(visita, [...cache, ...catchUp], true, HELLO)
    assert.equal(visita.unseen.size, 12)
    assert.equal(visita.line, 'c0')
    // Uma resposta ao vivo com você no fim: não vira nova e a linha não muda (a conversa não pula para ela).
    const depois = nextVisit(visita, [...cache, ...catchUp, aoVivo], true, HELLO)
    assert.equal(depois.unseen.has('v'), false)
    assert.equal(depois.line, 'c0')
    // Sem catch-up nenhum (tudo lido), a ao vivo no fim não cria linha: nada para a tela pular.
    const lida = nextVisit(openVisit([...cache, ...catchUp], abertura + 400 + desvio, true, 0), [...cache, ...catchUp, aoVivo], true, HELLO)
    assert.deepEqual([lida.unseen.size, lida.line], [0, null])
    // Fora do fim, a ao vivo é nova (a pílula conta).
    assert.equal(nextVisit(visita, [...cache, ...catchUp, aoVivo], false, HELLO).unseen.has('v'), true)
  })
}

test('antes do 1º hello tudo é carga; depois, só rev acima do hello é ao vivo', () => {
  assert.equal(isLive({ rev: 999 }, null), false)
  assert.equal(isLive({ rev: 50 }, 50), false)
  assert.equal(isLive({ rev: 51 }, 50), true)
  const chegou: Seen[] = [{ id: 'x', at: 10, mine: false, rev: 51 }, { id: 'y', at: 11, mine: true, rev: 52 }]
  assert.deepEqual([...withArrivals(new Set(), chegou, 0, false, 50)], ['x'])
})

// R1-02 do #511.
test('a marca é gravada durante a visita, no máximo uma vez por intervalo, e já ao sair', () => {
  const gravadas: number[] = []
  let agendado: (() => void) | null = null
  const saver = markSaver((at) => gravadas.push(at), 1000, { set: (fn) => { agendado = fn; return 1 }, clear: () => { agendado = null } })
  saver.push(3)
  saver.push(5)
  assert.deepEqual(gravadas, [])
  agendado!()
  assert.deepEqual(gravadas, [5])
  saver.push(7)
  saver.flush()
  assert.deepEqual(gravadas, [5, 7])
  saver.flush()
  assert.deepEqual(gravadas, [5, 7])
})

test('a marca só avança e vale na próxima abertura', () => {
  assert.deepEqual(advanceMark({ 'ana:-': 3 }, 'ana:-', 5), { 'ana:-': 5 })
  assert.equal(advanceMark({ 'ana:-': 5 }, 'ana:-', 4), null)
  const depois = advanceMark({}, 'ana:-', 5)!
  assert.equal(newIds(conversa, openMark(conversa, depois['ana:-'], true, 3)).size, 0)
})

test('o mapa de marcas tem teto e esquece sessões que não estão mais abertas', () => {
  const cheio = advanceMark({ 'a:-': 1, 'b:-': 2, 'c:-': 3 }, 'd:-', 4, 3)!
  assert.deepEqual(Object.keys(cheio).sort(), ['b:-', 'c:-', 'd:-'])
  const marcas = { 'ana:-': 1, 'ana:s1': 2, 'ana:s2': 3, 'bia:s1': 4 }
  assert.deepEqual(forgetSessions(marcas, 'ana', new Set(['s1'])), { 'ana:-': 1, 'ana:s1': 2, 'bia:s1': 4 })
  assert.equal(forgetSessions(marcas, 'ana', new Set(['s1', 's2'])), null)
})

test('abrir na linha quando as novas não cabem; senão no fim', () => {
  assert.equal(openAt(1200, 700), 'line')
  assert.equal(openAt(500, 700), 'end')
})

test('pílula só fora do fim; com mensagem nova lá em cima, o chat não pula', () => {
  assert.deepEqual(jumpPill(true, true, 3), { shown: false, count: 0 })
  assert.deepEqual(jumpPill(false, true, 3), { shown: true, count: 3 })
  assert.deepEqual(jumpPill(false, false, 0), { shown: false, count: 0 })
  assert.equal(afterChange(false, false), 'stay')
  assert.equal(afterChange(true, false), 'bottom')
  assert.equal(afterChange(false, true), 'restore')
})

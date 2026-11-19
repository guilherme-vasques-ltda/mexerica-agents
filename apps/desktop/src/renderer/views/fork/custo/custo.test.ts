// DON-908 (C8): o modelo do custo estimado (validação, formato, ordem) e os textos do balão e da página.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { CATALOGOS, traduzir } from '../../../lib/fork-i18n/index.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'
import { codigoFalha, corpoDo, daMesmaLeitura, dinheiro, lerRelatorio, mesVizinho, ordenar, parcial, partes, tituloVazio, type Totais } from './modelo.ts'
import { parcialDe, tokensDe, valorDe } from './textos.ts'

const totais = (o: Partial<Totais> = {}): Totais => ({ turns: 3, unpricedTurns: 0, inputTokens: 1000, cachedReadTokens: 80_000, cachedWriteTokens: null, outputTokens: 500, costs: [], ...o })
const conversa = (o: Record<string, unknown> = {}) => ({ botId: 'b1', chatId: 'b1', sessionId: 'b1', title: '', parentSessionId: null, totals: totais(), ...o })
const resposta = (o: Record<string, unknown> = {}) => ({
  hubId: 'hub', measuredAt: 1, month: '2026-11', total: totais(), bots: [{ botId: 'b1', name: 'Donna', totals: totais() }],
  conversations: [conversa()], helpers: [], conversation: null, ...o,
})

test('o pedido: principal sem sessionId, extra exata, mês só quando escolhido', () => {
  assert.deepEqual(corpoDo({ botId: 'b1', sessionId: null }), { botId: 'b1' })
  assert.deepEqual(corpoDo({ botId: 'b1', sessionId: 'b1' }), { botId: 'b1', sessionId: 'b1' })
  assert.deepEqual(corpoDo({}), {})
  assert.deepEqual(corpoDo({ month: '2026-10' }), { month: '2026-10' })
})

test('a resposta só vale do mesmo Hub, mês e conversa/sessão', () => {
  assert.equal(lerRelatorio(resposta(), {}, 'hub').month, '2026-11')
  assert.throws(() => lerRelatorio(resposta(), {}, 'outro'), /cost_invalid_answer/)
  assert.throws(() => lerRelatorio(resposta(), { month: '2026-10' }, 'hub'), /cost_invalid_answer/)
  // Principal: volta com sessionId = chatId; uma extra no lugar da principal (ou vice-versa) é recusada.
  assert.ok(lerRelatorio(resposta({ conversation: conversa() }), { botId: 'b1', sessionId: null }, 'hub').conversation)
  assert.throws(() => lerRelatorio(resposta({ conversation: conversa({ sessionId: 's2' }) }), { botId: 'b1', sessionId: null }, 'hub'))
  assert.throws(() => lerRelatorio(resposta({ conversation: conversa() }), { botId: 'b1', sessionId: 's2' }, 'hub'))
  assert.throws(() => lerRelatorio(resposta({ conversation: conversa({ botId: 'b2' }) }), { botId: 'b1', sessionId: null }, 'hub'))
  // Principal de um Space do mesmo bot (outro chatId, sessionId = chatId) não passa pela conversa pessoal.
  assert.throws(() => lerRelatorio(resposta({ conversation: conversa({ chatId: 'space-1', sessionId: 'space-1' }) }), { botId: 'b1', sessionId: null }, 'hub'), /cost_invalid_answer/)
  assert.throws(() => lerRelatorio(resposta({ conversation: conversa({ chatId: 'space-1', sessionId: 's2' }) }), { botId: 'b1', sessionId: 's2' }, 'hub'), /cost_invalid_answer/)
  assert.equal(lerRelatorio(resposta({ conversation: conversa({ sessionId: 's2' }) }), { botId: 'b1', sessionId: 's2' }, 'hub').conversation?.sessionId, 's2')
})

test('campos fora do contrato recusam tudo', () => {
  const ruim = [
    { total: totais({ costs: [{ amount: 1, currency: 'USD', source: 'motor' }, { amount: 2, currency: 'USD', source: 'motor' }] }) },
    { total: totais({ costs: [{ amount: -1, currency: 'USD', source: 'motor' }] }) },
    { total: totais({ costs: [{ amount: 1, currency: 'usd', source: 'motor' }] }) },
    { total: totais({ unpricedTurns: 4 }) },
    { total: { ...totais(), inputTokens: 1.5 } },
    { helpers: [conversa({ sessionId: 's2' })] },
    { month: '2026-13' },
    { bots: null },
  ]
  for (const o of ruim) assert.throws(() => lerRelatorio(resposta(o), {}, 'hub'), /cost_invalid_answer/, JSON.stringify(o))
  // Zero explícito do motor é legítimo; token null é sem dado.
  assert.equal(lerRelatorio(resposta({ total: totais({ costs: [{ amount: 0, currency: 'USD', source: 'motor' }], inputTokens: null }) }), {}, 'hub').total.costs[0]?.amount, 0)
})

test('dinheiro: cada moeda no formato do idioma, sem converter nem somar; vazio não vira zero', () => {
  assert.equal(dinheiro('pt-BR', []), null)
  assert.equal(dinheiro('en', [{ amount: 1.5, currency: 'USD', source: 'motor' }]), '$1.50')
  assert.equal(dinheiro('en', [{ amount: 0.4, currency: 'BRL', source: 'motor' }, { amount: 1.5, currency: 'USD', source: 'motor' }]), 'R$0.40 + $1.50')
  assert.equal(dinheiro('en', [{ amount: 0, currency: 'USD', source: 'motor' }]), '$0.00')
})

test('parcial e partes: nunca um total de tokens inventado', () => {
  assert.equal(parcial(totais({ unpricedTurns: 1 })), false)
  assert.equal(parcial(totais({ unpricedTurns: 1, costs: [{ amount: 1, currency: 'USD', source: 'motor' }] })), true)
  assert.deepEqual(partes(totais()).map((p) => p.parte), ['entrada', 'cacheLido', 'saida'])
})

test('ordem: com dinheiro antes, mesma moeda pelo valor, sem dinheiro pelos tokens', () => {
  const usd = (amount: number) => [{ amount, currency: 'USD', source: 'motor' }]
  const linhas = [
    { n: 'a', totals: totais({ inputTokens: 9_000_000 }) },
    { n: 'b', totals: totais({ costs: usd(1) }) },
    { n: 'c', totals: totais({ costs: usd(3) }) },
    { n: 'd', totals: totais({ costs: [{ amount: 50, currency: 'BRL', source: 'motor' }] }) },
    { n: 'e', totals: totais({ inputTokens: 1 }) },
  ]
  assert.deepEqual(ordenar(linhas, (l) => l.n).map((l) => l.n), ['d', 'c', 'b', 'a', 'e'])
})

test('meses, títulos vazios e falhas', () => {
  assert.equal(mesVizinho('2026-01', -1), '2025-12')
  assert.equal(mesVizinho('2026-12', 1), '2027-01')
  assert.equal(tituloVazio(conversa()), 'principal')
  assert.equal(tituloVazio(conversa({ sessionId: 's2' })), 'semTitulo')
  assert.equal(codigoFalha({ status: 404 }), 'antigo')
  assert.equal(codigoFalha(new Error('unknown method donnaCostReport')), 'antigo')
  assert.equal(codigoFalha({ status: 403, message: 'x' }), 'soDono')
  assert.equal(codigoFalha(new Error('cost_invalid_answer')), 'invalida')
  assert.equal(codigoFalha(new Error('boom')), 'falha')
})

test('textos: nota literal, sem preço mostra só tokens, parcial sinalizado', () => {
  const nota = 'Estimativa do equivalente em API. Na assinatura você não paga por token; o que conta é o limite da conta.'
  assert.equal(traduzir('pt-BR', 'contexto.custo.nota'), nota)
  assert.equal(traduzir('pt-BR', 'conversa.custo.nota'), nota)
  for (const cat of Object.values(CATALOGOS)) assert.ok(cat['contexto.custo.nota'] && cat['conversa.custo.nota'])
  // O mesmo `t` do useT, sem a preferência do React (estado.ts lê prefs, fora do Node).
  const traduzirSolto = traduzir as (idioma: 'pt-BR', chave: string, params?: Record<string, string | number>) => string
  const t = ((k: string, p?: Record<string, string | number>) => traduzirSolto('pt-BR', k, p)) as unknown as Tradutor
  assert.equal(valorDe(t, 'pt-BR', totais()), 'Sem preço')
  assert.equal(traduzir('pt-BR', 'contexto.custo.titulo'), 'Custo estimado desta conversa')
  assert.ok(!('contexto.custo.semPreco' in CATALOGOS['pt-BR']))
  assert.equal(tokensDe(t, 'pt-BR', 'contexto.custo', totais()).replace(/\s/g, ' '), 'Entrada 1 mil · Cache lido 80 mil · Saída 500')
  assert.equal(tokensDe(t, 'pt-BR', 'conversa.custo', totais({ inputTokens: null, cachedReadTokens: null, outputTokens: null })), 'Sem tokens registrados')
  assert.equal(parcialDe(t, 'pt-BR', 'conversa.custo', totais({ turns: 9, unpricedTurns: 2, costs: [{ amount: 1, currency: 'USD', source: 'motor' }] })), 'Parcial: 2 de 9 turnos sem preço')
  assert.equal(parcialDe(t, 'pt-BR', 'conversa.custo', totais({ unpricedTurns: 2 })), null)
})

test('identidade: leitura de outra sessão, Hub, principal ou client (reconexão) não aparece', () => {
  const c1 = {}, c2 = {}
  const chave = (hub: string, epoch: number, pedido: Parameters<typeof corpoDo>[0]) => JSON.stringify([hub, epoch, JSON.stringify(corpoDo(pedido))])
  const principal = chave('hub', 1, { botId: 'b1', sessionId: null })
  const g = { chave: principal, client: c1, valor: 'custo da principal' }
  assert.equal(daMesmaLeitura(g, principal, c1), 'custo da principal')
  assert.equal(daMesmaLeitura(g, chave('hub', 1, { botId: 'b1', sessionId: 's2' }), c1), null)
  assert.equal(daMesmaLeitura(g, chave('outro', 1, { botId: 'b1', sessionId: null }), c1), null)
  assert.equal(daMesmaLeitura(g, chave('hub', 2, { botId: 'b1', sessionId: null }), c1), null)
  assert.equal(daMesmaLeitura(g, principal, c2), null)
  assert.equal(daMesmaLeitura(g, principal, null), null)
  assert.equal(daMesmaLeitura(null, principal, c1), null)
})


test('estimativas aceitam Hub antigo e recusam valores inválidos', () => {
  assert.deepEqual(lerRelatorio(resposta(), {}, 'hub').total.estimatedCosts, [])
  const estimatedCosts = [{ amount: 0, currency: 'USD', source: 'estimado' }]
  assert.deepEqual(lerRelatorio(resposta({ total: totais({ estimatedCosts }) }), {}, 'hub').total.estimatedCosts, estimatedCosts)
  assert.throws(() => lerRelatorio(resposta({ total: totais({ estimatedCosts: [...estimatedCosts, ...estimatedCosts] }) }), {}, 'hub'), /cost_invalid_answer/)
  assert.throws(() => lerRelatorio(resposta({ total: totais({ estimatedCosts: [{ ...estimatedCosts[0]!, source: 'motor' }] }) }), {}, 'hub'), /cost_invalid_answer/)
  assert.equal(parcial(totais({ estimatedCosts, unpricedTurns: 1 })), true)
})

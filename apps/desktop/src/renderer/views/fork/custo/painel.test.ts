// MX-145 E2: o painel de uso (abas Por conta, Por modelo e Por bot): validação de `accounts`/`models`, Hub antigo,
// rótulos de conta, limites do plano e textos (parcial, sem preço, nulos, moedas separadas).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { CATALOGOS, traduzir } from '../../../lib/fork-i18n/index.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'
import { abaDoUso, chaveDoCadastro, daMesmaLeitura, dinheiro, idCurto, lerCadastro, lerRelatorio, parcial, rotuloDaConta, type ContaCadastro, type Totais } from './modelo.ts'
import { parcialDe, tokensDe, valorDe } from './textos.ts'

const usd = (amount: number) => [{ amount, currency: 'USD', source: 'motor' }]
const totais = (o: Partial<Totais> = {}): Totais => ({ turns: 4, unpricedTurns: 0, inputTokens: 1000, cachedReadTokens: null, cachedWriteTokens: null, outputTokens: 500, costs: [], ...o })
const resposta = (o: Record<string, unknown> = {}) => ({
  hubId: 'hub', measuredAt: 1, month: '2026-11', total: totais({ turns: 10 }), bots: [], conversations: [], helpers: [], conversation: null,
  accounts: [{ accountId: 'acc-1', totals: totais({ costs: usd(2) }) }, { accountId: null, totals: totais({ turns: 6 }) }],
  models: [{ model: 'claude-opus-5-5', totals: totais() }, { model: null, totals: totais({ turns: 6 }) }],
  ...o,
})
const t = ((k: string, p?: Record<string, string | number>) => (traduzir as (i: 'pt-BR', c: string, p?: Record<string, string | number>) => string)('pt-BR', k, p)) as unknown as Tradutor

test('accounts e models válidos entram na ordem do Hub, com null preservado', () => {
  const r = lerRelatorio(resposta(), {}, 'hub')
  assert.deepEqual(r.contas?.map((c) => c.accountId), ['acc-1', null])
  assert.deepEqual(r.modelos?.map((m) => m.model), ['claude-opus-5-5', null])
})

test('Hub antigo (sem accounts e models) degrada para null, sem inventar grupos', () => {
  const { accounts: _a, models: _m, ...antiga } = resposta()
  const r = lerRelatorio(antiga, {}, 'hub')
  assert.equal(r.contas, null)
  assert.equal(r.modelos, null)
  assert.equal(r.total.turns, 10)
})

test('accounts/models fora do contrato recusam tudo (INVALIDA)', () => {
  const { accounts: _a, ...soModels } = resposta()
  const { models: _m, ...soAccounts } = resposta()
  // Só um dos dois: nenhum Hub assim.
  for (const raw of [soModels, soAccounts]) assert.throws(() => lerRelatorio(raw, {}, 'hub'), /cost_invalid_answer/)
  const ruins: Record<string, unknown>[] = [
    { accounts: null },
    { models: 'x' },
    { accounts: [{ accountId: '', totals: totais() }] },
    { accounts: [{ totals: totais() }] }, // accountId ausente (null é explícito)
    { accounts: [{ accountId: 'a', totals: totais() }, { accountId: 'a', totals: totais() }] }, // grupo repetido
    { accounts: [{ accountId: null, totals: totais() }, { accountId: null, totals: totais() }] },
    { models: [{ model: 'm', totals: totais() }, { model: 'm', totals: totais() }] },
    { models: [{ model: 3, totals: totais() }] },
    { models: [{ model: 'm', totals: totais({ turns: 11 }) }] }, // mais turnos que o mês
    { accounts: [], models: [] }, // 10 turnos no mês e nenhum grupo
    { accounts: [{ accountId: 'a', totals: totais({ turns: 4 }) }] }, // os grupos não cobrem o mês (4 de 10)
    { models: [{ model: 'm', totals: totais({ turns: 4 }) }, { model: null, totals: totais({ turns: 7 }) }] }, // passam do mês (11)
    { accounts: [{ accountId: 'a', totals: totais({ costs: [...usd(1), ...usd(2)] }) }] }, // moeda repetida
    { accounts: [{ accountId: 'a', totals: totais({ unpricedTurns: 5 }) }] },
  ]
  for (const o of ruins) assert.throws(() => lerRelatorio(resposta(o), {}, 'hub'), /cost_invalid_answer/, JSON.stringify(o))
})

test('cadastro de contas: formato estrito; falha não vira lista vazia', () => {
  const ok = lerCadastro({ accounts: [{ id: 'acc-1', label: 'Claude', backend: 'claude', billing: 'subscription', state: 'connected', models: [] }] })
  assert.deepEqual(ok, [{ id: 'acc-1', label: 'Claude', backend: 'claude', billing: 'subscription' }])
  // Uma conta Grok no cadastro não derruba a leitura das outras.
  const comGrok = lerCadastro({ accounts: [{ id: 'acc-1', label: 'Claude', backend: 'claude', billing: 'subscription' }, { id: 'acc-2', label: 'Grok', backend: 'grok', billing: 'subscription' }] })
  assert.deepEqual(comGrok.map((c) => c.backend), ['claude', 'grok'])
  for (const raw of [null, {}, { accounts: [{ id: 'a', label: 'x', backend: 'gemini', billing: 'paid_api' }] }, { accounts: [{ id: '', label: 'x', backend: 'codex', billing: 'paid_api' }] }, { accounts: [{ id: 'a', label: 'x', backend: 'codex', billing: 'free' }] }]) {
    assert.throws(() => lerCadastro(raw), /cost_invalid_answer/, JSON.stringify(raw))
  }
})

const cadastro: ContaCadastro[] = [
  { id: 'acc-claude', label: 'Claude', backend: 'claude', billing: 'subscription' },
  { id: 'acc-api', label: 'OpenAI API', backend: 'codex', billing: 'paid_api' },
  { id: 'acc-sem-prova', label: 'Outra', backend: 'codex', billing: 'unverified' },
  { id: 'acc-vazia', label: '  ', backend: 'codex', billing: 'subscription' },
]

test('rótulo da conta: nome e tipo do cadastro; sem cadastro, o ID curto; null é não identificada', () => {
  assert.deepEqual(rotuloDaConta(null, cadastro), { kind: 'naoIdentificada' })
  assert.deepEqual(rotuloDaConta(null, null), { kind: 'naoIdentificada' })
  assert.deepEqual(rotuloDaConta('acc-claude', cadastro), { kind: 'nome', label: 'Claude', tipo: 'assinatura', backend: 'claude' })
  assert.deepEqual(rotuloDaConta('acc-api', cadastro), { kind: 'nome', label: 'OpenAI API', tipo: 'chave', backend: 'codex' })
  // `unverified` não afirma tipo.
  assert.deepEqual(rotuloDaConta('acc-sem-prova', cadastro), { kind: 'nome', label: 'Outra', tipo: null, backend: 'codex' })
  // Cadastro que falhou (null), conta desconhecida ou sem nome: ID curto; os números continuam.
  assert.deepEqual(rotuloDaConta('0123456789abcdef', null), { kind: 'curto', id: '01234567' })
  assert.deepEqual(rotuloDaConta('desconhecida-xyz', cadastro), { kind: 'curto', id: 'desconhe' })
  assert.deepEqual(rotuloDaConta('acc-vazia', cadastro), { kind: 'curto', id: 'acc-vazi' })
  assert.equal(idCurto('abc'), 'abc')
})

test('abas: desconhecida ou ausente cai em Por conta', () => {
  assert.equal(abaDoUso(null), 'conta')
  assert.equal(abaDoUso('teto'), 'conta')
  assert.equal(abaDoUso('modelo'), 'modelo')
  assert.equal(abaDoUso('bot'), 'bot')
})

test('sem preço nunca vira zero; parcial só com dinheiro e turnos de fora; moedas separadas', () => {
  assert.equal(valorDe(t, 'pt-BR', totais()), 'Sem preço')
  assert.equal(valorDe(t, 'pt-BR', totais({ costs: [{ amount: 0, currency: 'USD', source: 'motor' }] })).includes('0,00'), true) // zero do motor é zero explícito
  const misto = totais({ unpricedTurns: 2, costs: usd(5) })
  assert.equal(parcial(misto), true)
  assert.equal(parcialDe(t, 'pt-BR', 'conversa.custo', misto), 'Parcial: 2 de 4 turnos sem preço')
  assert.equal(parcialDe(t, 'pt-BR', 'conversa.custo', totais({ unpricedTurns: 4 })), null) // tudo sem preço: "Sem preço", não parcial
  assert.equal(dinheiro('en', [...usd(1.5), { amount: 3, currency: 'BRL', source: 'motor' }]), 'R$3.00 + $1.50')
  assert.equal(tokensDe(t, 'pt-BR', 'conversa.custo', totais({ inputTokens: null, outputTokens: null })), 'Sem tokens registrados')
})

test('Hub com mês vazio e grupos vazios é válido (nenhum turno, nenhum grupo)', () => {
  const vazio = resposta({ total: totais({ turns: 0, unpricedTurns: 0, inputTokens: null, outputTokens: null }), accounts: [], models: [] })
  const r = lerRelatorio(vazio, {}, 'hub')
  assert.deepEqual([r.contas, r.modelos], [[], []])
})

test('cadastro de contas: vale só para o mesmo Hub, época e client; reconexão com o mesmo objeto não o ressuscita', () => {
  const c1 = {}, c2 = {}
  const guardado = { chave: chaveDoCadastro('hub', 3), client: c1, valor: 'cadastro velho' }
  assert.equal(daMesmaLeitura(guardado, chaveDoCadastro('hub', 3), c1), 'cadastro velho')
  // Resync no mesmo client: outra época da principal, outro cadastro.
  assert.equal(daMesmaLeitura(guardado, chaveDoCadastro('hub', 4), c1), null)
  assert.equal(daMesmaLeitura(guardado, chaveDoCadastro('outro', 3), c1), null)
  assert.equal(daMesmaLeitura(guardado, chaveDoCadastro('hub', 3), c2), null)
  // Sem conexão (client null) nada vale; o hook também esquece o guardado, e `null` não casa com o objeto de volta.
  assert.equal(daMesmaLeitura(guardado, chaveDoCadastro('hub', 3), null), null)
  assert.notEqual(chaveDoCadastro(null, 3), chaveDoCadastro('hub', 3))
})

test('textos novos do painel existem nos seis idiomas e o rótulo de aba está no catálogo', () => {
  const chaves = ['conversa.custo.parcialCurto', 'ajustes.uso.usado', 'conversa.custo.limites', 'conversa.custo.limitesNota', 'conversa.custo.resumo', 'conversa.custo.contasIa', 'conversa.custo.porModelo', 'conversa.custo.contaNaoIdentificada', 'conversa.custo.modeloNaoIdentificado', 'conversa.custo.contaCurta', 'conversa.custo.tipo.assinatura', 'conversa.custo.tipo.chave', 'ajustes.aba.uso.conta', 'ajustes.aba.uso.modelo', 'ajustes.aba.uso.bot']
  for (const [idioma, cat] of Object.entries(CATALOGOS)) for (const k of chaves) assert.ok((cat as Record<string, string>)[k], `${idioma}: ${k}`)
  assert.equal(traduzir('pt-BR', 'conversa.custo.contaNaoIdentificada'), 'Conta não identificada')
  assert.equal(traduzir('pt-BR', 'conversa.custo.modeloNaoIdentificado'), 'Modelo não identificado')
  // O seletor abre meses passados: "Resumo do mês" em chinês é "月度概览", não "本月" (este mês).
  assert.equal(traduzir('zh-CN', 'conversa.custo.resumo'), '月度概览')
})

test('zh-CN: as chaves novas de conta usam 账号 (como o catálogo de Contas de IA), nunca 账户', () => {
  const chaves = ['conversa.custo.contasIa', 'conversa.custo.contaNaoIdentificada', 'conversa.custo.contaCurta', 'conversa.custo.antigoGrupos', 'conversa.custo.vazioDica', 'conversa.custo.contasSemNomes', 'ajustes.aba.uso.conta']
  for (const k of chaves) {
    const texto = (traduzir as (i: 'zh-CN', c: string) => string)('zh-CN', k)
    assert.doesNotMatch(texto, /账户/, k)
    assert.match(texto, /账号/, k)
  }
  // A seção da tela 01 se chama "Contas de IA" nos seis idiomas (a aba é que se chama "Por conta").
  assert.equal(traduzir('pt-BR', 'conversa.custo.contasIa'), 'Contas de IA')
  assert.equal(traduzir('pt-BR', 'ajustes.aba.uso.conta'), 'Por conta')
})

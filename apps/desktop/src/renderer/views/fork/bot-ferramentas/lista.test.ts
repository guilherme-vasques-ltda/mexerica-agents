import { test } from 'node:test'
import assert from 'node:assert/strict'
import { alternar, filtrar, indexar, navegacaoInicial, navegar, numerosDaPaginacao, paginar, POR_PAGINA, type AcaoDeNavegacao } from './lista.ts'

const itens = Array.from({ length: 23 }, (_, i) => ({ id: `c${i + 1}`, name: `Conector ${i + 1}`, detail: i % 2 ? 'par' : 'ímpar' }))
const contas = [
  { id: 'conta-stripe-a', name: 'Stripe · Loja A', detail: 'Pagamentos e assinaturas' },
  { id: 'conta-stripe-b', name: 'Stripe · Loja B', detail: 'Pagamentos e assinaturas' },
  { id: 'conta-gcal-p', name: 'Google Calendar · Pessoal', detail: 'Agenda e convites' },
]

test('MX-84: 15 por página; a segunda página traz o resto e a faixa certa', () => {
  assert.equal(POR_PAGINA, 15)
  const p1 = paginar(itens, 1)
  assert.deepEqual([p1.itens.length, p1.pagina, p1.paginas, p1.de, p1.ate, p1.total], [15, 1, 2, 1, 15, 23])
  const p2 = paginar(itens, 2)
  assert.deepEqual([p2.itens.length, p2.itens[0]!.id, p2.de, p2.ate], [8, 'c16', 16, 23])
})

test('MX-84: até 15 itens é uma página só (a paginação não aparece)', () => {
  assert.equal(paginar(itens.slice(0, 15), 1).paginas, 1)
  assert.equal(paginar(itens.slice(0, 16), 1).paginas, 2)
  assert.deepEqual(paginar([], 1), { itens: [], pagina: 1, paginas: 1, de: 0, ate: 0, total: 0 })
})

test('MX-84: página fora do que existe fica presa à última (ou à primeira)', () => {
  assert.equal(paginar(itens, 9).pagina, 2)
  assert.equal(paginar(itens, 0).pagina, 1)
  assert.equal(paginar(itens, Number.NaN).pagina, 1)
})

test('MX-84: a busca filtra o conjunto todo, não só a página', () => {
  const achados = filtrar(indexar(itens), 'conector 2')
  // Cada palavra da busca começa uma palavra do item: "2" acha 2 e 20 a 23, não o 12.
  assert.deepEqual(achados.map((i) => i.id), ['c2', 'c20', 'c21', 'c22', 'c23'])
  assert.equal(filtrar(indexar(itens), '   ').length, 23)
  assert.equal(filtrar(indexar(itens), 'nada disso').length, 0)
})

test('MX-84: acha pelo nome do conector, pelo rótulo da conta e pelo detalhe, sem acento nem caixa', () => {
  const indice = indexar(contas)
  assert.deepEqual(filtrar(indice, 'stripe').map((i) => i.id), ['conta-stripe-a', 'conta-stripe-b'])
  assert.deepEqual(filtrar(indice, 'Loja B').map((i) => i.id), ['conta-stripe-b'])
  assert.deepEqual(filtrar(indice, 'AGENDA').map((i) => i.id), ['conta-gcal-p'])
  assert.deepEqual(filtrar(indexar(itens), 'impar').length, 12)
})

test('MX-84 (condição da frente Navegador): ligar na página 2, voltar à 1 e desligar outro não mexe no da página 2', () => {
  let selecao = ['c1', 'c3']
  assert.ok(paginar(itens, 2).itens.some((i) => i.id === 'c20'))
  selecao = alternar(selecao, 'c20', true)
  assert.ok(!paginar(itens, 1).itens.some((i) => i.id === 'c20'))
  selecao = alternar(selecao, 'c3', false)
  assert.deepEqual(selecao, ['c1', 'c20'])
})

test('MX-84 (condição da frente Navegador): com busca ativa, o que o filtro esconde continua ligado', () => {
  let selecao = ['c1', 'c20', 'fora-da-lista']
  const visiveis = filtrar(indexar(itens), 'conector 5').map((i) => i.id)
  assert.deepEqual(visiveis, ['c5'])
  selecao = alternar(selecao, 'c5', true)
  selecao = alternar(selecao, 'c5', true)
  assert.deepEqual(selecao, ['c1', 'c20', 'fora-da-lista', 'c5'])
  selecao = alternar(selecao, 'c5', false)
  // Nem o item escondido pelo filtro nem um id que a lista deste computador não mostra saem da seleção.
  assert.deepEqual(selecao, ['c1', 'c20', 'fora-da-lista'])
})

test('MX-84: "Loja B" acha só a segunda conta, mesmo com um "b" solto na descrição da primeira', () => {
  const duas = indexar([
    { id: 'a', name: 'Pagamentos · Loja A', detail: 'Cobranças e assinaturas' },
    { id: 'b', name: 'Pagamentos · Loja B', detail: 'Cobranças e assinaturas' },
  ])
  assert.deepEqual(filtrar(duas, 'Loja B').map((i) => i.id), ['b'])
  assert.deepEqual(filtrar(duas, 'loja  b').map((i) => i.id), ['b'])
  assert.deepEqual(filtrar(duas, 'b loja').map((i) => i.id), ['b'])
  assert.deepEqual(filtrar(duas, 'pagamentos').map((i) => i.id), ['a', 'b'])
  assert.deepEqual(filtrar(duas, 'pag loj').map((i) => i.id), ['a', 'b'])
  // Um pedaço do meio da palavra ainda acha, quando é o texto inteiro da busca.
  assert.deepEqual(filtrar(duas, 'branca').map((i) => i.id), ['a', 'b'])
  assert.deepEqual(filtrar(duas, ' · ').length, 2)
})

test('MX-95: trocar de aba e buscar voltam à página 1; a busca de cada aba fica nela', () => {
  let nav = navegar(navegacaoInicial(), { tipo: 'pagina', pagina: 2 })
  assert.equal(nav.pagina.connectors, 2)
  nav = navegar(nav, { tipo: 'aba', aba: 'skills' })
  nav = navegar(nav, { tipo: 'pagina', pagina: 3 })
  nav = navegar(nav, { tipo: 'aba', aba: 'connectors' })
  assert.equal(nav.pagina.connectors, 1, 'voltar para a aba começa da página 1')
  nav = navegar(nav, { tipo: 'pagina', pagina: 2 })
  nav = navegar(nav, { tipo: 'busca', texto: 'loja' })
  assert.deepEqual([nav.busca.connectors, nav.pagina.connectors], ['loja', 1])
  nav = navegar(nav, { tipo: 'aba', aba: 'logins' })
  assert.deepEqual([nav.aba, nav.busca.logins, nav.busca.connectors], ['logins', '', 'loja'])
})

test('MX-95: o estado da navegação só tem aba, busca e página (nenhuma seleção nem permissão)', () => {
  const acoes: AcaoDeNavegacao[] = [{ tipo: 'aba', aba: 'logins' }, { tipo: 'busca', texto: 'x' }, { tipo: 'pagina', pagina: 4 }, { tipo: 'aba', aba: 'skills' }, { tipo: 'busca', texto: '' }]
  let nav = navegacaoInicial()
  for (const a of acoes) {
    const antes = structuredClone(nav)
    const depois = navegar(nav, a)
    assert.deepEqual(nav, antes, 'não muda o estado anterior')
    assert.deepEqual(Object.keys(depois).sort(), ['aba', 'busca', 'pagina'])
    nav = depois
  }
})

test('MX-95: números do rodapé — até 7 páginas todas; depois primeira, última e vizinhas', () => {
  assert.deepEqual(numerosDaPaginacao(1, 1), [1])
  assert.deepEqual(numerosDaPaginacao(2, 3), [1, 2, 3])
  assert.deepEqual(numerosDaPaginacao(1, 10), [1, 2, null, 10])
  assert.deepEqual(numerosDaPaginacao(5, 10), [1, null, 4, 5, 6, null, 10])
  assert.deepEqual(numerosDaPaginacao(10, 10), [1, null, 9, 10])
  assert.deepEqual(numerosDaPaginacao(3, 10), [1, 2, 3, 4, null, 10])
})

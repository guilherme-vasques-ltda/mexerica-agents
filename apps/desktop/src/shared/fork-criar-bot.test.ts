import assert from 'node:assert/strict'
import { test } from 'node:test'
import { FUNCOES_EXEMPLO, funcoesProprias } from './fork-bot-funcao.ts'
import { FUNCOES_DA_CRIACAO, PASSOS, anterior, nomesDoPapel, papeisComuns, pedirAutonomia, proximo, sortearNome, ultimo } from './fork-criar-bot.ts'
import { normalizeBot } from './models.ts'

test('quatro passos em ordem, com o botão final só no último', () => {
  assert.deepEqual([...PASSOS], ['papel', 'motor', 'permissoes', 'ferramentas'])
  assert.equal(proximo('papel'), 'motor')
  assert.equal(proximo('ferramentas'), null)
  assert.equal(anterior('papel'), null)
  assert.equal(anterior('permissoes'), 'motor')
  assert.deepEqual(PASSOS.map(ultimo), [false, false, false, true])
})

test('Liberar só vale depois da confirmação; Pedir vale na hora', () => {
  assert.deepEqual(pedirAutonomia('ask', 'auto'), { aplicar: null, confirmar: true })
  assert.deepEqual(pedirAutonomia('auto', 'ask'), { aplicar: 'ask', confirmar: false })
  assert.deepEqual(pedirAutonomia('ask', 'ask'), { aplicar: 'ask', confirmar: false })
  // Herdado da geral, já está em Liberar: escolher de novo não pede outra confirmação.
  assert.deepEqual(pedirAutonomia('auto', 'auto'), { aplicar: 'auto', confirmar: false })
})

test('o dado sorteia outro nome do mesmo estilo, nunca o atual', () => {
  const qa = papeisComuns()[0]!
  const nomes = nomesDoPapel(qa)
  assert.equal(nomes[0], 'Crivo', 'o primeiro é o nome da biblioteca')
  for (const sorte of [0, 0.3, 0.6, 0.99]) {
    const nome = sortearNome(nomes, 'Crivo', sorte)
    assert.notEqual(nome, 'Crivo')
    assert.ok(nomes.includes(nome))
  }
  assert.equal(sortearNome(['Só'], 'Só', 0.5), 'Só', 'com uma opção só, fica nela')
  assert.ok(nomesDoPapel(null).length > 1, 'sem papel, usa a lista geral')
  const outro = FUNCOES_EXEMPLO.find((f) => f.id === 'exemplo:amazon')!
  assert.equal(nomesDoPapel(outro)[0], 'Buybox', 'papel fora dos comuns começa pelo nome da biblioteca')
})

test('os sete papéis comuns existem e os rótulos em inglês aparecem em português, sem mudar IDs', () => {
  assert.equal(papeisComuns().length, 7)
  const nomes = new Map(FUNCOES_EXEMPLO.map((f) => [f.id, f.nome]))
  assert.equal(nomes.get('exemplo:research'), 'Pesquisa')
  assert.equal(nomes.get('exemplo:chief'), 'Chefe de gabinete')
  assert.equal(nomes.get('exemplo:bookkeeper'), 'Conciliação')
  assert.equal(nomes.get('exemplo:contabil'), 'Contabilidade')
  assert.equal(new Set(nomes.values()).size, nomes.size, 'nenhum rótulo repetido entre os 30')
  assert.equal(nomes.get('exemplo:dev'), 'Desenvolvimento')
  for (const ingles of ['Chief of Staff', 'Project Manager', 'Calendar', 'Notes Taker', 'Researcher', 'Travel Planner', 'Bookkeeper']) {
    assert.ok(![...nomes.values()].includes(ingles), ingles)
  }
  // Um bot antigo com a grafia em inglês não vira "função própria".
  assert.deepEqual(funcoesProprias([
    normalizeBot({ id: 'a', name: 'Fonte', title: 'Researcher', createdAt: 1 }),
    normalizeBot({ id: 'b', name: 'Byte', title: 'Desenvolvedor', createdAt: 2 }),
  ]), [])
})

test('D366/D444: a criação não oferece o papel do principal nem o nome reservado (Berga); Donna fica livre', () => {
  assert.equal(FUNCOES_DA_CRIACAO.length, 29)
  assert.ok(!FUNCOES_DA_CRIACAO.some((f) => f.id === 'exemplo:donna' || f.nomeDoBot === 'Donna'))
  assert.ok(!papeisComuns().some((f) => f.id === 'exemplo:donna'))
  const donna = FUNCOES_EXEMPLO.find((f) => f.id === 'exemplo:donna')!
  assert.ok(!nomesDoPapel(donna).includes('Berga'))
  for (const sorte of [0, 0.5, 0.99]) assert.notEqual(sortearNome(['Berga', 'berga '], 'X', sorte).trim().toLowerCase(), 'berga')
  assert.equal(sortearNome(['Donna'], 'X', 0), 'Donna')
})

test('o Hub recebe o title original; só a tela mostra o rótulo traduzido', async () => {
  const { ROLES } = await import('./fork-avatar-config.ts')
  const { exibirFuncao } = await import('./fork-bot-funcao.ts')
  for (const r of ROLES) {
    const f = FUNCOES_EXEMPLO.find((x) => x.id === `exemplo:${r.id}`)!
    assert.equal(f.valor, r.role, `${r.id}: o valor gravado é o da biblioteca`)
    assert.equal(exibirFuncao(f.valor), f.nome, `${r.id}: a tela mostra o rótulo`)
  }
  const pesquisa = FUNCOES_EXEMPLO.find((f) => f.id === 'exemplo:research')!
  assert.deepEqual([pesquisa.valor, pesquisa.nome], ['Researcher', 'Pesquisa'])
  assert.equal(exibirFuncao('Prospecção BR'), 'Prospecção BR', 'função própria fica como foi digitada')
})

test('D366: a função personalizada da principal não vira opção em "Suas funções"', async () => {
  const { funcoesDaCriacao } = await import('./fork-criar-bot.ts')
  const bots = [
    { ...normalizeBot({ id: 'donna', name: 'Donna', title: 'Líder da equipe', createdAt: 3 }), manageTeam: true },
    normalizeBot({ id: 'rafa', name: 'Rafa', title: 'Prospecção BR', createdAt: 2 }),
    normalizeBot({ id: 'outra', name: 'Lia', title: 'Chefia', createdAt: 1 }),
  ]
  assert.deepEqual(funcoesDaCriacao(bots, 'outra').map((f) => f.nome), ['Prospecção BR'], 'sai a principal pelo ID e a que tem a gestão')
  assert.deepEqual(funcoesDaCriacao(bots, null).map((f) => f.nome).sort(), ['Chefia', 'Prospecção BR'], 'sem ID da principal, sai a que tem a gestão')
})

test('catálogo do motor: nomes e valores conhecidos viram chave estável; o resto fica como veio', async () => {
  const { chaveDoCatalogo } = await import('./fork-criar-bot.ts')
  assert.deepEqual(['Reasoning', 'Medium', 'High', 'Low', 'Off', 'reasoning_effort'].map(chaveDoCatalogo), ['reasoning', 'medium', 'high', 'low', 'off', 'reasoning'])
  assert.equal(chaveDoCatalogo('Grok 4.7'), null)
})

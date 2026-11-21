import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  acoesQueAPessoaDecide, agruparContas, agruparPorCategoria, alternarGrupo, alternarVarios, blocosDeBots, botsParaLigar, categoriasDoCatalogo, codigoDaRecusa, contarPorCategoria, cofreTrancado, escolhasDeBots, estadoDoServico, filtrarServicos, gruposVisiveis, jeitoDaConta, jeitoEscolhido, jeitoRecomendado, linhasDoBot, listaDeNomes,
  mostraPersonalizar, motivosDoEmBreve, nomeCurto, nomeSugerido, padraoSeguro, perigosasDoTipo, planoDeNomes, seOPedidoNaoDiz, skillsDaFicha, temAcoesPerigosas, visaoDoCatalogo,
} from './regras.ts'
import { catalogo, conta, grupo, servico, tipo } from './dados-de-teste.ts'

test('o código de recusa vem do começo do texto do Hub, com ou sem prefixo', () => {
  assert.equal(codigoDaRecusa(new Error('nome_repetido: já existe')), 'nome_repetido')
  assert.equal(codigoDaRecusa(new Error('400: nome_primeira_obrigatorio: dê um nome')), 'nome_primeira_obrigatorio')
  assert.equal(codigoDaRecusa(new Error('ficha_vencida: passou de 90 dias')), 'ficha_vencida')
  assert.equal(codigoDaRecusa(new Error('algo inesperado')), null)
  assert.equal(codigoDaRecusa('liberacao_indisponivel: x'), 'liberacao_indisponivel')
})

test('regra de nomes: a 1ª conta não pede nome; da 2ª em diante pede para a nova e, se ainda tem o nome do serviço, para a 1ª', () => {
  assert.deepEqual(planoDeNomes(servico('s', 'Stripe', { contas: [] })), { pedeNomeDaNova: false, pedeNomeDaPrimeira: false, primeira: null })
  const uma = planoDeNomes(servico('s', 'Stripe', { contas: [{ id: 'a', name: 'Stripe', identidade: null }] }))
  assert.equal(uma.pedeNomeDaNova, true)
  assert.equal(uma.pedeNomeDaPrimeira, true)
  const umaNomeada = planoDeNomes(servico('s', 'Stripe', { contas: [{ id: 'a', name: 'Stripe – Loja', identidade: null }] }))
  assert.equal(umaNomeada.pedeNomeDaNova, true)
  assert.equal(umaNomeada.pedeNomeDaPrimeira, false)
  const duas = planoDeNomes(servico('s', 'Stripe', { contas: [{ id: 'a', name: 'Stripe', identidade: null }, { id: 'b', name: 'Stripe – X', identidade: null }] }))
  assert.equal(duas.pedeNomeDaPrimeira, false)
  assert.equal(duas.pedeNomeDaNova, true)
})

test('nome sugerido: vazio na 1ª conta; "Serviço 2" na 2ª; pula o número que já existe', () => {
  assert.equal(nomeSugerido(servico('s', 'Stripe', { contas: [] })), '')
  assert.equal(nomeSugerido(servico('s', 'Stripe', { contas: [{ id: 'a', name: 'Stripe', identidade: null }] })), 'Stripe 2')
  const duas = [{ id: 'a', name: 'Stripe', identidade: null }, { id: 'b', name: 'Stripe 3', identidade: null }]
  assert.equal(nomeSugerido(servico('s', 'Stripe', { contas: duas })), 'Stripe 4')
  const ocupado = [{ id: 'a', name: 'Stripe', identidade: null }, { id: 'b', name: 'stripe 2', identidade: null }]
  assert.equal(nomeSugerido(servico('s', 'Stripe', { contas: ocupado })), 'Stripe 3')
})

test('"Personalizar" só com mais de 3 grupos; antes dele, só o padrão seguro à vista', () => {
  const tres = tipo([grupo('a', { padrao: true }), grupo('b'), grupo('c')])
  const quatro = tipo([grupo('a', { padrao: true }), grupo('b'), grupo('c'), grupo('d')])
  assert.equal(mostraPersonalizar(tres), false)
  assert.equal(mostraPersonalizar(quatro), true)
  assert.deepEqual(gruposVisiveis(tres, false).map((g) => g.id), ['a', 'b', 'c'])
  assert.deepEqual(gruposVisiveis(quatro, false).map((g) => g.id), ['a'])
  assert.deepEqual(gruposVisiveis(quatro, true).map((g) => g.id), ['a', 'b', 'c', 'd'])
})

test('o padrão seguro marca só o grupo padrão disponível; alternar mantém a ordem da ficha e ignora grupo indisponível', () => {
  const t = tipo([grupo('ler', { padrao: true }), grupo('criar'), grupo('editar', { disponivel: false })])
  assert.deepEqual(padraoSeguro(t), ['ler'])
  assert.deepEqual(alternarGrupo(t, ['ler'], 'criar'), ['ler', 'criar'])
  assert.deepEqual(alternarGrupo(t, ['criar', 'ler'], 'ler'), ['criar'])
  assert.deepEqual(alternarGrupo(t, ['ler'], 'editar'), ['ler'])
  assert.deepEqual(alternarGrupo(t, ['ler'], 'nao-existe'), ['ler'])
})

test('ações perigosas: a lista da ficha, e nada quando a ficha não tem', () => {
  assert.deepEqual(perigosasDoTipo(tipo([grupo('a')])), [])
  assert.deepEqual(perigosasDoTipo(null), [])
  assert.equal(perigosasDoTipo(tipo([grupo('a')], { perigosas: [{ ferramenta: 'f', rotulo: 'Apagar' }] })).length, 1)
})

test('busca sem caixa nem acento, por nome, descrição e exemplo; categorias na ordem do Hub', () => {
  const a = servico('a', 'Alfa', { categoria: 'Pagamentos', exemplos: ['Listar cobranças do mês'] })
  const b = servico('b', 'Beta', { categoria: 'Código' })
  const c = servico('c', 'Gama', { emBreve: 'em breve' })
  assert.deepEqual(filtrarServicos([a, b], 'COBRANCAS', null).map((s) => s.id), ['a'])
  assert.deepEqual(filtrarServicos([a, b], '', 'Código').map((s) => s.id), ['b'])
  assert.deepEqual(filtrarServicos([a, b], 'nada disso', null), [])
  assert.deepEqual(categoriasDoCatalogo([a, b, c]), ['Pagamentos', 'Código', 'Projetos'], 'o em breve também tem a sua categoria')
})

test('visão do catálogo: todos os conectáveis na lista, na ordem do Hub, sem destaque; "Em breve" fica à parte', () => {
  const a = servico('a', 'Alfa')
  const b = servico('b', 'Beta')
  const g = servico('g', 'Gmail', { emBreve: 'sem caminho hoje' })
  const cat = catalogo([a, b, g])
  const v = visaoDoCatalogo(cat, '', null)
  assert.deepEqual(v.lista.map((s) => s.id), ['a', 'b'])
  assert.deepEqual(v.emBreve.map((s) => s.id), ['g'])
  assert.equal(v.total, 2)
  assert.equal('destaque' in v, false)
  const filtrado = visaoDoCatalogo(cat, 'beta', null)
  assert.deepEqual(filtrado.lista.map((s) => s.id), ['b'])
  const vencido = visaoDoCatalogo(catalogo([servico('v', 'Velho', { conectavel: false, conferenciaVigente: false }), b]), '', null)
  assert.deepEqual(vencido.lista.map((s) => s.id), ['v', 'b'])
})

test('estado do serviço: sem conta, N contas ou reconectar quando alguma expirou', () => {
  const s = servico('s', 'S')
  assert.deepEqual(estadoDoServico(s, []), { tipo: 'semConta' })
  assert.deepEqual(estadoDoServico(s, [conta('1', 'S', 's'), conta('2', 'S – B', 's')]), { tipo: 'contas', n: 2 })
  assert.deepEqual(estadoDoServico(s, [conta('1', 'S', 's', { auth: 'signedOut' })]), { tipo: 'reconectar', n: 1 })
  assert.deepEqual(estadoDoServico(s, [conta('1', 'X', 'outro')]), { tipo: 'semConta' })
})

test('contas agrupadas por serviço na ordem do catálogo; MCP do usuário por último', () => {
  const cat = catalogo([servico('a', 'Alfa'), servico('b', 'Beta')])
  const grupos = agruparContas([conta('3', 'Meu', null), conta('2', 'Beta', 'b'), conta('1', 'Alfa', 'a')], cat)
  assert.deepEqual(grupos.map((g) => g.servico), ['a', 'b', null])
})

test('serviço fora do catálogo não mostra o id técnico: o grupo fica sem nome e marcado como fora do catálogo', () => {
  const cat = catalogo([servico('a', 'Alfa')])
  const grupos = agruparContas([conta('1', 'Minha loja', 'loja_agil_v2'), conta('2', 'Meu', null)], cat)
  assert.deepEqual(grupos.map((g) => [g.servico, g.nome, g.foraDoCatalogo ?? false]), [['loja_agil_v2', null, true], [null, null, false]])
  assert.equal(JSON.stringify(grupos).includes('"nome":"loja_agil_v2"'), false)
})

test('ficha de dois jeitos: liberações e ações perigosas seguem o jeito escolhido, não o recomendado', () => {
  const oauth = tipo([grupo('ler', { padrao: true }), grupo('escrever')], { id: 's.oficial_oauth', recomendado: true, perigosas: [{ ferramenta: 'apagar', rotulo: 'Apagar tudo' }] })
  const chave = tipo([grupo('so_leitura', { padrao: true })], { id: 's.chave', tipo: 'oficial_chave', recomendado: false, perigosas: [] })
  const s = servico('s', 'Serv', { tipos: [oauth, chave] })
  assert.equal(jeitoRecomendado(s)?.id, 's.oficial_oauth')
  const aberto = jeitoEscolhido(s, 's.oficial_oauth')
  assert.deepEqual(aberto?.grupos.map((g) => g.id), ['ler', 'escrever'])
  assert.deepEqual(perigosasDoTipo(aberto).map((p) => p.ferramenta), ['apagar'])
  const marcado = jeitoEscolhido(s, 's.chave')
  assert.deepEqual(marcado?.grupos.map((g) => g.id), ['so_leitura'])
  assert.deepEqual(perigosasDoTipo(marcado), [])
  // Sem escolha válida (ou jeito indisponível), volta ao recomendado.
  assert.equal(jeitoEscolhido(s, null)?.id, 's.oficial_oauth')
  assert.equal(jeitoEscolhido(servico('s', 'Serv', { tipos: [oauth, { ...chave, disponivel: false }] }), 's.chave')?.id, 's.oficial_oauth')
})

test('"cofre trancado" só quando o Hub diz (contas: null na ficha); falha de marketContas sem causa não é cofre', () => {
  assert.equal(cofreTrancado(catalogo([servico('a', 'Alfa', { contas: null })])), true)
  assert.equal(cofreTrancado(catalogo([servico('a', 'Alfa', { contas: [] }), servico('b', 'Beta', { contas: null })])), true)
  assert.equal(cofreTrancado(catalogo([servico('a', 'Alfa', { contas: [] })])), false)
  assert.equal(cofreTrancado(null), false)
})

test('nome curto tira o prefixo do serviço', () => {
  assert.equal(nomeCurto('GitHub – Pessoal', 'GitHub'), 'Pessoal')
  assert.equal(nomeCurto('GitHub', 'GitHub'), 'GitHub')
  assert.equal(nomeCurto('Loja Centro', 'Stripe'), 'Loja Centro')
  assert.equal(listaDeNomes(['Pessoal', 'Estúdio'], 'e'), 'Pessoal e Estúdio')
  assert.equal(listaDeNomes(['A', 'B', 'C'], 'e'), 'A, B e C')
})

const noBot = (id: string, extra = {}) => ({ id, nome: id, ligada: true, padrao: false, expirada: false, identidade: null, ...extra })

test('se o pedido não diz a conta: padrão, única ligada ou pergunta; "perguntar" anula o padrão', () => {
  assert.deepEqual(seOPedidoNaoDiz([], false), { tipo: 'nenhuma' })
  assert.deepEqual(seOPedidoNaoDiz([noBot('a')], false), { tipo: 'usa', conta: 'a' })
  assert.deepEqual(seOPedidoNaoDiz([noBot('a'), noBot('b', { padrao: true })], false), { tipo: 'usa', conta: 'b' })
  assert.deepEqual(seOPedidoNaoDiz([noBot('a'), noBot('b')], false), { tipo: 'pergunta' })
  assert.deepEqual(seOPedidoNaoDiz([noBot('a'), noBot('b', { padrao: true })], true), { tipo: 'pergunta' })
  assert.deepEqual(seOPedidoNaoDiz([noBot('a')], true), { tipo: 'usa', conta: 'a' })
})

test('linhas do bot: uma por serviço com conta no Space; a coluna de perigosas só existe se alguma ficha tiver', () => {
  const cat = catalogo([servico('a', 'Alfa'), servico('b', 'Beta'), servico('c', 'Gama')])
  const contas = [conta('a1', 'Alfa – 1', 'a', { identidade: 'a@x.com' }), conta('b1', 'Beta', 'b', { auth: 'signedOut' })]
  const linhas = linhasDoBot(cat, contas, {
    botId: 'nina', spaceId: 'pessoal', perguntarConta: {},
    contas: [
      { conta: 'a1', nome: 'Alfa – 1', servico: 'a', ligada: true, padraoDoServico: false },
      { conta: 'b1', nome: 'Beta', servico: 'b', ligada: false, padraoDoServico: false },
    ],
  })
  assert.deepEqual(linhas.map((l) => l.servico), ['a', 'b'])
  assert.deepEqual(linhas[0].seODizNaoPedido, { tipo: 'usa', conta: 'a1' })
  assert.deepEqual(linhas[1].seODizNaoPedido, { tipo: 'nenhuma' })
  assert.equal(linhas[1].contas[0].expirada, true)
  assert.equal(temAcoesPerigosas(linhas), false)
  const comPerigosa = catalogo([servico('a', 'Alfa', { tipos: [tipo([grupo('ler', { padrao: true })], { perigosas: [{ ferramenta: 'f', rotulo: 'Mesclar' }] })] })])
  const l2 = linhasDoBot(comPerigosa, [contas[0]], { botId: 'n', spaceId: 'pessoal', perguntarConta: {}, contas: [{ conta: 'a1', nome: 'Alfa – 1', servico: 'a', ligada: true, padraoDoServico: false }] })
  assert.equal(temAcoesPerigosas(l2), true)
})

test('o jeito da conta vem do id do jeito na ficha, nunca do texto do id', () => {
  const cat = catalogo([servico('a', 'Alfa', { tipos: [tipo([grupo('ler', { padrao: true })], { id: 'a.oficial_chave', tipo: 'oficial_chave' })] })])
  assert.equal(jeitoDaConta(conta('1', 'Alfa', 'a', { tipo: 'a.oficial_chave' }), cat), 'oficial_chave')
  assert.equal(jeitoDaConta(conta('2', 'Meu', null), cat), null)
  assert.equal(jeitoDaConta(conta('3', 'Alfa', 'a', { tipo: 'a.outro' }), cat), null)
})

// ───────────────────────────── Usar em quais bots? ─────────────────────────────

const nos = [
  { id: 'mkt', name: 'Marketing', parentId: null, managerBotId: null, position: 0 },
  { id: 'soc', name: 'Social', parentId: 'mkt', managerBotId: null, position: 0 },
  { id: 'fin', name: 'Finanças', parentId: null, managerBotId: null, position: 1 },
]
const membro = (botId: string, teamId: string | null) => ({ botId, teamId, parentBotId: null, role: 'executor' as const, reportsTo: { kind: 'owner' as const }, ownerContact: 'reports' as const })
const times = { nodes: nos, members: [membro('vit', 'soc'), membro('str', 'soc'), membro('sal', 'fin'), membro('joi', null)] }
const botsDaConta = [
  { id: 'ber', name: 'Berga' }, { id: 'joi', name: 'Joio' }, { id: 'vit', name: 'Vitrine' }, { id: 'str', name: 'Strip' }, { id: 'sal', name: 'Saldo' },
]
const funcoes: Record<string, string> = { ber: 'Principal', joi: 'Pesquisa', vit: 'Loja virtual', str: 'Pagamentos', sal: 'Contas a pagar' }
const escolhas = () => escolhasDeBots(botsDaConta, (id) => funcoes[id], times, 'ber')

test('quem é: função, equipe com o caminho acima e o principal pelo id que o app recebe, nunca pelo nome', () => {
  const e = escolhas()
  assert.deepEqual(e.find((b) => b.id === 'vit'), { id: 'vit', name: 'Vitrine', funcao: 'Loja virtual', equipe: 'Marketing › Social', principal: false })
  assert.equal(e.find((b) => b.id === 'sal')?.equipe, 'Finanças')
  assert.equal(e.find((b) => b.id === 'joi')?.equipe, null)
  assert.equal(e.find((b) => b.id === 'ber')?.principal, true)
  // Um bot chamado "Berga" que não é o principal continua marcável; o principal com outro nome continua bloqueado.
  const outro = escolhasDeBots([{ id: 'x', name: 'Berga' }, { id: 'y', name: 'Nina' }], () => undefined, null, 'y')
  assert.deepEqual(outro.map((b) => [b.name, b.principal]), [['Berga', false], ['Nina', true]])
  // Principal desconhecido (identidade ainda não confirmada): ninguém é marcado como principal pela tela; o Hub recusa.
  assert.equal(escolhasDeBots(botsDaConta, () => undefined, null, null).some((b) => b.principal), false)
})

test('equipes: um bloco por equipe, "sem equipe" depois e o principal por último; sem dados de equipes, uma lista só', () => {
  const blocos = blocosDeBots(escolhas(), '')
  assert.deepEqual(blocos.map((b) => [b.chave, b.titulo, b.bots.map((x) => x.name)]), [
    ['equipe:Marketing › Social', 'Marketing › Social', ['Vitrine', 'Strip']],
    ['equipe:Finanças', 'Finanças', ['Saldo']],
    ['$sem-equipe', null, ['Joio']],
    ['$principal', null, ['Berga']],
  ])
  const plana = blocosDeBots(escolhasDeBots(botsDaConta, (id) => funcoes[id], null, 'ber'), '')
  assert.deepEqual(plana.map((b) => [b.chave, b.bots.map((x) => x.name)]), [['$todos', ['Joio', 'Vitrine', 'Strip', 'Saldo']], ['$principal', ['Berga']]])
})

test('busca: acha por nome, função e equipe, sem acento nem caixa, e mantém os títulos das equipes', () => {
  const nomes = (busca: string) => blocosDeBots(escolhas(), busca).flatMap((b) => b.bots.map((x) => x.name))
  assert.deepEqual(nomes('vitri'), ['Vitrine'])
  assert.deepEqual(nomes('PAGAMENTOS'), ['Strip'])
  assert.deepEqual(nomes('financas'), ['Saldo'])
  assert.deepEqual(nomes('marketing social'), ['Vitrine', 'Strip'])
  assert.deepEqual(nomes('berga'), ['Berga'], 'o principal aparece na busca, desabilitado')
  assert.deepEqual(nomes('nada disso'), [])
  assert.equal(blocosDeBots(escolhas(), 'saldo')[0].titulo, 'Finanças')
})

test('"Marcar todos do grupo": marca só os marcáveis visíveis; com todos marcados, desmarca; o principal nunca entra', () => {
  const [social] = blocosDeBots(escolhas(), '')
  assert.deepEqual(social.marcaveis, ['vit', 'str'])
  const marcados = alternarVarios(new Set(['joi']), social.marcaveis)
  assert.deepEqual([...marcados].sort(), ['joi', 'str', 'vit'])
  assert.deepEqual([...alternarVarios(marcados, social.marcaveis)], ['joi'])
  // Em busca, o grupo só tem quem apareceu.
  assert.deepEqual(blocosDeBots(escolhas(), 'vitrine')[0].marcaveis, ['vit'])
  const principal = blocosDeBots(escolhas(), '').at(-1)!
  assert.deepEqual(principal.marcaveis, [])
  assert.deepEqual([...alternarVarios(new Set(), [])], [])
})

test('o que vai ao Hub nunca leva o principal, mesmo marcado por engano', () => {
  assert.deepEqual(botsParaLigar(new Set(['ber', 'vit', 'zzz']), escolhas()), ['vit'])
})

test('a recusa do principal vem do código estável do Hub', () => {
  assert.equal(codigoDaRecusa(new Error('principal_sem_contas: o bot principal do Space não usa contas.')), 'principal_sem_contas')
})

test('as ações que a pessoa decide: tudo menos o permitido, em ordem de rótulo; a recusa do Hub tem código estável', () => {
  const f = (nome: string, estado: 'permitida' | 'ligada' | 'desligada', rotulo: string | null = null, motivo: 'perigosa' | 'nova' | 'alterada' | null = null) => ({ nome, rotulo, estado, motivo, digest: 'd' })
  const dados = { botId: 'b', conta: 'c', inventarioPronto: true, vinculo: 0, ferramentas: [f('list_orders', 'permitida'), f('x_novo', 'desligada', null, 'nova'), f('create_refund', 'ligada', 'Reembolsar'), f('delete_all', 'desligada', 'Apagar tudo', 'perigosa')] }
  assert.deepEqual(acoesQueAPessoaDecide(dados).map((x) => x.nome), ['delete_all', 'create_refund', 'x_novo'])
  assert.deepEqual(acoesQueAPessoaDecide({ ...dados, ferramentas: [f('list_orders', 'permitida')] }), [])
  assert.equal(codigoDaRecusa(new Error('ferramenta_mudou: a ação mudou.')), 'ferramenta_mudou')
  assert.equal(codigoDaRecusa(new Error('confirmacao_obrigatoria: confirme.')), 'confirmacao_obrigatoria')
  assert.equal(codigoDaRecusa(new Error('vinculo_mudou: a conta saiu do bot.')), 'vinculo_mudou')
})

test('o catálogo vem por categoria, na ordem do Hub; categoria que o Hub não conhece vai depois', () => {
  const s = [
    servico('a', 'A', { categoria: 'Dados' }), servico('b', 'B', { categoria: 'Projetos' }), servico('c', 'C', { categoria: 'Dados' }),
    servico('d', 'D', { categoria: 'Nova' }), servico('e', 'E', { categoria: 'Código', emBreve: 'sem data', conectavel: false }),
  ]
  const ordem = ['Projetos', 'Código', 'Dados']
  assert.deepEqual(categoriasDoCatalogo(s, ordem), ['Projetos', 'Código', 'Dados', 'Nova'], 'inclui a categoria que só tem serviço em breve')
  assert.deepEqual(categoriasDoCatalogo(s), ['Dados', 'Projetos', 'Nova', 'Código'], 'sem ordem do Hub, a ordem em que aparecem')
  const grupos = agruparPorCategoria(s.slice(0, 4), ordem)
  assert.deepEqual(grupos.map((g) => [g.categoria, g.servicos.map((x) => x.id)]), [['Projetos', ['b']], ['Dados', ['a', 'c']], ['Nova', ['d']]])
  const visao = visaoDoCatalogo(catalogo(s, ordem), '', null)
  assert.deepEqual(visao.porCategoria.map((g) => g.categoria), ['Projetos', 'Dados', 'Nova'])
  assert.deepEqual(visao.porCategoria.flatMap((g) => g.servicos), visao.lista, 'o agrupamento não perde nem repete serviço')
  const dados = visaoDoCatalogo(catalogo(s, ordem), '', 'Dados')
  assert.deepEqual(dados.porCategoria.map((g) => g.categoria), ['Dados'])
})

test('a categoria escolhida mostra os conectáveis e os em breve dela, e cada categoria diz quantos serviços tem', () => {
  const s = [
    servico('a', 'A', { categoria: 'Dados' }), servico('b', 'B', { categoria: 'Código', emBreve: 'sem data', conectavel: false }),
    servico('c', 'C', { categoria: 'Código' }), servico('d', 'D', { categoria: 'Dados', emBreve: 'depois', conectavel: false }),
  ]
  assert.deepEqual(contarPorCategoria(s), { Dados: 2, Código: 2 })
  const codigo = visaoDoCatalogo(catalogo(s), '', 'Código')
  assert.deepEqual(codigo.lista.map((x) => x.id), ['c'])
  assert.deepEqual(codigo.emBreve.map((x) => x.id), ['b'])
  const soEmBreve = visaoDoCatalogo(catalogo([servico('b', 'B', { categoria: 'Código', emBreve: 'sem data', conectavel: false })]), '', 'Código')
  assert.deepEqual([soEmBreve.lista.length, soEmBreve.emBreve.length], [0, 1])
})

test('o motivo do em breve é o da ficha; sem ele, o dos jeitos, sem repetir nem vazio', () => {
  const sem = servico('s', 'S', { emBreve: null, tipos: [tipo([], { disponivel: false, motivoIndisponivel: 'Entra depois do filtro.' }), tipo([], { disponivel: false, motivoIndisponivel: '  ' }), tipo([], { disponivel: false, motivoIndisponivel: null })] })
  assert.deepEqual(motivosDoEmBreve(sem), ['Entra depois do filtro.'])
  const dois = servico('d', 'D', { emBreve: 'A conta também libera execuções cobradas.', tipos: [tipo([], { disponivel: false, motivoIndisponivel: 'Sem como limitar só à busca.' })] })
  assert.deepEqual(motivosDoEmBreve(dois), ['A conta também libera execuções cobradas.'], 'o motivo da ficha manda')
  const semFicha = servico('f', 'F', { emBreve: null, tipos: [tipo([], { disponivel: false, motivoIndisponivel: 'Só o jeito diz.' })] })
  assert.deepEqual(motivosDoEmBreve(semFicha), ['Só o jeito diz.'])
  assert.deepEqual(motivosDoEmBreve(servico('v', 'V', { emBreve: null, tipos: [] })), [])
})

test('o total do cabeçalho acompanha a lista exibida, com a busca e a categoria', () => {
  const s = [
    servico('a', 'A', { categoria: 'Dados' }), servico('b', 'B', { categoria: 'Código' }), servico('c', 'C', { categoria: 'Código' }),
    servico('d', 'D', { categoria: 'Código', emBreve: 'depois', conectavel: false }),
  ]
  assert.equal(visaoDoCatalogo(catalogo(s), '', null).total, 3)
  assert.equal(visaoDoCatalogo(catalogo(s), '', 'Código').total, 2, 'o em breve não entra na contagem da lista')
  assert.equal(visaoDoCatalogo(catalogo(s), 'a', 'Dados').total, 1)
  assert.equal(visaoDoCatalogo(catalogo(s), 'nada disso', null).total, 0)
})

test('skills sugeridas: Hub antigo sem a lista (ou com ela vazia) não mostra seção; a lista nova passa inteira', () => {
  assert.deepEqual(skillsDaFicha({}), [])
  assert.deepEqual(skillsDaFicha({ skills: null }), [])
  assert.deepEqual(skillsDaFicha({ skills: [] }), [])
  const uma = { id: 'x-fazer-y', titulo: 'Fazer Y', resumo: 'Faz Y só por leitura.' }
  assert.deepEqual(skillsDaFicha({ skills: [uma] }), [uma])
  assert.deepEqual(skillsDaFicha(servico('s', 'S', { skills: [uma], emBreve: 'depois' })), [uma], 'a ficha em breve também sugere')
})

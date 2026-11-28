// Marketplace (fatia 1b): funções puras que preparam o que a tela mostra, sem DOM, Hub nem React. As regras que
// decidem (nome único, Space, escopos, liberações) são do Hub; aqui só se escolhe o que exibir e como ler a recusa.
import { matchesWords, searchWords } from '../../../lib/busca.ts'
import type { TimesGraph } from '../../../../shared/fork-times.ts'
import type { PedidoPersonalizado } from './api.ts'
import type { BotDaConta, Catalogo, PassoDoGuia, Conta, ContaResumo, FerramentaDaConta, FerramentasDaConta, Grupo, Perigosa, Servico, SkillSugerida, TipoConexao, TipoId, VinculosDoBot } from './tipos.ts'

/** Os códigos estáveis de recusa do Hub (`docs/features/marketplace.md`, "API do Marketplace"). */
export const CODIGOS_DE_RECUSA = [
  'nome_obrigatorio', 'nome_primeira_obrigatorio', 'nome_repetido', 'nome_invalido', 'ficha_vencida',
  'servico_desconhecido', 'tipo_desconhecido', 'tipo_indisponivel', 'liberacao_desconhecida', 'liberacao_indisponivel',
  'liberacao_obrigatoria', 'conta_de_outro_space', 'conta_padrao_invalida', 'ficha_indisponivel', 'ligar_e_desligar', 'principal_sem_contas',
  'confirmacao_obrigatoria', 'ferramenta_mudou', 'vinculo_mudou', 'vinculo_obrigatorio', 'ferramenta_desconhecida', 'ferramenta_fora_da_ficha', 'conta_sem_ficha', 'conta_desligada_no_bot', 'descricao_invalida', 'descricao_desconhecida', 'app_desatualizado',
  'app_proprio_ausente', 'app_google_invalido', 'cofre_trancado', 'credencial_com_escrita',
] as const
export type CodigoDeRecusa = (typeof CODIGOS_DE_RECUSA)[number]

/** O código estável no começo (ou dentro) do texto de uma recusa do Hub; `null` se não houver nenhum conhecido. */
export function codigoDaRecusa(erro: unknown): CodigoDeRecusa | null {
  const texto = erro instanceof Error ? erro.message : String(erro ?? '')
  for (const codigo of CODIGOS_DE_RECUSA) if (new RegExp(`(^|[^a-z_])${codigo}\\s*:`).test(texto)) return codigo
  return null
}

/** Minúsculas, sem acento nem pontuação, espaços únicos: a mesma régua da busca e do "nome parecido". */
export function normalizar(texto: string): string {
  return texto.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, ' ').trim()
}

// ───────────────────────────── Nomes ─────────────────────────────

export interface PlanoDeNomes {
  /** A conta nova precisa de nome (já existe ao menos uma do serviço neste Space). */
  pedeNomeDaNova: boolean
  /** A 1ª conta, que ainda se chama só pelo nome do serviço, também precisa de um nome. */
  pedeNomeDaPrimeira: boolean
  primeira: ContaResumo | null
}

/**
 * Espelha a regra do Hub só para decidir quais campos mostrar (ele confere de novo ao conectar): a 1ª conta não pede
 * nome; da 2ª em diante o nome é obrigatório para a nova e, enquanto a 1ª ainda tiver só o nome do serviço, para ela.
 */
export function planoDeNomes(servico: Pick<Servico, 'nome' | 'contas'>): PlanoDeNomes {
  const contas = servico.contas ?? []
  if (contas.length === 0) return { pedeNomeDaNova: false, pedeNomeDaPrimeira: false, primeira: null }
  const primeira = contas[0]
  const semNome = contas.length === 1 && normalizar(primeira.name) === normalizar(servico.nome)
  return { pedeNomeDaNova: true, pedeNomeDaPrimeira: semNome, primeira }
}

/**
 * O nome que a tela já põe no campo da conta nova ("Linear 2"): o primeiro número livre a partir de 2, sem repetir um
 * nome que já existe (o Hub recusa nome repetido). Só vale da 2ª conta em diante; a pessoa edita à vontade.
 */
export function nomeSugerido(servico: Pick<Servico, 'nome' | 'contas'>): string {
  const contas = servico.contas ?? []
  if (contas.length === 0) return ''
  const usados = new Set(contas.map((c) => normalizar(c.name)))
  let n = Math.max(2, contas.length + 1)
  while (usados.has(normalizar(`${servico.nome} ${n}`))) n++
  return `${servico.nome} ${n}`
}

// ───────────────────────────── Liberações ─────────────────────────────

/** `Personalizar` só aparece quando a ficha tem mais de 3 grupos de liberação (decisão do dono, 16/11). */
export const LIMITE_SEM_PERSONALIZAR = 3
export const mostraPersonalizar = (tipo: Pick<TipoConexao, 'grupos'>) => tipo.grupos.length > LIMITE_SEM_PERSONALIZAR

/** O padrão seguro: os grupos pré-marcados e disponíveis, na ordem da ficha. */
export const padraoSeguro = (tipo: Pick<TipoConexao, 'grupos'>): string[] => tipo.grupos.filter((g) => g.padrao && g.disponivel).map((g) => g.id)

/** Os grupos exibidos de início: todos, ou só os do padrão quando há "Personalizar" e a pessoa ainda não abriu. */
export function gruposVisiveis(tipo: Pick<TipoConexao, 'grupos'>, personalizando: boolean): Grupo[] {
  if (!mostraPersonalizar(tipo) || personalizando) return tipo.grupos
  return tipo.grupos.filter((g) => g.padrao && g.disponivel)
}

/** Os grupos que "Personalizar" ainda esconde (para a linha "Mais N: …"). */
export const gruposEscondidos = (tipo: Pick<TipoConexao, 'grupos'>): Grupo[] => tipo.grupos.filter((g) => !(g.padrao && g.disponivel))

/** Liga ou desliga um grupo, mantendo a ordem da ficha. Grupo indisponível nunca entra. */
export function alternarGrupo(tipo: Pick<TipoConexao, 'grupos'>, escolhidos: readonly string[], id: string): string[] {
  const grupo = tipo.grupos.find((g) => g.id === id)
  if (!grupo?.disponivel) return [...escolhidos]
  const ligar = !escolhidos.includes(id)
  return tipo.grupos.filter((g) => (g.id === id ? ligar : escolhidos.includes(g.id))).map((g) => g.id)
}

/** O jeito que a ficha abre marcado: o recomendado e disponível, senão o primeiro disponível. */
export function jeitoRecomendado(servico: Pick<Servico, 'tipos'>): TipoConexao | null {
  const disponiveis = servico.tipos.filter((t) => t.disponivel)
  return disponiveis.find((t) => t.recomendado) ?? disponiveis[0] ?? null
}

/** O jeito ESCOLHIDO na ficha (o botão Conectar, as liberações e as ações perigosas seguem este); sem escolha válida, o recomendado. */
export const jeitoEscolhido = (servico: Pick<Servico, 'tipos'>, escolhido: string | null): TipoConexao | null =>
  servico.tipos.find((t) => t.id === escolhido && t.disponivel) ?? jeitoRecomendado(servico)

/** As ações perigosas da ficha: a tela só as lista (desligadas) e some quando não há nenhuma. */
export const perigosasDoTipo = (tipo: Pick<TipoConexao, 'perigosas'> | null | undefined): Perigosa[] => tipo?.perigosas ?? []

// ───────────────────────────── Catálogo ─────────────────────────────

export interface SeloDoServico { oficial: boolean; jeitos: number }

export function seloDoServico(servico: Pick<Servico, 'tipos'>): SeloDoServico {
  const ativos = servico.tipos.filter((t) => t.disponivel)
  return { oficial: servico.tipos.some((t) => t.tipo.startsWith('oficial')), jeitos: ativos.length }
}

/** Dá para começar a conectar agora: a ficha tem um jeito disponível e a conferência está em dia. */
export const podeConectar = (s: Pick<Servico, 'conectavel' | 'emBreve'>) => s.emBreve === null && s.conectavel

/** A posição da categoria na ordem do Hub; categoria fora dela (Hub antigo) vai depois, na ordem em que apareceu. */
const posicaoDaCategoria = (ordem: readonly string[], categoria: string) => {
  const i = ordem.indexOf(categoria)
  return i === -1 ? ordem.length : i
}

/** Todas as categorias do catálogo (inclusive as que só têm serviço "em breve"), na ordem do Hub. */
export function categoriasDoCatalogo(servicos: readonly Servico[], ordem: readonly string[] = []): string[] {
  const vistas: string[] = []
  for (const s of servicos) if (!vistas.includes(s.categoria)) vistas.push(s.categoria)
  return vistas.map((c, i) => ({ c, i })).sort((a, b) => posicaoDaCategoria(ordem, a.c) - posicaoDaCategoria(ordem, b.c) || a.i - b.i).map(({ c }) => c)
}

/** As skills que a ficha sugere; Hub antigo não manda a lista (ou manda vazia): nada a mostrar. */
export const skillsDaFicha = (servico: { skills?: readonly SkillSugerida[] | null }): readonly SkillSugerida[] => servico.skills ?? []

/** Quantos serviços cada categoria tem (os "em breve" contam), para o número ao lado do nome da categoria. */
export function contarPorCategoria(servicos: readonly Pick<Servico, 'categoria'>[]): Record<string, number> {
  const n: Record<string, number> = {}
  for (const s of servicos) n[s.categoria] = (n[s.categoria] ?? 0) + 1
  return n
}

/** Por que um serviço "em breve" ainda não conecta: o motivo da ficha; sem ele, o de cada jeito de conectar (sem repetir nem vazio). */
export function motivosDoEmBreve(servico: Pick<Servico, 'emBreve' | 'tipos'>): string[] {
  const motivos = servico.emBreve === null ? servico.tipos.map((t) => t.motivoIndisponivel) : [servico.emBreve]
  return motivos.filter((m, i): m is string => typeof m === 'string' && m.trim() !== '' && motivos.indexOf(m) === i)
}

export interface GrupoDeCategoria { categoria: string; servicos: Servico[] }

/** A lista agrupada por categoria, na ordem do Hub; dentro de cada uma, a ordem do catálogo. */
export function agruparPorCategoria(servicos: readonly Servico[], ordem: readonly string[] = []): GrupoDeCategoria[] {
  const grupos: GrupoDeCategoria[] = []
  for (const s of servicos) {
    const grupo = grupos.find((g) => g.categoria === s.categoria)
    if (grupo) grupo.servicos.push(s)
    else grupos.push({ categoria: s.categoria, servicos: [s] })
  }
  return grupos.map((g, i) => ({ g, i })).sort((a, b) => posicaoDaCategoria(ordem, a.g.categoria) - posicaoDaCategoria(ordem, b.g.categoria) || a.i - b.i).map(({ g }) => g)
}

function textoBuscavel(s: Servico): string {
  return normalizar([s.nome, s.categoria, s.descricao, ...s.exemplos].join(' '))
}

/** Busca por nome, categoria, descrição e exemplos de pedidos, sem caixa nem acento; todas as palavras precisam casar. */
export function filtrarServicos(servicos: readonly Servico[], busca: string, categoria: string | null): Servico[] {
  const palavras = normalizar(busca).split(' ').filter(Boolean)
  return servicos.filter((s) => (categoria === null || s.categoria === categoria) && (() => {
    const alvo = textoBuscavel(s)
    return palavras.every((p) => alvo.includes(p))
  })())
}

// ───────────────────────────── Conector personalizado ─────────────────────────────

export type ModoPersonalizado = 'link' | 'comando'

/** O pedido ao Hub: a chave só entra no cabeçalho de um link (o comando não leva chave por aqui). */
export function pedidoDoPersonalizado(modo: ModoPersonalizado, alvo: string, nome: string, chave: string): PedidoPersonalizado {
  const base = { name: nome.trim() }
  if (modo === 'comando') return { ...base, command: alvo.trim() }
  const pedido: PedidoPersonalizado = { ...base, url: alvo.trim() }
  return chave.trim() === '' ? pedido : { ...pedido, headers: { Authorization: `Bearer ${chave.trim()}` } }
}

// ───────────────────────────── Google direto (o app OAuth da própria pessoa) ─────────────────────────────

/** O serviço entra com o app Google da pessoa (`Auth::AppProprio`, provedor Google): vive no grupo "Google". */
export const ehDoGoogle = (s: Pick<Servico, 'tipos'>): boolean => s.tipos.some((t) => t.auth.modo === 'app_proprio' && t.auth.provedor === 'google')

/** Os passos do guia "crie o seu app" que a ficha manda (iguais nos serviços do Google); vazio se a ficha não traz. */
export function guiaDoGoogle(servicos: readonly Pick<Servico, 'tipos'>[]): PassoDoGuia[] {
  for (const s of servicos) for (const t of s.tipos) if (t.auth.modo === 'app_proprio' && t.auth.provedor === 'google') return t.auth.guia
  return []
}

/** Os escopos pedidos ao Google são só os dos grupos marcados: a tela só marca grupos, nunca escopos. */
export const jeitoDoGoogle = (s: Pick<Servico, 'tipos'>): TipoConexao | null =>
  s.tipos.find((t) => t.disponivel && t.auth.modo === 'app_proprio' && t.auth.provedor === 'google') ?? null

export interface VisaoDoCatalogo {
  /** Os serviços do grupo "Google" que a busca deixa ver; o grupo é um cartão só, fora da lista comum. */
  google: Servico[]
  /** Os serviços conectáveis que casam com a busca e a categoria, na ordem do Hub e todos iguais. */
  lista: Servico[]
  /** A mesma `lista`, por categoria. */
  porCategoria: GrupoDeCategoria[]
  emBreve: Servico[]
  /** Quantos serviços a lista mostra agora (com o Google, e conforme a busca e a categoria). */
  total: number
}

export function visaoDoCatalogo(catalogo: Catalogo, busca: string, categoria: string | null): VisaoDoCatalogo {
  const doGoogle = catalogo.servicos.filter(ehDoGoogle)
  const comum = catalogo.servicos.filter((s) => !ehDoGoogle(s))
  const conectaveis = comum.filter((s) => s.emBreve === null)
  const achados = filtrarServicos(conectaveis, busca, categoria)
  const semFiltro = busca.trim() === '' && categoria === null
  const porCategoria = agruparPorCategoria(achados, catalogo.categorias ?? [])
  const google = filtrarServicos(doGoogle, busca, categoria)
  return {
    google,
    lista: porCategoria.flatMap((g) => g.servicos),
    porCategoria,
    emBreve: semFiltro ? comum.filter((s) => s.emBreve !== null) : filtrarServicos(comum.filter((s) => s.emBreve !== null), busca, categoria),
    total: porCategoria.flatMap((g) => g.servicos).length + google.length,
  }
}

/**
 * O cofre está trancado quando o Hub diz isso: a ficha do catálogo traz `contas: null` (`docs/features/marketplace.md`).
 * Qualquer outra falha de `marketContas` (Hub antigo, permissão, link caindo) não diz a causa e não vira "trancado".
 */
export const cofreTrancado = (catalogo: Pick<Catalogo, 'servicos'> | null): boolean => catalogo?.servicos.some((s) => s.contas === null) ?? false

/** A conta expirou: entrou por login e o Hub perdeu a concessão (`signedOut`). */
export const contaExpirada = (c: Pick<Conta, 'auth'>) => c.auth === 'signedOut'

export type EstadoDoServico = { tipo: 'semConta' } | { tipo: 'contas'; n: number } | { tipo: 'reconectar'; n: number }

/** O que o cartão do serviço mostra à direita: Conectar, "N contas" ou Reconectar. */
export function estadoDoServico(servico: Pick<Servico, 'id'>, contas: readonly Conta[]): EstadoDoServico {
  const doServico = contas.filter((c) => c.servico === servico.id)
  if (doServico.length === 0) return { tipo: 'semConta' }
  return doServico.some(contaExpirada) ? { tipo: 'reconectar', n: doServico.length } : { tipo: 'contas', n: doServico.length }
}

// ───────────────────────────── Minhas contas ─────────────────────────────

/** `nome` é `null` no MCP do próprio usuário e nas contas de um serviço que o catálogo não conhece (`foraDoCatalogo`). */
export interface GrupoDeContas { servico: string | null; marca?: string; nome: string | null; contas: Conta[]; foraDoCatalogo?: boolean }

/** As contas agrupadas por serviço, na ordem do catálogo; o MCP do próprio usuário (sem serviço) vai por último. */
export function agruparContas(contas: readonly Conta[], catalogo: Catalogo | null): GrupoDeContas[] {
  const grupos: GrupoDeContas[] = []
  for (const s of catalogo?.servicos ?? []) {
    const dele = contas.filter((c) => c.servico === s.id)
    if (dele.length) grupos.push({ servico: s.id, marca: s.marca, nome: s.nome, contas: dele })
  }
  const conhecidas = new Set(grupos.flatMap((g) => g.contas.map((c) => c.id)))
  const sobras = contas.filter((c) => !conhecidas.has(c.id))
  const semServico = sobras.filter((c) => c.servico === null)
  const desconhecidas = sobras.filter((c) => c.servico !== null)
  for (const c of desconhecidas) grupos.push({ servico: c.servico, nome: null, contas: [c], foraDoCatalogo: true })
  if (semServico.length) grupos.push({ servico: null, nome: null, contas: semServico })
  return grupos
}

/** O jeito pelo qual a conta foi conectada: o `tipo` dela é o id do jeito na ficha (`servico.oficial_oauth`). */
export function jeitoDaConta(conta: Pick<Conta, 'servico' | 'tipo'>, catalogo: Catalogo | null): TipoId | null {
  if (conta.servico === null || conta.tipo === null) return null
  return catalogo?.servicos.find((s) => s.id === conta.servico)?.tipos.find((t) => t.id === conta.tipo)?.tipo ?? null
}

export const botsQueUsam = (conta: Pick<Conta, 'bots'>) => conta.bots.filter((b) => b.ligada)

/** O nome sem o prefixo do serviço: "GitHub – Pessoal" → "Pessoal". Sem prefixo, o nome inteiro. */
export function nomeCurto(nome: string, servico: string): string {
  const resto = nome.replace(new RegExp(`^\\s*${servico.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\s*[–—-]\\s*`, 'i'), '').trim()
  return resto || nome
}

// ───────────────────────────── Contas no bot ─────────────────────────────

export type SeOPedidoNaoDiz =
  | { tipo: 'nenhuma' }
  | { tipo: 'usa'; conta: string }
  | { tipo: 'pergunta' }

export interface ContaNoBot { id: string; nome: string; ligada: boolean; padrao: boolean; expirada: boolean; identidade: string | null }

export interface LinhaDoBot {
  servico: string
  marca?: string
  nome: string
  contas: ContaNoBot[]
  ligadas: ContaNoBot[]
  perguntar: boolean
  seODizNaoPedido: SeOPedidoNaoDiz
  perigosas: Perigosa[]
}

/**
 * O que o Hub faz quando o pedido não cita a conta (host/src/market/contas_escolha.rs): o padrão do bot, senão a única
 * conta ligada, senão pergunta; com "perguntar qual conta" e mais de uma conta, o padrão não vale.
 */
export function seOPedidoNaoDiz(ligadas: readonly ContaNoBot[], perguntar: boolean): SeOPedidoNaoDiz {
  if (ligadas.length === 0) return { tipo: 'nenhuma' }
  if (perguntar && ligadas.length > 1) return { tipo: 'pergunta' }
  const padrao = ligadas.find((c) => c.padrao)
  if (padrao) return { tipo: 'usa', conta: padrao.id }
  if (ligadas.length === 1) return { tipo: 'usa', conta: ligadas[0].id }
  return { tipo: 'pergunta' }
}

/**
 * Uma linha por serviço que tem conta neste Space, na ordem do catálogo (tela 05). `contas` traz `auth` e identidade
 * (`marketContas`); `vinculos` diz o que o bot liga (`marketBotContas`). Conta de outro Space nunca chega aqui.
 */
export function linhasDoBot(catalogo: Catalogo | null, contas: readonly Conta[], vinculos: VinculosDoBot): LinhaDoBot[] {
  const linhas: LinhaDoBot[] = []
  const servicos = catalogo?.servicos ?? []
  const ids = new Set([...servicos.map((s) => s.id), ...vinculos.contas.map((c) => c.servico)])
  for (const id of ids) {
    const doServico = vinculos.contas.filter((c) => c.servico === id)
    if (doServico.length === 0) continue
    const ficha = servicos.find((s) => s.id === id)
    const todas: ContaNoBot[] = doServico.map((v) => {
      const publica = contas.find((c) => c.id === v.conta)
      return { id: v.conta, nome: v.nome, ligada: v.ligada, padrao: v.padraoDoServico, expirada: publica ? contaExpirada(publica) : false, identidade: publica?.identidade ?? null }
    })
    const ligadas = todas.filter((c) => c.ligada)
    const perguntar = vinculos.perguntarConta[id] === true
    const tipos = ficha?.tipos.filter((t) => t.disponivel) ?? []
    const perigosas = tipos.flatMap((t) => t.perigosas)
    linhas.push({ servico: id, ...(ficha ? { marca: ficha.marca } : {}), nome: ficha?.nome ?? id, contas: todas, ligadas, perguntar, seODizNaoPedido: seOPedidoNaoDiz(ligadas, perguntar), perigosas })
  }
  return linhas
}

/** A coluna "Ações perigosas" só existe quando algum serviço da tabela tem alguma (hoje nenhuma ficha tem). */
export const temAcoesPerigosas = (linhas: readonly LinhaDoBot[]) => linhas.some((l) => l.perigosas.length > 0)

/** As ferramentas que a pessoa decide (fatia 2): o que o Hub deixou desligado ou ela ligou; as permitidas não aparecem. */
export const acoesQueAPessoaDecide = (dados: FerramentasDaConta): FerramentaDaConta[] =>
  dados.ferramentas.filter((f) => f.estado !== 'permitida').sort((a, b) => (a.rotulo ?? a.nome).localeCompare(b.rotulo ?? b.nome))

/** Sem o texto lido pelo Hub, ou com texto grande demais ou fora do padrão, não há botão de ligar: o Hub também recusa. */
export const podeLigar = (f: FerramentaDaConta) =>
  f.estado === 'desligada' && f.motivo !== 'descricao_invalida' && f.motivo !== 'descricao_desconhecida' &&
  typeof f.descricao === 'string' && typeof f.apresentacao === 'string'

/** O Hub ainda não leu o texto desta ação (ou ele não veio): a tela diz por que não há botão de ligar. */
export const textoNaoLido = (f: FerramentaDaConta) =>
  f.estado === 'desligada' && f.motivo !== 'descricao_invalida' && (f.motivo === 'descricao_desconhecida' || typeof f.descricao !== 'string' || typeof f.apresentacao !== 'string')

/** Caracteres que não aparecem na tela ou mudam a direção da escrita: a pessoa é avisada antes de ligar. */
export const temCaracteresOcultos = (texto: string) => /[\u00AD\u061C\u180E\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u2069\uFEFF]/u.test(texto)

/** "Pessoal e Estúdio" / "Pessoal, Estúdio e Loja": as contas ligadas, pelo nome curto. */
export function listaDeNomes(nomes: readonly string[], e: string): string {
  if (nomes.length <= 1) return nomes[0] ?? ''
  return `${nomes.slice(0, -1).join(', ')} ${e} ${nomes[nomes.length - 1]}`
}

// ───────────────────────────── Usar em quais bots? (passo 3 de conectar) ─────────────────────────────

/** A busca só aparece a partir daqui: com poucos bots, um campo a mais atrapalha mais do que ajuda. */
export const BOTS_PARA_BUSCAR = 5

/** Um bot do Space como o passo 3 o mostra: quem é (nome, função), em que equipe está e se pode receber a conta. */
export interface EscolhaDeBot {
  id: string
  name: string
  /** A função curta do bot (`Bot.title`); vazia se ele não tem. */
  funcao: string
  /** O caminho da equipe ("Marketing › Social"); `null` fora de equipe ou sem dados de equipes. */
  equipe: string | null
  /** O principal do Space nunca usa contas (o Hub recusa): aparece desabilitado, com o motivo na linha. */
  principal: boolean
}

/** O caminho de equipes até `teamId` ("Marketing › Social"); sem ciclo, e `null` para equipe que a lista não conhece. */
function caminhoDaEquipe(nodes: TimesGraph['nodes'], teamId: string): string | null {
  const porId = new Map(nodes.map((n) => [n.id, n]))
  const nomes: string[] = []
  const vistos = new Set<string>()
  for (let atual = porId.get(teamId); atual && !vistos.has(atual.id); atual = atual.parentId ? porId.get(atual.parentId) : undefined) {
    vistos.add(atual.id)
    nomes.unshift(atual.name)
  }
  return nomes.length > 0 ? nomes.join(' › ') : null
}

/**
 * Junta o que o app já recebe: os bots da conta (id e nome, do Hub), a função de cada um (a lista de bots), a equipe
 * (o grafo de Times do Space) e quem é o principal (`principal.protectedId`, o mesmo critério da tabela "Contas no bot").
 * Nada aqui decide acesso: quem recusa o principal é o Hub; a tela só o mostra desabilitado.
 */
export function escolhasDeBots(
  bots: readonly Pick<BotDaConta, 'id' | 'name'>[],
  funcaoDe: (id: string) => string | undefined,
  times: Pick<TimesGraph, 'nodes' | 'members'> | null,
  principalId: string | null,
): EscolhaDeBot[] {
  const equipes = new Map<string, string | null>()
  const membros = new Map((times?.members ?? []).map((m) => [m.botId, m]))
  const equipeDe = (id: string): string | null => {
    const teamId = membros.get(id)?.teamId
    if (!times || !teamId) return null
    if (!equipes.has(teamId)) equipes.set(teamId, caminhoDaEquipe(times.nodes, teamId))
    return equipes.get(teamId) ?? null
  }
  return bots.map((b) => ({ id: b.id, name: b.name, funcao: funcaoDe(b.id)?.trim() ?? '', equipe: equipeDe(b.id), principal: principalId !== null && b.id === principalId }))
}

/** Um bloco da lista: uma equipe (com título), os sem equipe, ou a lista sem título quando ninguém tem equipe. */
export interface BlocoDeBots {
  chave: string
  titulo: string | null
  /** Os bots deste bloco que a busca deixa ver; o principal só no seu bloco, no fim. */
  bots: EscolhaDeBot[]
  /** O que "Marcar todos do grupo" marca: os visíveis que podem receber a conta (nunca o principal). */
  marcaveis: string[]
}

export const BLOCO_SEM_EQUIPE = '$sem-equipe'
export const BLOCO_PRINCIPAL = '$principal'

/**
 * Agrupa por equipe (na ordem em que os bots chegam) e filtra pela busca: nome, função ou equipe, todas as palavras.
 * Só há títulos quando alguma equipe aparece; sem dados de equipes, a lista é uma só. O principal vem por último.
 */
export function blocosDeBots(escolhas: readonly EscolhaDeBot[], busca: string): BlocoDeBots[] {
  const palavras = searchWords(busca)
  const vistos = escolhas.filter((b) => matchesWords(palavras, [b.name, b.funcao, b.equipe ?? '']))
  const comEquipe = escolhas.some((b) => !b.principal && b.equipe !== null)
  const blocos = new Map<string, BlocoDeBots>()
  const bloco = (chave: string, titulo: string | null) => {
    let b = blocos.get(chave)
    if (!b) blocos.set(chave, (b = { chave, titulo, bots: [], marcaveis: [] }))
    return b
  }
  const ordem: string[] = []
  for (const b of vistos) {
    const chave = b.principal ? BLOCO_PRINCIPAL : !comEquipe ? '$todos' : b.equipe === null ? BLOCO_SEM_EQUIPE : `equipe:${b.equipe}`
    if (!blocos.has(chave)) ordem.push(chave)
    const alvo = bloco(chave, b.principal ? null : b.equipe)
    alvo.bots.push(b)
    if (!b.principal) alvo.marcaveis.push(b.id)
  }
  // Sem equipe e principal sempre no fim, nessa ordem.
  const fim = [BLOCO_SEM_EQUIPE, BLOCO_PRINCIPAL]
  return [...ordem.filter((c) => !fim.includes(c)), ...fim.filter((c) => ordem.includes(c))].map((c) => blocos.get(c)!)
}

/** Marca ou desmarca de uma vez os `ids` (os marcáveis de um bloco): se todos já estão marcados, desmarca. */
export function alternarVarios(marcados: ReadonlySet<string>, ids: readonly string[]): Set<string> {
  const todos = ids.length > 0 && ids.every((id) => marcados.has(id))
  const novo = new Set(marcados)
  for (const id of ids) { if (todos) novo.delete(id); else novo.add(id) }
  return novo
}

/** O que sai para o Hub: nunca o principal, mesmo que algo o tenha marcado (o Hub recusa de qualquer jeito). */
export const botsParaLigar = (marcados: ReadonlySet<string>, escolhas: readonly EscolhaDeBot[]): string[] =>
  escolhas.filter((b) => marcados.has(b.id) && !b.principal).map((b) => b.id)

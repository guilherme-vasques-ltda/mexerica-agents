// DON-907 C9 (D373): as regras do seletor de modelo, sem React.
// (`engine {accountId, backend, model, modelOptions, fastMode, source, revision}`, `sessionEngineSet`).
// Sem texto de tela: os erros saem como chave (`conversa.seletor.*`), traduzida na view.
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import { localeDe } from '../../../lib/fork-i18n/index.ts'

/** Uma opção do modelo (esforço…), do catálogo real do motor (`agentModels`). */
export interface OpcaoModelo { id: string; name: string; values: { id: string; name: string }[]; default?: string | null }

export interface ModeloCatalogo {
  id: string
  name: string
  /** Tamanho da janela em tokens, quando a frente Modelos e contas o informar; ausente, a linha não mostra. */
  contextTokens?: number | null
  options?: OpcaoModelo[]
}

export interface Provedor {
  /** `backend` do Hub (claude, codex…). */
  id: string
  name: string
  /** O registro do ícone do motor (`AgentIcon`); ausente, usa o `id`. */
  registry?: string
  /** `null` enquanto carrega; vazio quando o motor não lista modelos. */
  models: ModeloCatalogo[] | null
  /** O motor oferece o modo rápido. */
  fastMode?: boolean
  /** O id do "Default (recommended)" do próprio motor, fora da lista: é a mesma escolha que "Padrão do motor". */
  padrao?: string
  /** O Hub marcou o motor como "age sem pedir aprovação": o seletor avisa, sem bloquear a escolha. */
  semAprovacao?: boolean
  /** A leitura do catálogo falhou (diferente de "o motor não lista modelos"): o seletor oferece tentar de novo. */
  falhou?: boolean
}

/** Tira da lista o "Default (recommended)" do motor (como o seletor antigo) e devolve o id dele. */
export function separarPadrao(modelos: readonly ModeloCatalogo[]): { models: ModeloCatalogo[]; padrao?: string } {
  const padrao = modelos.find((m) => m.name.toLowerCase().startsWith('default'))
  return { models: modelos.filter((m) => m !== padrao), ...(padrao ? { padrao: padrao.id } : {}) }
}

/** A escolha é o padrão do motor: modelo vazio ou o "Default" dele. */
export const ehPadraoDoMotor = (escolha: Pick<Escolha, 'model'>, provedor: Pick<Provedor, 'padrao'> | undefined) =>
  escolha.model === null || (!!provedor?.padrao && escolha.model === provedor.padrao)

/** O que o seletor escolhe: a projeção `engine` sem `source`/`revision`, que são do servidor. */
export interface Escolha {
  accountId?: string | null
  backend: string
  model: string | null
  modelOptions: Record<string, string>
  fastMode: boolean
}

/**
 * O modo do componente: na conversa grava por `sessionEngineSet`; no criar bot só devolve a escolha; na função de fundo
 * (Ajustes → Bot de fundo) devolve conta e modelo juntos, com o que a função exige em `ConfigFuncao`.
 */
export type ModoSeletor = 'conversa' | 'padraoDoBot' | 'funcaoDeFundo'

/** Uma conta que ainda não serve à função (desconectada, login por confirmar…): fica apagada, com o estado em palavras e a ação que resolve. */
export interface ContaInativa {
  id: string
  label: string
  backend: string
  /** O estado já no idioma ("desconectada", "login não confirmado"). */
  estado: string
  acao?: { rotulo: string; fazer: () => void }
}

/**
 * O que a função de fundo muda no seletor (modo `funcaoDeFundo`); os textos já vêm no idioma de quem chama. As contas que
 * servem vão em `contas` (`CadastroContas`), só as conectadas e aceitas pela função: o seletor nunca escolhe outra.
 */
export interface ConfigFuncao {
  /** Motores que a função não aceita: o motivo curto (no trilho) e o inteiro (na aba, nas linhas e na dica). */
  recusados: Readonly<Record<string, { curto: string; motivo: string }>>
  /** Contas que não servem agora: ficam na linha Conta, apagadas, com o estado e a ação ao lado. */
  inativas: readonly ContaInativa[]
  /** O motor em que "modelo vazio" é uma escolha (Automático); nos outros só há modelos. */
  automatico: { backend: string; nome: string; etiqueta?: string; meta?: string } | null
  /** O texto do rodapé. */
  nota: string
  /** Nenhuma conta do motor serve (a linha das contas e o motivo das linhas). */
  semConta: (motor: string) => string
  /** Outra conta marcada no motor da escolha: o que acontece ao escolher um modelo. */
  trocarDica: (conta: string) => string
  /** O Automático não vale nesta conta (o Hub não resolve o modelo leve dela): o motivo da linha apagada. */
  automaticoFora: (conta: string) => string
}

/**
 * Na função de fundo cada escolha leva conta e modelo juntos: não existe "mesmo motor" (o motor da escolha salva só decide
 * a aba que abre e a conta que já vem marcada). A conta marcada é a que a pessoa tocou; sem toque, a da escolha salva.
 */
export const marcadaDaFuncao = (valor: Pick<Escolha, 'backend' | 'accountId'>, backend: string, marcadas: Readonly<Record<string, string>>): string | null =>
  marcadas[backend] ?? (valor.backend === backend ? valor.accountId ?? null : null)

/** A aba com que o seletor da função abre: o motor da escolha, ou o primeiro que a função aceita (nunca um recusado). */
export const abaInicialDaFuncao = (backend: string, provedores: readonly Pick<Provedor, 'id'>[], recusados: ConfigFuncao['recusados']): string =>
  provedores.some((p) => p.id === backend && !recusados[p.id]) ? backend : (provedores.find((p) => !recusados[p.id])?.id ?? backend)

/** Os favoritos guardam `backend/modelo`. */
export const chave = (backend: string, model: string) => `${backend}/${model}`

/** Liga ou desliga a estrela; um favorito novo vai para o fim (os três primeiros são ⌘1–⌘3). */
export function alternarFavorito(favoritos: readonly string[], backend: string, model: string): string[] {
  const k = chave(backend, model)
  return favoritos.includes(k) ? favoritos.filter((f) => f !== k) : [...favoritos, k]
}

/** O favorito `backend/modelo` no catálogo carregado. */
function achar(k: string | undefined, provedores: readonly Provedor[]): { provedor: Provedor; modelo: ModeloCatalogo } | null {
  for (const provedor of provedores) {
    const modelo = k ? provedor.models?.find((m) => chave(provedor.id, m.id) === k) : undefined
    if (modelo) return { provedor, modelo }
  }
  return null
}

/** O modelo do atalho ⌘`n` (1–3): o `n`-ésimo favorito, se ainda está no catálogo. */
export const atalho = (favoritos: readonly string[], provedores: readonly Provedor[], n: number) =>
  n >= 1 && n <= 3 ? achar(favoritos[n - 1], provedores) : null

const norma = (s: string) => s.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase()

/** A busca olha nome e id, sem diferença de maiúsculas nem acentos. */
export function filtrar(modelos: readonly ModeloCatalogo[], busca: string): ModeloCatalogo[] {
  const q = norma(busca.trim())
  return q ? modelos.filter((m) => norma(m.name).includes(q) || norma(m.id).includes(q)) : [...modelos]
}

/** Por lista de modelos (uma por motor): o texto já normalizado de cada modelo e o resultado da última busca. */
const memoFiltro = new WeakMap<readonly ModeloCatalogo[], { textos: string[]; q: string; achados: readonly ModeloCatalogo[] }>()

/**
 * `filtrar` em memo por motor: a lista do motor é normalizada uma vez só e a mesma busca devolve o mesmo vetor (a
 * lista de quem chama não se refaz). Sem texto, devolve a própria lista. Vale enquanto a lista for a mesma (a do
 * catálogo guardado); não a altere.
 */
export function filtrarMemo(modelos: readonly ModeloCatalogo[], busca: string): readonly ModeloCatalogo[] {
  const q = norma(busca.trim())
  if (!q) return modelos
  let memo = memoFiltro.get(modelos)
  if (!memo) memoFiltro.set(modelos, memo = { textos: modelos.map((m) => `${norma(m.name)}\n${norma(m.id)}`), q: '', achados: modelos })
  if (memo.q !== q) {
    const { textos } = memo
    memo.achados = modelos.filter((_, i) => textos[i]!.includes(q))
    memo.q = q
  }
  return memo.achados
}

/** Os favoritos que existem no catálogo carregado, na ordem guardada, com o atalho ⌘1–⌘3 dos três primeiros. */
export function favoritosNoCatalogo(favoritos: readonly string[], provedores: readonly Provedor[]) {
  return favoritos.flatMap((k, i) => {
    const achado = achar(k, provedores)
    return achado ? [{ ...achado, atalho: i < 3 ? i + 1 : null }] : []
  })
}

/** A opção de esforço do modelo, se o catálogo a tiver (nunca inventada). */
export const opcaoEsforco = (modelo: ModeloCatalogo | null | undefined) =>
  modelo?.options?.find((o) => o.id === 'effort' || o.id === 'reasoning_effort' || /esfor|racioc|effort|reason/i.test(o.name)) ?? null

/** Trocar a partir de quantos tokens a reler pede confirmação (D375, coordenação: absoluto, não percentual). */
export const LIMIAR_CONFIRMAR_TOKENS = 50_000

/**
 * A troca pede confirmação? Mesmo provedor: relê a conversa inteira (`used` do medidor). Outro provedor: só a
 * abertura e a passagem, que o app não mede; troca direto. Sem medida, troca direto (não inventa).
 */
export function precisaConfirmar(atual: Pick<Escolha, 'backend' | 'model'>, nova: Pick<Escolha, 'backend' | 'model'>, usado: number | null): boolean {
  if (atual.backend !== nova.backend || atual.model === nova.model) return false
  return typeof usado === 'number' && usado >= LIMIAR_CONFIRMAR_TOKENS
}

/** Um formatador compacto por idioma e os tamanhos já formatados (criar `Intl.NumberFormat` a cada linha custa caro). */
const compactos = new Map<AppLanguage, { formato: Intl.NumberFormat; prontos: Map<number, string> }>()

/** O tamanho da janela no formato compacto do idioma ("1 mi", "400 mil", "1M"); `null` sem número. */
export function tamanhoContexto(idioma: AppLanguage, tokens: number | null | undefined): string | null {
  if (typeof tokens !== 'number' || !(tokens > 0)) return null
  let c = compactos.get(idioma)
  if (!c) compactos.set(idioma, c = { formato: new Intl.NumberFormat(localeDe(idioma), { notation: 'compact', maximumFractionDigits: 1 }), prontos: new Map() })
  let texto = c.prontos.get(tokens)
  if (texto === undefined) c.prontos.set(tokens, texto = c.formato.format(tokens))
  return texto
}

/** O chip: o nome do modelo (ou só o motor, quando o bot usa o padrão dele) e o esforço escolhido ou o padrão. */
export function rotuloChip(escolha: Escolha, provedores: readonly Provedor[]): { modelo: string | null; motor: string; esforco: string | null } {
  const provedor = provedores.find((p) => p.id === escolha.backend)
  const modelo = provedor?.models?.find((m) => m.id === escolha.model) ?? null
  const opcao = opcaoEsforco(modelo)
  const valor = opcao ? (escolha.modelOptions[opcao.id] ?? opcao.default ?? null) : null
  return {
    modelo: ehPadraoDoMotor(escolha, provedor) ? null : (modelo?.name ?? escolha.model),
    motor: provedor?.name ?? escolha.backend,
    esforco: valor ? (opcao!.values.find((v) => v.id === valor)?.name ?? valor) : null,
  }
}

/** A projeção `engine` de uma sessão, se o Hub a mandar e ela for legível. */
export function engineDaSessao(session: unknown): (Escolha & { revision: number; source: string }) | null {
  const e = (session as { engine?: unknown } | null)?.engine as Partial<Escolha & { revision: number; source: string }> | undefined
  if (!e || typeof e !== 'object' || typeof e.backend !== 'string' || typeof e.revision !== 'number') return null
  return {
    accountId: e.accountId ?? null, backend: e.backend, model: e.model ?? null, modelOptions: e.modelOptions ?? {},
    fastMode: e.fastMode === true, revision: e.revision, source: e.source ?? 'botDefault',
  }
}

/** O erro de `sessionEngineSet` como chave do catálogo; o texto do Hub nunca vai à tela. */
export function erroDaTroca(error: unknown) {
  const message = (error as { message?: unknown } | null)?.message
  const code = typeof message === 'string' ? /^[a-z_]+/.exec(message)?.[0] : undefined
  switch (code) {
    case 'engine_not_allowed': return 'conversa.seletor.erro.naoPermitido' as const
    case 'engine_owner_only': return 'conversa.seletor.erro.soDono' as const
    case 'session_revision_conflict': return 'conversa.seletor.erro.conflito' as const
    case 'session_closed': return 'conversa.seletor.erro.encerrada' as const
    case 'engine_unavailable': return 'conversa.seletor.erro.indisponivel' as const
    default: return 'conversa.seletor.erro.falhou' as const
  }
}

/** Uma conta do cadastro do Hub (`donnaEngineAccounts`), só os campos que a troca de provedor usa. */
export interface ContaMotor {
  id: string
  label: string
  backend: string
  state: string
  origin: string
  models: readonly { id: string }[]
  /** Entrega 3: os modelos que a pessoa ocultou nesta conta (Contas de IA); ausente ou vazio, nenhum. */
  hiddenModelIds?: readonly string[]
  /** Só na função de fundo: o Automático (modelo vazio) tem receita que o Hub resolve nesta conta; `false` o bloqueia. Ausente: serve (conversa). */
  automatico?: boolean
}

/**
 * D372 (R8): as contas do cadastro que podem receber a conversa noutro motor: gerenciadas, do motor e conectadas, na
 * ordem do Hub. A conta legada sem id próprio não serve: a troca sem conta só existe no mesmo motor.
 */
export const contasDoMotor = (contas: readonly ContaMotor[], backend: string) =>
  contas.filter((c) => c.origin === 'managed' && c.backend === backend && c.state === 'connected')

/** A conta destino de uma troca para `backend`. */
export type Destino =
  /** Mesmo motor: a conta atual da conversa continua. */
  | { tipo: 'mesmo' }
  /** O cadastro ainda está sendo lido. */
  | { tipo: 'carregando' }
  /** A leitura do cadastro falhou: nenhuma troca entre motores (sem chute); reabrir o seletor tenta de novo. */
  | { tipo: 'falhou' }
  /** Nenhuma conta conectada desse motor (ou o cadastro não pôde ser lido): o motor fica indisponível. */
  | { tipo: 'indisponivel' }
  /** Várias contas e nenhuma marcada: a pessoa escolhe; nunca a primeira por conta própria. */
  | { tipo: 'escolher'; contas: ContaMotor[] }
  /** A conta que vai no pedido: a única elegível (visível) ou a marcada. */
  | { tipo: 'conta'; conta: ContaMotor; contas: ContaMotor[] }

/** O cadastro como o seletor o recebe: a lista lida, `'falhou'` (só até fechar o seletor) ou `null` enquanto lê. */
export type CadastroContas = readonly ContaMotor[] | 'falhou' | null

/** `contas`: o cadastro (`CadastroContas`); `marcada`: a conta que a pessoa marcou para esse motor. */
export function destinoDaTroca(atual: string, backend: string, contas: CadastroContas, marcada: string | null): Destino {
  if (backend === atual) return { tipo: 'mesmo' }
  if (contas === 'falhou') return { tipo: 'falhou' }
  if (!contas) return { tipo: 'carregando' }
  const elegiveis = contasDoMotor(contas, backend)
  if (!elegiveis.length) return { tipo: 'indisponivel' }
  const conta = elegiveis.length === 1 ? elegiveis[0]! : elegiveis.find((c) => c.id === marcada)
  return conta ? { tipo: 'conta', conta, contas: elegiveis } : { tipo: 'escolher', contas: elegiveis }
}

/**
 * O modelo cabe na conta? Só filtra quando a conta traz a lista de modelos: lista ausente ou vazia não filtra nada (o
 * Hub continua decidindo). O "Padrão do motor" (modelo vazio) sempre cabe.
 */
export const modeloNaConta = (conta: Pick<ContaMotor, 'models' | 'automatico'> | null | undefined, model: string | null) =>
  model === null ? conta?.automatico !== false : !conta?.models?.length || conta.models.some((m) => m.id === model)

/** Entrega 3: o modelo foi ocultado nesta conta (Contas de IA → Ocultar). Sem conta ou sem a lista, nada é oculto. */
export const ocultoNaConta = (conta: Pick<ContaMotor, 'hiddenModelIds'> | null | undefined, model: string) =>
  !!conta?.hiddenModelIds?.includes(model)

/**
 * Entrega 3: a conta cujos modelos a aba de um motor mostra. A marcada na aba vence; no motor da conversa, a conta
 * dela; noutro motor, a única elegível. Com várias e nenhuma marcada, `null` (a pessoa escolhe).
 */
export function contaAlvo(atual: Pick<Escolha, 'backend' | 'accountId'>, backend: string, contas: CadastroContas, marcada: string | null): ContaMotor | null {
  if (!contas || contas === 'falhou') return null
  const elegiveis = contasDoMotor(contas, backend)
  const escolhida = marcada ? elegiveis.find((c) => c.id === marcada) : undefined
  if (escolhida) return escolhida
  if (backend === atual.backend) return (atual.accountId ? contas.find((c) => c.id === atual.accountId && c.backend === backend) : undefined) ?? null
  return elegiveis.length === 1 ? elegiveis[0]! : null
}

/**
 * Entrega 3: o que some do seletor num motor, resolvido uma vez por motor (não por linha). Com a conta alvo conhecida,
 * vale o oculto dela; sem ela (várias contas, nenhuma marcada), só some o que está oculto em todas. Sem cadastro, nada some.
 */
export function ocultosDoMotor(atual: Pick<Escolha, 'backend' | 'accountId'>, backend: string, contas: CadastroContas, marcada: string | null): (model: string) => boolean {
  const nada = () => false
  if (!contas || contas === 'falhou') return nada
  const alvo = contaAlvo(atual, backend, contas, marcada)
  const elegiveis = alvo ? [alvo] : backend !== atual.backend ? contasDoMotor(contas, backend) : []
  if (!elegiveis.length || elegiveis.some((c) => !c.hiddenModelIds?.length)) return nada
  const conjuntos = elegiveis.map((c) => new Set(c.hiddenModelIds))
  return (model) => conjuntos.every((c) => c.has(model))
}

/** Entrega 3: o modelo some do seletor? (`ocultosDoMotor` para um modelo só.) */
export const modeloOculto = (atual: Pick<Escolha, 'backend' | 'accountId'>, backend: string, model: string, contas: CadastroContas, marcada: string | null): boolean =>
  ocultosDoMotor(atual, backend, contas, marcada)(model)

/** O motivo de o modelo (ou o Automático, `model` nulo) não caber na conta. */
const foraDaConta = (conta: Pick<ContaMotor, 'label'>, model: string | null): MotivoTroca => ({ chave: model === null ? 'conversa.seletor.conta.automaticoFora' : 'conversa.seletor.conta.modeloFora', conta: conta.label })

/** A escolha noutro motor com a conta destino explícita; `null` quando o modelo não cabe nela (ver `modeloNaConta`). */
export function escolhaCruzada(conta: ContaMotor, backend: string, model: string | null): Escolha | null {
  if (!modeloNaConta(conta, model)) return null
  return { accountId: conta.id, backend, model, modelOptions: {}, fastMode: false }
}

/** O corpo de `sessionEngineSet`: as cinco chaves sempre presentes; `sessionId` `null` é a principal. */
export function corpoTroca(botId: string, sessionId: string | null, expectedRevision: number, escolha: Escolha) {
  return {
    botId, sessionId, expectedRevision,
    engine: { accountId: escolha.accountId ?? null, backend: escolha.backend, model: escolha.model, modelOptions: escolha.modelOptions, fastMode: escolha.fastMode },
  }
}

/** O motivo, em chave do catálogo, de uma troca que ainda não pode ser enviada. */
export type MotivoTroca =
  | { chave: 'conversa.seletor.conta.escolha' | 'conversa.seletor.conta.carregando' | 'conversa.seletor.conta.nenhuma'; motor: string }
  | { chave: 'conversa.seletor.conta.modeloFora' | 'conversa.seletor.conta.automaticoFora'; conta: string }
  | { chave: 'conversa.seletor.conta.falhou' }

/** O que um clique (lista, favorito ou atalho) num modelo faz no modo conversa com o cadastro de contas. */
export type IntencaoTroca =
  /** Mesmo motor e cabe na conta atual: segue a escolha de sempre. */
  | { tipo: 'mesmo' }
  /** Outro motor ainda não exibido: primeiro mostra a aba e a conta, sem pedido (N1). */
  | { tipo: 'navegar'; motivo: MotivoTroca | null }
  /** Aba exibida, mas falta conta ou o modelo não cabe: só o motivo, sem pedido. */
  | { tipo: 'esperar'; motivo: MotivoTroca }
  /** Aba e conta visíveis: o pedido com a conta destino explícita. */
  | { tipo: 'emitir'; escolha: Escolha }

/**
 * D372 (R8/N1): o pedido só sai quando a aba do motor e a conta destino já estavam visíveis antes do clique. Um
 * favorito ou atalho de outro motor fora da aba exibida só navega (mostrando o motivo, se houver).
 */
export function intencaoDeTroca(o: {
  atual: Pick<Escolha, 'backend' | 'accountId'>
  aba: string
  provedor: Pick<Provedor, 'id' | 'name'>
  model: string | null
  contas: CadastroContas
  marcada: string | null
}): IntencaoTroca {
  const destino = destinoDaTroca(o.atual.backend, o.provedor.id, o.contas, o.marcada)
  if (destino.tipo === 'mesmo') {
    // Entrega 3: no mesmo motor, outra conta marcada na aba exibida vai explícita no pedido (a conta troca junto).
    const outra = o.marcada && o.marcada !== o.atual.accountId && o.aba === o.provedor.id && Array.isArray(o.contas) ? contasDoMotor(o.contas, o.provedor.id).find((c) => c.id === o.marcada) : undefined
    if (outra) {
      const escolha = escolhaCruzada(outra, o.provedor.id, o.model)
      return escolha ? { tipo: 'emitir', escolha } : { tipo: 'esperar', motivo: foraDaConta(outra, o.model) }
    }
    const conta = o.atual.accountId && Array.isArray(o.contas) ? o.contas.find((c) => c.id === o.atual.accountId) : undefined
    return modeloNaConta(conta, o.model) || !conta ? { tipo: 'mesmo' } : { tipo: 'esperar', motivo: foraDaConta(conta, o.model) }
  }
  const motivo: MotivoTroca | null = destino.tipo === 'conta'
    ? (modeloNaConta(destino.conta, o.model) ? null : foraDaConta(destino.conta, o.model))
    : destino.tipo === 'falhou' ? { chave: 'conversa.seletor.conta.falhou' }
    : { chave: destino.tipo === 'escolher' ? 'conversa.seletor.conta.escolha' : destino.tipo === 'carregando' ? 'conversa.seletor.conta.carregando' : 'conversa.seletor.conta.nenhuma', motor: o.provedor.name }
  if (o.aba !== o.provedor.id) return { tipo: 'navegar', motivo }
  if (motivo || destino.tipo !== 'conta') return { tipo: 'esperar', motivo: motivo! }
  return { tipo: 'emitir', escolha: escolhaCruzada(destino.conta, o.provedor.id, o.model)! }
}

/** Os efeitos de uma intenção de troca, na ordem em que o seletor os aplica. */
export interface EfeitosTroca {
  /** Mostra a aba do motor (e o campo Conta) com o motivo, se houver; nunca emite. */
  navegar: (motor: string, motivo: MotivoTroca | null) => void
  /** Só o motivo; nunca emite. */
  avisar: (motivo: MotivoTroca) => void
  /** Emite a escolha (outro motor com conta destino, ou o caminho direto do mesmo motor). */
  emitir: (escolha: Escolha) => void
}

/**
 * Aplica a intenção: no máximo uma emissão por ação. Mesmo motor segue direto (`direta`, a escolha de sempre, com a
 * confirmação D375 de quem chama); outro motor só emite com a aba e a conta já visíveis (N1).
 */
export function aplicarIntencao(i: IntencaoTroca, motor: string, direta: () => Escolha, efeitos: EfeitosTroca) {
  if (i.tipo === 'navegar') efeitos.navegar(motor, i.motivo)
  else if (i.tipo === 'esperar') efeitos.avisar(i.motivo)
  else efeitos.emitir(i.tipo === 'emitir' ? i.escolha : direta())
}

/**
 * D372 (R8): o cadastro de contas que o Chip passa ao seletor, preso ao link/época (`vinculo`). Só a lista lida fica
 * guardada, e cada abertura a relê; uma falha vale até fechar o seletor e reabrir tenta de novo. Resposta de outro pedido, link ou época
 * (tardia) é descartada.
 */
export class LeituraContas<C extends object> {
  private guardado: { client: C; epoch: number; estado: readonly ContaMotor[] | 'falhou' } | null = null
  private pedido = 0
  private readonly vinculo: () => { client: C | null; epoch: number }
  private readonly ler: (client: C) => Promise<readonly ContaMotor[]>
  private readonly mudou: () => void
  constructor(vinculo: () => { client: C | null; epoch: number }, ler: (client: C) => Promise<readonly ContaMotor[]>, mudou: () => void) {
    this.vinculo = vinculo
    this.ler = ler
    this.mudou = mudou
  }

  /** O cadastro do link/época atuais: a lista, `'falhou'` ou `null` (lendo, ou ainda não lido). */
  estado(): CadastroContas {
    const { client, epoch } = this.vinculo()
    const g = this.guardado
    return g && g.client === client && g.epoch === epoch ? g.estado : null
  }

  /**
   * Seletor aberto (ou outro link/época com ele aberto): lê sempre. O cadastro muda fora daqui (Contas de IA oculta um
   * modelo, conecta ou remove uma conta), então reabrir relê. A lista já lida deste link/época continua à mostra
   * enquanto a nova não chega, e fica se a releitura falhar; sem lista, a falha aparece.
   */
  abrir() {
    const { client, epoch } = this.vinculo()
    if (!client) return
    const g = this.guardado
    const id = ++this.pedido
    if (g?.estado === 'falhou') {
      this.guardado = null
      this.mudou()
    }
    void this.ler(client).then((lista) => [...lista], () => 'falhou' as const).then((estado) => {
      const agora = this.vinculo()
      if (id !== this.pedido || agora.client !== client || agora.epoch !== epoch) return
      const antes = this.guardado
      if (estado === 'falhou' && antes && antes.client === client && antes.epoch === epoch && antes.estado !== 'falhou') return
      this.guardado = { client, epoch, estado }
      this.mudou()
    })
  }

  /** Seletor fechado: um pedido em voo é descartado e uma falha não fica guardada. */
  fechar() {
    this.pedido++
    if (this.guardado?.estado === 'falhou') this.guardado = null
  }
}

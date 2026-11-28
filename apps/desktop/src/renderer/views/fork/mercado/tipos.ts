// Marketplace de conectores (fatia 1b): o formato do que o Hub devolve (`docs/features/marketplace.md`, "API do
// Marketplace"). O app só lê e renderiza; nome, Space, escopos e liberações são decididos e conferidos no Hub.

export type TipoId = 'oficial_oauth' | 'oficial_chave' | 'oficial_app' | 'local'

export type AuthDoTipo =
  | { modo: 'oauth'; registro: string }
  | { modo: 'chave'; destino: 'header' | 'env'; nome: string; /** O que vem antes da chave no cabeçalho (ex.: `Bearer `). */ prefixo?: string | null; comoCriarLeitura: string }
  /** O app OAuth da própria pessoa (Google): `provedor` agrupa os serviços; `guia` são os passos para criar o app. */
  | { modo: 'app_proprio'; provedor: string; emissor: string; guia: PassoDoGuia[]; configuracao?: { cadastro: string } }

/** Um passo do guia "crie o seu app": o `id` escolhe o texto; `url` é o link oficial, quando há. */
export interface PassoDoGuia { id: string; url: string | null }

/** O app Google do Space como o Hub o mostra: só o ID. A chave secreta nunca volta. */
export interface AppProprio { configurado: boolean }
export interface AppGoogle extends AppProprio { clientId: string | null }

/** Um bloco de permissão da ficha ("Ler", "Criar e editar"). */
export interface Grupo {
  id: string
  rotulo: string
  escopos: string[]
  url: string | null
  ferramentas: string[]
  /** Pré-marcado ao conectar: o padrão seguro. */
  padrao: boolean
  disponivel: boolean
  /** Limite de leitura do fornecedor ou liberação de ações pelo filtro do Hub. */
  imposicao?: 'url' | 'escopo' | 'natureza' | 'filtro' | null
}

/** Ação que só a pessoa liga, com confirmação (fatia 2). Hoje a ficha só a lista. */
export interface Perigosa { ferramenta: string; rotulo: string }

export interface TipoConexao {
  id: string
  tipo: TipoId
  rotulo: string
  mantidoPor: string
  url: string
  auth: AuthDoTipo
  grupos: Grupo[]
  perigosas: Perigosa[]
  recomendado: boolean
  disponivel: boolean
  motivoIndisponivel: string | null
}

export interface ContaResumo { id: string; name: string; identidade: string | null }

/** Uma skill do catálogo curado que a ficha sugere (só texto; não liga sozinha). */
export interface SkillSugerida { id: string; titulo: string; resumo: string }

/** Skill que o fornecedor publica: a ficha só mostra o link e a licença. */
export interface SkillOficial { url: string; origem: 'fornecedor' | 'skills_sh' | string; licenca: string; aproveitavel: boolean }

export interface Servico {
  id: string
  /** A chave da logo do serviço no app. */
  marca: string
  nome: string
  categoria: string
  descricao: string
  exemplos: string[]
  /** Página do fornecedor e documentação do conector dele (Hub antigo não manda: opcionais). */
  site?: string
  docs?: string | null
  links: { privacidade: string; termos: string }
  /** `AAAA-MM-DD`. */
  conferidaEm: string
  fontes: string[]
  aviso: string | null
  tipos: TipoConexao[]
  skills: SkillSugerida[]
  skillsOficiais?: SkillOficial[]
  emBreve: string | null
  conectavel: boolean
  conferenciaVigente: boolean
  diasDesdeConferencia: number
  /** As contas do Space; `null` com o cofre trancado. */
  contas: ContaResumo[] | null
}

export interface Catalogo {
  versao: number
  prazoConferenciaDias: number
  /** As categorias na ordem do Hub (lista fechada). Hub antigo não manda: a tela usa a ordem em que aparecem. */
  categorias?: string[]
  hoje: string
  spaceId: string
  servicos: Servico[]
  /** O app OAuth da pessoa por Space; `google` é `null` com o cofre trancado (e ausente em Hub antigo). */
  appsProprios?: Record<string, AppProprio | null> & { google: AppGoogle | null }
  oauthRedirectUris?: string[]
}

export interface BotDaConta { id: string; name: string; ligada: boolean; padraoDoServico: boolean }

/** Uma conta do Space como `marketContas` a devolve (`auth`: `none`, `signedIn` ou `signedOut` = expirou). */
export interface Conta {
  id: string
  name: string
  servico: string | null
  tipo: string | null
  liberacoes: string[]
  space: string
  identidade: string | null
  auth: 'none' | 'signedIn' | 'signedOut' | string
  origem: 'catalogo' | 'usuario' | string
  naoVerificado: boolean
  bots: BotDaConta[]
}

export interface VinculoConta { conta: string; nome: string; servico: string; ligada: boolean; padraoDoServico: boolean }

export interface VinculosDoBot {
  botId: string
  spaceId: string
  contas: VinculoConta[]
  perguntarConta: Record<string, boolean>
}

export type ProximoPasso = 'entrar' | 'chave'

export interface ResultadoDesconectar {
  removed: boolean
  bots: { id: string; name: string }[]
  /** O Hub nunca diz que revogou no fornecedor: hoje é sempre `false`. */
  revogadoNoFornecedor: boolean
}

export interface PedidoDefinirVinculo {
  botId: string
  ligar?: string[]
  desligar?: string[]
  padrao?: Record<string, string | null>
  perguntar?: Record<string, boolean>
}

/**
 * Fatia 2: as ferramentas de uma conta para um bot (`marketFerramentas`). `permitida`: passa sem liberação;
 * `ligada`: a pessoa ligou neste bot; `desligada`: perigosa, nova ou alterada pelo fornecedor, só a pessoa liga.
 */
export type EstadoDaFerramenta = 'permitida' | 'ligada' | 'desligada'
export type MotivoDaFerramenta = 'perigosa' | 'nova' | 'alterada' | 'descricao_invalida' | 'descricao_desconhecida'
/** As partes de uma ação que o fornecedor pode mudar depois que a pessoa a ligou. */
export type ParteDaFerramenta = 'descricao' | 'entrada' | 'titulo' | 'saida' | 'anotacoes'

export interface FerramentaDaConta {
  nome: string
  /** O rótulo da ficha (`Reembolsar`); sem ele, a tela mostra o nome técnico. */
  rotulo: string | null
  estado: EstadoDaFerramenta
  motivo: MotivoDaFerramenta | null
  /** A impressão digital que a pessoa viu: o Hub só liga a ferramenta se ela ainda for esta. */
  digest: string
  /** O texto que o modelo lê sobre a ação, já passado pelo filtro do Hub; `null` enquanto o Hub não o leu. */
  descricao: string | null
  /** O texto que o modelo lê em cada campo da ação, como `[campo, texto]`. */
  campos: [string, string][] | null
  /** A impressão do texto mostrado (descrição e campos): o app a devolve ao ligar, e o Hub só liga se ainda for esta. */
  apresentacao: string | null
  /** Ligada antes e mudada pelo fornecedor: o que mudou (vazio quando o Hub não sabe dizer). */
  mudou: ParteDaFerramenta[] | null
}

export interface FerramentasDaConta {
  botId: string
  conta: string
  /** `false` até o Hub ver a lista completa de ferramentas da conta (verificação ou primeiro uso). */
  inventarioPronto: boolean
  /** A época do vínculo bot–conta que a pessoa viu: o Hub recusa a confirmação se a conta saiu do bot desde então. */
  vinculo: number
  ferramentas: FerramentaDaConta[]
}

export interface PedidoLigarFerramenta { botId: string; conta: string; ferramenta: string; digest: string; vinculo: number; apresentacao: string; confirmo: true }

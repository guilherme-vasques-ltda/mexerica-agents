/** D169 v1: contextos persistidos do mesmo dono/Hub. Não são tenants ou grants. */
export const SPACES_CONTRACT = 1 as const
export const SPACE_ICON_IDS = ['briefcase', 'building', 'chart', 'target', 'rocket', 'scale', 'contract', 'globe', 'bag', 'cart', 'store', 'boxes', 'tag', 'truck', 'card', 'megaphone', 'code', 'chip', 'cloud', 'zap', 'database', 'bot', 'user', 'home', 'leaf', 'heart', 'book', 'coffee', 'paw', 'dumbbell'] as const
export type SpaceColor = 'violeta' | 'cobre' | 'musgo' | 'oceano' | 'ameixa' | 'grafite'
export type SpaceSymbol =
  | { kind: 'icon'; value: string }
  | { kind: 'letters'; value: string }
  /** Somente data URL devolvida/validada pelo Hub; nunca URL ou path externo. */
  | { kind: 'logo'; value: string }
export interface SpaceTile { color: SpaceColor; style: 'filled' | 'soft'; symbol: SpaceSymbol }
export interface SpaceAppearance {
  wallpaper: 'none' | 'serra' | 'duna' | 'mare' | 'noite' | 'cidade' | 'risca' | 'mogno'
  blur: number
  transparency: number
  veilLight: number
  veilDark: number
}
/** Apresentação escolhida pelo dono. Ausência continua ausência, sem inferir provedor. */
export interface OwnerPresentation { name: string; initials: string; photo: string | null }
export interface Space {
  id: string
  kind: 'personal' | 'space'
  name: string
  position: number
  revision: number
  /** Ausente em Spaces legados; nunca inferir o principal do Pessoal. */
  principalBotId?: string | null
  tile: SpaceTile | null
  /** false no Pessoal migrado: conservar preferências locais até importação explícita. */
  appearanceConfigured: boolean
  appearance: SpaceAppearance
  owner: OwnerPresentation | null
}
export interface SpacesSnapshot {
  version: typeof SPACES_CONTRACT
  personalSpaceId: string
  revision: number
  spaces: Space[]
}
export interface SpaceScope { hubId: string; spaceId: string }
/** Revisão CAS da coleção; nunca timestamp inferido de outra entidade. */
export interface SpaceCreate { name: string; tile: SpaceTile; expectedRevision: number }
export interface SpaceBirth extends SpaceCreate {
  clientNonce: string
  principal: { name: string; backend: import('./fork-engine-accounts').Engine; engineAccountId: string }
}
/** D543 E1: aparência Simples do Berga por Space — peças do editor de avatar, sem arte nova.
 *  `modeloId` fica ausente: as figuras são personalidades, não os modelos nomeados. */
export interface SpacePrincipalAppearance {
  v: 2
  estilo: 'simples'
  aparencia: import('./fork-mexeriquinha-config').AparenciaMexeriquinha
}
/** Ativação do Berga num Space legado sem principal; o nome é fixo no Hub ("Berga"). */
export interface SpacePrincipal extends SpaceScope {
  expectedRevision: number
  clientNonce: string
  principal: {
    name?: string
    backend: import('./fork-engine-accounts').Engine
    engineAccountId: string
    appearance?: SpacePrincipalAppearance
  }
}
/** Recusa de exclusão por bot ocupado: só os nomes, nunca ids nem atividade interna. */
export interface SpaceDeleteBusy { busy: { botId: string; name: string }[] }
export interface SpaceDelete extends SpaceScope {
  expectedRevision: number
  confirmName: string
}
export type SpaceDeleteResult = SpacesSnapshot | SpaceDeleteBusy
export interface SpaceUpdate extends SpaceScope {
  expectedRevision: number
  patch: { name?: string; tile?: SpaceTile; appearance?: SpaceAppearance }
}
export interface SpaceReorder { expectedRevision: number; ids: string[] }
export interface SpaceConversation {
  id: string
  spaceId: string
  botId: string
  title: string
  revision: number
  /** Revisão de projeção/eventos; leitura humana e continuidade não alteram o CAS do binding. */
  stateRevision: number
  primary: boolean
  closed: boolean
  /** Marca de leitura do cliente humano; não prova que o bot leu/consumiu. */
  readRevision: number
  /** Referência explícita ao recurso do Hub. null impede execução fora do legado Pessoal. */
  engineAccountId: string | null
  engineSelection: { backend: string; model: string | null; modelOptions: Record<string, string> } | null
  accountRevision: number | null
  continuity: { mode: 'fresh' | 'fresh_reconstructed'; evidence: 'prepared_by_hub'; coverage: HistoryCoverage | null } | null
}
export interface ConversationScope extends SpaceScope { conversationId: string }
export interface ConversationSend extends ConversationScope {
  expectedRevision: number
  text: string
  clientNonce: string
  attachments?: string[]
  replyTo?: string
}
export interface SpaceContext {
  version: typeof SPACES_CONTRACT
  spaceId: string
  conversationId: string
  botId: string
  conversationRevision: number
}
export interface SpaceEvent {
  type: 'spaces' | 'conversation'
  spaceId: string
  rev: number
  snapshot?: SpacesSnapshot
  conversation?: SpaceConversation
}

export interface HistoryCoverage {
  kind: 'bounded_visible_messages'
  includesAttachments: false
  includesTools: false
  includesApprovals: false
  messages: number
  maxMessages: 40
  scanLimit: 200
  maxCharsPerMessage: 8192
  maxResponseBytes: 65536
  truncated: boolean
  contextStartSeq: number
}
export interface ConversationUpdate {
  expectedRevision: number
  title?: string
  closed?: boolean
  engineAccountId?: string | null
  engineSelection?: NonNullable<SpaceConversation['engineSelection']>
  expectedAccountRevision?: number
}
export interface ConversationHistory {
  context: SpaceContext
  entries: import('./models').Entry[]
  hasMore: boolean
}
export interface ConversationUpload {
  uploadId: string
  name: string
  offset: number
  /** Base64; até 384 KiB decodificados por chamada. */
  data: string
  done: boolean
  expectedRevision: number
}
export interface ContextAttachment {
  id: string
  name: string
  size: number
  sha256: string
  revision: number
  spaceId: string
  conversationId: string
}

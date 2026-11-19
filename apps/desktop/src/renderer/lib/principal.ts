import { resumeLabel } from '../../shared/subscription.ts'
import type { AppLanguage } from '../../shared/fork-idioma.ts'
import { UNAVAILABLE_TEXT } from '../../shared/models.ts'
import type { RosterItem } from '../store/app-model.ts'
import type { Tradutor } from './fork-i18n/index.ts'

// D121: projeções de apresentação. Identidade vem exclusivamente de teamSettings.
export interface PrincipalIdentity {
  id: string | null
  phase: 'unknown' | 'loading' | 'ready' | 'error'
}

export interface PrincipalStatus {
  text: string
  secondary?: string
  /** `unknown`: Estado indisponível (runtime-audiencia-01), sem cor de estado. */
  tone: 'normal' | 'attention' | 'danger' | 'unknown'
}

const KNOWN = new Set(['idle', 'working', 'needsInput', 'waiting', 'error'])

export interface PrincipalPreference {
  hostId: string
  primaryBotId: string | null
}

/** A preferência só restringe lista, navegação e ações; nunca confirma a identidade. */
export function protectedPrincipal(identity: PrincipalIdentity, saved: PrincipalPreference | null, hostId: string | null) {
  return identity.id ?? (identity.phase !== 'ready' && saved?.hostId === hostId ? saved.primaryBotId : null)
}

/** Não usa última mensagem, activity nem contagens de Atividades ainda inexistentes. */
export function principalStatus(t: Tradutor, idioma: AppLanguage, bot: { status: string; resumeAt?: number | null }, online: boolean, pendingInput = false, mismatch = false): PrincipalStatus {
  const attention = pendingInput || bot.status === 'needsInput'
  const problem = bot.status === 'waiting' ? t('navegacao.barra.principal.cota') : bot.status === 'error' ? t('navegacao.barra.principal.erro') : undefined
  const resume = bot.status === 'waiting' && bot.resumeAt != null ? resumeLabel(t, idioma, bot.resumeAt) : undefined
  // runtime-audiencia-01: sem runtime aqui não é Livre nem Ocupada. Um pedido pendente desta conversa ainda vale.
  const unknown = !KNOWN.has(bot.status)
  const current: PrincipalStatus = attention
    ? { text: t('navegacao.barra.principal.precisa'), secondary: [problem, resume].filter(Boolean).join(' · ') || undefined, tone: 'attention' }
    : problem ? { text: problem, ...(resume ? { secondary: resume } : {}), tone: bot.status === 'error' ? 'danger' : 'normal' }
      : unknown ? { text: UNAVAILABLE_TEXT, tone: 'unknown' }
        : { text: t(bot.status === 'working' ? 'navegacao.barra.principal.ocupado' : 'navegacao.barra.principal.livre'), tone: 'normal' }
  return online && !mismatch ? current : { text: t(mismatch ? 'navegacao.barra.principal.atualize' : 'navegacao.barra.principal.semHub'), secondary: t('navegacao.barra.principal.ultimo', { estado: [current.text, current.secondary].filter(Boolean).join(' · ') }), tone: 'normal' }
}

export function principalEmpty(t: Tradutor, identity: PrincipalIdentity, online: boolean, mismatch = false) {
  if (mismatch) return t('navegacao.barra.principal.atualize')
  if (!online) return t('navegacao.barra.principal.semHub')
  if (identity.phase === 'loading') return t('navegacao.barra.principal.carregando')
  return t('navegacao.barra.principal.falhou')
}

/** A assinatura exclui status/rev/posição: tokens e filtros nunca consultam a identidade. */
export function principalTrigger(bots: Iterable<{ id: string; manageTeam: boolean | null }>) {
  return JSON.stringify([...bots].map((bot) => [bot.id, bot.manageTeam]).sort((a, b) => String(a[0]).localeCompare(String(b[0]))))
}

/** Só a projeção da lista perde a principal; a paleta recebe inclusive hidden legado. */
export function projectPrincipal<T extends { bot: { id: string } }>(items: T[], id: string | null, principal?: T) {
  const palette = principal && !items.some((item) => item.bot.id === principal.bot.id) ? [...items, principal] : items
  return { palette, others: items.filter((item) => item.bot.id !== id) }
}

/**
 * D121: a paleta de cada computador da barra, completa pela principal do store (inclusive hidden legado), e a lista
 * sem ela. É o que a busca mostra; a abertura no Berga não usa esta projeção para decidir se há bot visível.
 */
export function paletteProjections<T extends { computer: RosterItem['computer']; store: RosterItem['store']; items: RosterItem[] }>(sections: T[]) {
  return sections.map((section) => {
    const id = section.store.principal.id
    const bot = id ? section.store.bots.get(id) : undefined
    const principal: RosterItem | undefined = bot ? { bot, store: section.store, computer: section.computer, ref: { computerId: section.computer.id, botId: bot.id } } : undefined
    return { section, ...projectPrincipal(section.items, section.store.principal.protectedId, principal) }
  })
}

/** D249 R5: offline, the list says nothing (the conversation side says it once, with Tentar de novo). */
export function principalListEmpty(t: Tradutor, filtering: boolean, hasStore = true, offline = false): string | null {
  if (offline && !filtering) return null
  if (!hasStore) return t('navegacao.barra.aguardandoComputador')
  return t(filtering ? 'navegacao.barra.nenhumOutro' : 'navegacao.barra.outrosAqui')
}

/**
 * D249 R5: the Hub is unreachable for what the bar shows: at least one computer shown and every shown one offline
 * (none online, none connecting). Hidden computers don't count either way; SSH profiles have their own recovery.
 */
export function shownHubsOffline(shown: { kind: string }[]): boolean {
  return shown.length > 0 && shown.every((c) => c.kind === 'offline' || c.kind === 'computerOffline')
}

/** Uma autorização só: o que o bot pede, lido do estado (status e cartões), nunca do texto da atividade (MX-147). */
export type PedidoAberto = 'aprovacao' | 'pergunta' | null
export function pedidoAberto(bot: { status: string }, cartaoPendente: boolean, soPerguntas: boolean): PedidoAberto {
  if (cartaoPendente) return 'aprovacao'
  if (bot.status !== 'needsInput') return null
  return soPerguntas ? 'pergunta' : 'aprovacao'
}

/** O card do principal pedindo você diz o pedido numa linha só, quando nada mais (cota, sem Hub) disputa a linha. */
export function linhaDoPedido(status: PrincipalStatus, online: boolean, mismatch: boolean, pedido: PedidoAberto): PedidoAberto {
  return online && !mismatch && status.tone === 'attention' && !status.secondary ? pedido : null
}

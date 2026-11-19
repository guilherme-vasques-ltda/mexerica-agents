// DON-907 C5: encerrar, continuar e "Arquivar sozinho". Puro,
// sem DOM nem textos: a view traduz os códigos com `useT` (`conversa.arquivar.*`). O Hub decide e grava tudo, inclusive
// a varredura que encerra por inatividade; o app não tem timer nem preferência própria.
import type { HubSession } from '../../sessoes/estado.ts'

/** Por que encerrar ou continuar não deu certo. Nenhum texto do Hub vai à tela. */
export type ControleFalha =
  | 'unsupported' // o Hub não anuncia `close`/`continue` (antigo) neste link
  | 'linkChanged' // reconectou ou trocou de Hub durante o pedido
  | 'offline'
  | 'pending' // `session_control_pending`: resultado incerto, o catálogo foi relido
  | 'alreadyClosed' // `session_closed`
  | 'stillOpen' // `session_open`: continuar só vale para encerrada
  | 'conflict' // `session_revision_conflict`
  | 'notAllowed' // `not_allowed: …`
  | 'chatUnsupported' // `session_chat_unsupported`
  | 'unknownSession' // `unknown_session`
  | 'busy'
  | 'principal'
  | 'failed'

export type ControleResultado = { ok: true; session: HubSession } | { ok: false; falha: ControleFalha }

/** O código no começo da mensagem de erro do Hub (encerrar.rs); o resto nunca é exibido. */
const codigo = (error: unknown) => {
  const message = (error as { message?: unknown } | null)?.message
  return typeof message === 'string' ? /^[a-z_]+/.exec(message)?.[0] ?? '' : ''
}

/** Como a conversa encerrada diz o motivo: `idle` = a varredura do Hub, `owner` = você; outro valor, genérico. */
export function motivoEncerrada(session: Pick<HubSession, 'closedReason'>): 'idle' | 'owner' | 'other' {
  return session.closedReason === 'idle' ? 'idle' : session.closedReason === 'owner' ? 'owner' : 'other'
}

/** `sessionAutoCloseConfig`/`Save`: `days` null = Nunca. Ausente no Hub = 3 dias (o próprio Hub responde assim). */
export interface ArquivarConfig { days: number | null; revision: number }

export const DIAS_PADRAO = 3
export const OPCOES_DIAS = [1, 3, 7, 14, 30, 90] as const
/** O "Nunca" no seletor (`days: null` no fio). */
export const NUNCA = 0

/** A resposta `{config: {days, revision, …}}`; qualquer outra forma é recusada. */
export function lerConfig(v: unknown): ArquivarConfig {
  const c = (v as { config?: unknown } | null)?.config as Partial<ArquivarConfig> | null | undefined
  const days = c?.days
  const okDays = days === null || (typeof days === 'number' && Number.isInteger(days) && days >= 1 && days <= 90)
  if (!c || typeof c !== 'object' || !okDays || typeof c.revision !== 'number') throw new Error('arquivar-forma-invalida')
  return { days: days as number | null, revision: c.revision }
}

/** As opções do seletor; um valor gravado fora da lista (outro cliente) aparece também, sem ser trocado. */
export function opcoesDias(atual: number | null): number[] {
  const lista: number[] = [...OPCOES_DIAS]
  if (atual !== null && !lista.includes(atual)) lista.push(atual)
  return [...lista.sort((a, b) => a - b), NUNCA]
}

/** O pedido de gravação, com a revisão lida (CAS no Hub). */
export const pedidoSalvar = (escolha: number, lido: ArquivarConfig) => ({ days: escolha === NUNCA ? null : escolha, expectedRevision: lido.revision })

const status = (error: unknown) => (error as { status?: unknown } | null)?.status
const semAutoridade = (error: unknown) => status(error) === 401 || status(error) === 403 || codigo(error) === 'not_allowed'

export function falhaDeControle(error: unknown): ControleFalha {
  if (status(error) === 0) return 'offline'
  if (semAutoridade(error)) return 'notAllowed'
  switch (codigo(error)) {
    case 'session_busy': return 'busy'
    case 'principal_not_deletable': return 'principal'
    case 'session_control_pending': return 'pending'
    case 'session_closed': return 'alreadyClosed'
    case 'session_open': return 'stillOpen'
    case 'session_revision_conflict': return 'conflict'
    case 'session_chat_unsupported': return 'chatUnsupported'
    case 'unknown_session': return 'unknownSession'
    default: return 'failed'
  }
}

/** Hub sem o RPC: 404 ou método desconhecido (`api::error_status`). */
export const hubAntigo = (error: unknown) => status(error) === 404 || /unknown method/i.test(String((error as { message?: unknown } | null)?.message ?? ''))

export type FalhaAjuste = 'conflict' | 'notAllowed' | 'failed'
export function falhaDeAjuste(error: unknown): FalhaAjuste {
  return codigo(error) === 'autoclose_revision_conflict' ? 'conflict' : semAutoridade(error) ? 'notAllowed' : 'failed'
}

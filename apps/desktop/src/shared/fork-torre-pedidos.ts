// Times R3a / MB-2: a seção "Pedidos entre bots" do Mission Board.
// Lê o campo `interBotRequests` do snapshot (`host/src/fork/retornos/pendentes.rs`): só quem pediu a quem, como
// terminou, o modo e quando. Nenhum texto do pedido nem resposta. Incerto continua incerto (D296).

export type InterBotState = 'finished' | 'cancelled' | 'interrupted'
export type InterBotMode = 'wait' | 'async'

export interface InterBotItem {
  key: string
  requesterBotId: string
  targetBotId: string
  state: InterBotState
  reason: string | null
  outcome: string | null
  mode: InterBotMode
  endedAt: number
}

export interface InterBotRequests { items: InterBotItem[]; omitted: number }

export type InterBotTone = 'uncertain' | 'error' | 'neutral'

// O mapa do plano (seção 3), conferido contra o CHECK da tabela do R1.
// DON-917: `rotulo` é o código do rótulo (o motivo ou o desfecho); a tela escolhe a frase em `torre.pedidos.rotulo.*`.
export type InterBotLabel = 'restart_uncertain' | 'restart_before_start' | 'host_stopped' | 'start_failed' | 'timeout' | 'recipient_stopped' | 'cancelled' | 'not_started' | 'failed' | 'stopped' | 'incomplete'
const DECIDED: Record<string, { state: InterBotState; rotulo: InterBotLabel; tone: InterBotTone }> = {
  restart_uncertain: { state: 'interrupted', rotulo: 'restart_uncertain', tone: 'uncertain' },
  restart_before_start: { state: 'interrupted', rotulo: 'restart_before_start', tone: 'neutral' },
  host_stopped: { state: 'interrupted', rotulo: 'host_stopped', tone: 'error' },
  start_failed: { state: 'interrupted', rotulo: 'start_failed', tone: 'error' },
  timeout: { state: 'cancelled', rotulo: 'timeout', tone: 'neutral' },
  recipient_stopped: { state: 'cancelled', rotulo: 'recipient_stopped', tone: 'error' },
  cancelled: { state: 'cancelled', rotulo: 'cancelled', tone: 'neutral' },
  not_started: { state: 'cancelled', rotulo: 'not_started', tone: 'neutral' },
}
const FINISHED: Record<string, { rotulo: InterBotLabel; tone: InterBotTone }> = {
  failed: { rotulo: 'failed', tone: 'error' },
  stopped: { rotulo: 'stopped', tone: 'neutral' },
  incomplete: { rotulo: 'incomplete', tone: 'neutral' },
}

/** O que aconteceu, em palavras; null para combinação que o app não conhece (a linha não aparece). */
export function interBotLabel(item: Pick<InterBotItem, 'state' | 'reason' | 'outcome'>): { rotulo: InterBotLabel; tone: InterBotTone } | null {
  if (item.state === 'finished') return item.reason === null && item.outcome !== null ? FINISHED[item.outcome] ?? null : null
  const decided = item.reason === null || item.outcome !== null ? undefined : DECIDED[item.reason]
  return decided && decided.state === item.state ? { rotulo: decided.rotulo, tone: decided.tone } : null
}


const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const str = (v: unknown) => (typeof v === 'string' && v ? v : null)

function item(v: unknown): InterBotItem | null {
  if (!isObj(v)) return null
  const key = str(v.key)
  const requesterBotId = str(v.requesterBotId)
  const targetBotId = str(v.targetBotId)
  const endedAt = typeof v.endedAt === 'number' && Number.isFinite(v.endedAt) ? v.endedAt : null
  if (!key || !requesterBotId || !targetBotId || endedAt === null) return null
  if (v.mode !== 'wait' && v.mode !== 'async') return null
  const parsed = {
    key, requesterBotId, targetBotId, endedAt, mode: v.mode as InterBotMode,
    state: v.state as InterBotState, reason: str(v.reason), outcome: str(v.outcome),
  }
  return interBotLabel(parsed) ? parsed : null
}

/** Lista inválida vira indisponível; item desconhecido é ignorado, nunca vira outro estado. */
export function parseInterBotRequests(v: unknown): InterBotRequests | null {
  if (!isObj(v) || !Array.isArray(v.items)) return null
  const omitted = v.omitted
  if (typeof omitted !== 'number' || !Number.isInteger(omitted) || omitted < 0) return null
  return { items: v.items.map(item).filter((i): i is InterBotItem => i !== null), omitted }
}

/** Há quanto tempo, contado da hora da fotografia (não do relógio da tela). Dado: a tela formata no idioma
 *  (`Intl.RelativeTimeFormat`; menos de 1 minuto é `torre.pedidos.agoraHaPouco`). */
export type Ago = { unit: 'now' } | { unit: 'minute' | 'hour' | 'day'; n: number }
export function ago(endedAt: number, collectedAt: number): Ago {
  const min = Math.max(0, Math.floor((collectedAt - endedAt) / 60_000))
  if (min < 1) return { unit: 'now' }
  if (min < 60) return { unit: 'minute', n: min }
  const h = Math.floor(min / 60)
  if (h < 24) return { unit: 'hour', n: h }
  return { unit: 'day', n: Math.floor(h / 24) }
}

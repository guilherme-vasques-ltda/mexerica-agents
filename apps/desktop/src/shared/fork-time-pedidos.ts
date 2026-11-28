// Aba "Time" do painel do bot: os pedidos que o bot fez a colegas (`donnaTeamRequests`, host/src/fork/pedidos_time).
// Abertos e terminados nas últimas 24 h, com quem pegou, o texto pedido e o estado. Resposta que não confere vira
// indisponível; item desconhecido é ignorado e nunca vira outro estado.
import { interBotLabel, type InterBotLabel, type InterBotMode, type InterBotState, type InterBotTone } from './fork-torre-pedidos.ts'

export type TeamRequestState = 'queued' | 'running' | InterBotState

export interface TeamRequest {
  key: string
  targetBotId: string
  /** null: o aviso na conversa sumiu ou não confere com o registro. */
  text: string | null
  state: TeamRequestState
  outcome: string | null
  reason: string | null
  mode: InterBotMode
  updatedAt: number
}

export interface TeamRequests { botId: string; items: TeamRequest[]; omitted: number }

export type TeamRequestLabel = 'queued' | 'running' | 'completed' | InterBotLabel
export type TeamRequestTone = 'active' | 'done' | InterBotTone

const ESTADOS: readonly string[] = ['queued', 'running', 'finished', 'cancelled', 'interrupted']
const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const str = (v: unknown) => (typeof v === 'string' && v ? v : null)
const ausente = (v: unknown) => v === null || v === undefined

/** Como está, em código de rótulo; null para combinação que o app não conhece (a linha não aparece). */
export function teamRequestLabel(item: Pick<TeamRequest, 'state' | 'reason' | 'outcome'>): { rotulo: TeamRequestLabel; tone: TeamRequestTone } | null {
  if (item.state === 'queued' || item.state === 'running') return item.reason === null && item.outcome === null ? { rotulo: item.state, tone: 'active' } : null
  if (item.state === 'finished' && item.reason === null && item.outcome === 'completed') return { rotulo: 'completed', tone: 'done' }
  return interBotLabel({ state: item.state, reason: item.reason, outcome: item.outcome })
}

function item(v: unknown): TeamRequest | null {
  if (!isObj(v)) return null
  const key = str(v.key)
  const targetBotId = str(v.targetBotId)
  const updatedAt = typeof v.updatedAt === 'number' && Number.isFinite(v.updatedAt) ? v.updatedAt : null
  if (!key || !targetBotId || updatedAt === null || typeof v.state !== 'string' || !ESTADOS.includes(v.state)) return null
  if (v.mode !== 'wait' && v.mode !== 'async') return null
  if (v.text !== null && typeof v.text !== 'string') return null
  const outcome = str(v.outcome)
  const reason = str(v.reason)
  if ((outcome === null && !ausente(v.outcome)) || (reason === null && !ausente(v.reason))) return null
  const pedido: TeamRequest = { key, targetBotId, text: str(v.text), state: v.state as TeamRequestState, outcome, reason, mode: v.mode, updatedAt }
  return teamRequestLabel(pedido) ? pedido : null
}

/** A resposta do Hub para este bot; outra coisa (outro bot, formato desconhecido) é null. */
export function parseTeamRequests(v: unknown, botId: string): TeamRequests | null {
  if (!isObj(v) || v.botId !== botId || !Array.isArray(v.items)) return null
  const omitted = v.omitted
  if (typeof omitted !== 'number' || !Number.isInteger(omitted) || omitted < 0) return null
  return { botId, items: v.items.map(item).filter((i): i is TeamRequest => i !== null), omitted }
}

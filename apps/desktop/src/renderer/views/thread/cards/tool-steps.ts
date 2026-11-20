// D165 fase F (components/ToolSteps): the steps a bot took in a turn, from the Hub's own `tool` entries
// (title, kind, status, time). Pure, so it is testable. Nothing is inferred: no entry, no step.
import type { Entry } from '@shared/models'
import type { Tradutor } from '../../../lib/fork-i18n/estado'

export type StepState = 'done' | 'running' | 'failed' | 'waiting'

export interface Step {
  id: string
  /** O título do Hub; vazio quando ele não manda (a view mostra o nome do tipo, `toolLabel`). */
  title: string
  kind: string
  state: StepState
  at: number
}

export interface TurnSteps {
  steps: Step[]
  /** Distinct tool kinds, in first-use order (grupos de `ToolGroup`; o texto vem do catálogo). */
  tools: ToolGroup[]
  /** From the first step to the reply (or to the last step while it works), in ms. */
  duration: number
}

export type ToolGroup = 'comando' | 'edicao' | 'leitura' | 'busca' | 'web' | 'raciocinio' | 'ferramenta'

const KIND_GROUPS: Record<string, ToolGroup> = {
  execute: 'comando',
  edit: 'edicao',
  delete: 'edicao',
  move: 'edicao',
  read: 'leitura',
  search: 'busca',
  fetch: 'web',
  think: 'raciocinio',
  other: 'ferramenta',
}

export const toolGroup = (kind: string | undefined): ToolGroup => KIND_GROUPS[kind ?? ''] ?? 'ferramenta'
/** O nome curto do tipo ("Comando", "Edição"…) no idioma. */
export const toolLabel = (t: Tradutor, kind: string | undefined) => t(`conversa.cartoes.passos.tipo.${toolGroup(kind)}`)

export function stepState(status: string | undefined): StepState {
  switch (status) {
    case 'completed':
      return 'done'
    case 'failed':
      return 'failed'
    case 'in_progress':
      return 'running'
    default:
      return 'waiting'
  }
}

const key = (turn: number, author: string) => `${turn}|${author}`
const authorOf = (e: Entry) => e.data.author ?? e.botId

const GENERIC: ToolGroup = 'ferramenta'

function summarize(steps: Step[], end: number): TurnSteps {
  const labels: ToolGroup[] = []
  for (const s of steps) {
    const label = toolGroup(s.kind)
    if (!labels.includes(label)) labels.push(label)
  }
  // D169: "Ferramenta" says nothing next to named kinds; it stays only when it's all there is.
  const named = labels.filter((label) => label !== GENERIC)
  const tools = named.length ? named : labels
  const first = steps[0]?.at ?? end
  return { steps, tools, duration: Math.max(0, end - first) }
}

/**
 * Steps per reply: a turn's `tool` entries go with that author's last final reply in the same turn.
 * Only the current turn's unreplied steps are `pending`; earlier turns need not have a final reply.
 */
export function stepsByReply(entries: readonly Entry[]): { byReply: Map<string, TurnSteps>; pending: Map<string, TurnSteps> } {
  const steps = new Map<string, Step[]>()
  const lastReply = new Map<string, Entry>()
  // allEntries is in transcript order, including a new user entry before its first tool arrives.
  // A local optimistic user entry has turn 0, which also keeps earlier work out of its live list.
  const currentTurn = entries[entries.length - 1]?.turn
  for (const e of entries) {
    const k = key(e.turn, authorOf(e))
    if (e.kind === 'tool') {
      const list = steps.get(k) ?? []
      list.push({ id: e.id, title: e.data.title ?? '', kind: e.data.toolKind ?? 'other', state: stepState(e.data.status), at: e.createdAt })
      steps.set(k, list)
    } else if (e.kind === 'agent' && (e.data.final === true || e.data.interrupted === true)) {
      lastReply.set(k, e)
    }
  }
  const byReply = new Map<string, TurnSteps>()
  const pending = new Map<string, TurnSteps>()
  for (const [k, list] of steps) {
    const reply = lastReply.get(k)
    // A reply that came before the turn's last step is not the end of the work: the steps stay live.
    if (reply && reply.createdAt >= list[list.length - 1]!.at) byReply.set(reply.id, summarize(list, reply.createdAt))
    else if (k.startsWith(`${currentTurn}|`)) pending.set(k.slice(k.indexOf('|') + 1), summarize(list, list[list.length - 1]!.at))
  }
  return { byReply, pending }
}

/** "41 s", "2 min 5 s", "1 h 3 min" (as unidades no idioma). */
export function durationLabel(t: Tradutor, ms: number) {
  const s = Math.round(ms / 1000)
  if (s < 60) return t('conversa.cartoes.passos.duracao.s', { s })
  const m = Math.floor(s / 60)
  if (m < 60) return s % 60 ? t('conversa.cartoes.passos.duracao.ms', { m, s: s % 60 }) : t('conversa.cartoes.passos.duracao.m', { m })
  return t('conversa.cartoes.passos.duracao.hm', { h: Math.floor(m / 60), m: m % 60 })
}

/** "4 passos · Comando, Edição · 41 s" */
export function stepsSummary(t: Tradutor, steps: TurnSteps) {
  const count = t.plural('conversa.cartoes.passos.contagem', steps.steps.length)
  const tools = steps.tools.map((g) => t(`conversa.cartoes.passos.tipo.${g}`)).join(t('conversa.cartoes.passos.separadorLista'))
  return [count, tools, steps.duration > 0 ? durationLabel(t, steps.duration) : null].filter(Boolean).join(' · ')
}

/** D169 (ToolSteps): while the bot works, "Trabalhando · 2 passos". */
export function liveSummary(t: Tradutor, steps: TurnSteps) {
  return t.plural('conversa.cartoes.passos.trabalhando', steps.steps.length)
}

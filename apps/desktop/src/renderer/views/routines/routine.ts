// Routines (kit Routine.swift, RoutineScheduleDraft.swift). The host hydrates, compiles,
// validates and describes schedules; the app only keeps form values.
import type { RunOutcome } from './run-outcome'
import { erroDaDonna } from '../../lib/fork-donna-erros.ts'
import { ltr } from '../../../shared/bidi.ts'

export interface RoutineTrigger {
  type: string
  seconds?: number | null
  at?: number | null
  expression?: string | null
  timeZone?: string | null
  source?: string | null
  event?: string | null
  filters?: Record<string, string> | null
}

export interface Routine {
  id: string
  botId: string
  name: string
  instruction: string
  triggers: RoutineTrigger[]
  enabled: boolean
  createdAt: number
  updatedAt: number
  triggerDescriptions: string[]
  nextRunAt?: number | null
  timeoutSeconds?: number | null
  lastError?: string | null
  externalReleasePending?: boolean
}

export interface RoutineRun {
  id: string
  routineId: string
  botId: string
  status: string
  createdAt: number
  finishedAt?: number | null
  detail?: string | null
  /** I-023: o desfecho gravado pelo Hub; ausente em execuções antigas ou Hub antigo. */
  outcome?: RunOutcome | null
}

export const isActiveRun = (run: RoutineRun) => ['pending', 'starting', 'running', 'recovering'].includes(run.status)

export interface RoutineListing {
  routines: Routine[]
  runs: RoutineRun[]
}

/** `url` is public through the Codync cloud (null while the cloud is off), `localUrl` works on the host itself. */
export interface RoutineWebhook {
  url?: string | null
  localUrl: string
  key: string
  connected: boolean
}

/** Form values only. */
export interface RoutineScheduleDraft {
  kind: string
  amount: string
  unit: number
  calendarStyle: string
  weekday: number
  selectedDays: number[]
  monthDay: number
  minuteStep: number
  hour: number
  minute: number
  at: number
  expression: string
  zone: string
  original: RoutineTrigger[]
}

export const emptyDraft = (): RoutineScheduleDraft => ({
  kind: 'cron',
  amount: '1',
  unit: 3600,
  calendarStyle: 'daily',
  weekday: 1,
  selectedDays: [],
  monthDay: 1,
  minuteStep: 15,
  hour: 9,
  minute: 0,
  at: 0,
  expression: '',
  zone: '',
  original: [],
})

export interface RoutineSchedulePreview {
  draft: RoutineScheduleDraft
  triggers: RoutineTrigger[]
  summary: string
  /** Donnabot: a agenda em pt-BR (Hub, calendar.rs); fora do pt-BR ou num Hub antigo, `summary` (agendaDoHub). */
  resumo?: string
  nextRunAt?: number | null
  warning?: string | null
}

/**
 * The Repeat choices: the host's calendar styles the editor offers. A schedule in any other
 * style (every few minutes, monthly) opens as Custom, its cron as written. "Some days" (`some`) is the host's
 * `days` (one or more days; also what `weekly` was) and, like Every day and Weekdays, has the time pills.
 */
export const repeatChoices = [
  // Donnabot (DON-917): os rótulos vêm do catálogo (`rotinas.editor.repetir.<id>`), no idioma do app.
  { id: 'hourly' },
  { id: 'daily' },
  { id: 'weekdays' },
  { id: 'some' },
  { id: 'custom' },
] as const

export type RepeatChoice = (typeof repeatChoices)[number]['id']

export const repeatChoice = (style: string): RepeatChoice => (style === 'weekly' || style === 'days' ? 'some' : (repeatChoices.find((c) => c.id === style)?.id ?? 'custom'))

/** The cron the host compiled for a checked schedule, to start Custom from the same times. */
export const compiledCron = (preview: RoutineSchedulePreview | null) => {
  const [trigger] = preview?.triggers ?? []
  return trigger?.type === 'cron' ? (trigger.expression ?? null) : null
}

/** A timeout in seconds as people say it ("1 hour", "30 minutes", "90 seconds"); other text as typed. */
export function durationText(text: string) {
  const value = text.trim()
  if (!/^\d+$/.test(value) || Number(value) === 0) return `${value}s`
  const seconds = Number(value)
  const count = (amount: number, unit: string) => `${amount} ${unit}${amount === 1 ? '' : 's'}`
  if (seconds % 3600 === 0) return count(seconds / 3600, 'hour')
  if (seconds % 60 === 0) return count(seconds / 60, 'minute')
  return count(seconds, 'second')
}

/** A mensagem crua de um erro, para guardar no estado; a tela a traduz ao desenhar com `errorText` (DON-917). */
export const rawError = (e: unknown) => (e instanceof Error ? e.message : String(e))

export const errorText = (e: unknown) => {
  const message = e instanceof Error ? e.message : String(e)
  return erroDaDonna(message) ?? message
}

/** Rótulo de minuto do menu de "toda hora" (":05"). Isolado em LTR: em RTL (ar-AE) o ":" iria para o outro lado ("05:"). */
export const minuteLabel = (minute: number): string => ltr(`:${String(minute).padStart(2, '0')}`)

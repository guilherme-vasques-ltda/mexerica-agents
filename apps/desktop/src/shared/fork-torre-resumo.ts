// MB-3: a coluna "O que aconteceu" do Mission Board.
// Lê o campo `whatHappened` do snapshot (`host/src/fork/resumo_diario/torre.rs`): o último dia fechado, por Time,
// com uma linha do Time e uma linha curta por bot. Fica fora dos contadores. Só rótulos; nada é calculado aqui.

export interface SummaryLine { text: string; joined: boolean }
export interface SummaryBot { botId: string; line: SummaryLine }
export interface SummaryTeam { teamId: string; name: string; line: SummaryLine | null; bots: SummaryBot[] }
export type OutReason = 'codex' | 'api' | 'other_engine' | 'no_account' | 'no_model'
export interface SummaryOut { botId: string; reason: OutReason }

export interface WhatHappened {
  enabled: boolean
  time: string
  timeZone: string
  day: string
  closesAt: number
  nextAt: number
  teams: SummaryTeam[]
  loose: SummaryBot[]
  out: SummaryOut[]
}

/** Os motivos conhecidos; o rótulo é da tela (`torre.oqa.motivo.*`, DON-917). */
const OUT_REASONS: readonly OutReason[] = ['codex', 'api', 'other_engine', 'no_account', 'no_model']

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const str = (v: unknown) => (typeof v === 'string' && v ? v : null)
const num = (v: unknown) => (typeof v === 'number' && Number.isFinite(v) ? v : null)

function line(v: unknown): SummaryLine | null {
  if (!isObj(v)) return null
  const text = str(v.text)
  return text && typeof v.joined === 'boolean' ? { text, joined: v.joined } : null
}

function bot(v: unknown): SummaryBot | null {
  if (!isObj(v)) return null
  const botId = str(v.botId)
  const l = line(v.line)
  return botId && l ? { botId, line: l } : null
}

const bots = (v: unknown) => (Array.isArray(v) ? v.map(bot).filter((b): b is SummaryBot => b !== null) : [])

/** Campo inválido vira indisponível; item desconhecido é ignorado. */
export function parseWhatHappened(v: unknown): WhatHappened | null {
  if (!isObj(v) || !Array.isArray(v.teams) || !Array.isArray(v.loose) || !Array.isArray(v.out)) return null
  const time = str(v.time)
  const timeZone = str(v.timeZone)
  const day = str(v.day)
  const closesAt = num(v.closesAt)
  const nextAt = num(v.nextAt)
  if (typeof v.enabled !== 'boolean' || !time || !timeZone || !day || closesAt === null || nextAt === null) return null
  const teams = v.teams.flatMap((t): SummaryTeam[] => {
    if (!isObj(t)) return []
    const teamId = str(t.teamId)
    const name = str(t.name)
    return teamId && name ? [{ teamId, name, line: t.line == null ? null : line(t.line), bots: bots(t.bots) }] : []
  })
  const out = v.out.flatMap((o): SummaryOut[] => {
    if (!isObj(o)) return []
    const botId = str(o.botId)
    return botId && typeof o.reason === 'string' && (OUT_REASONS as readonly string[]).includes(o.reason) ? [{ botId, reason: o.reason as OutReason }] : []
  })
  return { enabled: v.enabled, time, timeZone, day, closesAt, nextAt, teams, loose: bots(v.loose), out }
}

const ymd = (ms: number, timeZone: string) => {
  try {
    return new Intl.DateTimeFormat('en-CA', { timeZone, year: 'numeric', month: '2-digit', day: '2-digit' }).format(ms)
  } catch {
    return new Date(ms).toISOString().slice(0, 10)
  }
}

/** Dia relativo: `offset` −1/0/+1 (ontem, hoje, amanhã; a tela diz no idioma) ou só o dia AAAA-MM-DD. */
export type RelativeDay = { offset: -1 | 0 | 1 } | { day: string }

/** Hoje, ontem ou a data, contado da hora da fotografia no fuso salvo. */
export function relativeDay(day: string, collectedAt: number, timeZone: string): RelativeDay {
  if (ymd(collectedAt, timeZone) === day) return { offset: 0 }
  if (ymd(collectedAt - 86_400_000, timeZone) === day) return { offset: -1 }
  return { day }
}

/** Hoje, amanhã ou a data do próximo fechamento. */
export function nextDay(nextAt: number, collectedAt: number, timeZone: string): RelativeDay {
  const day = ymd(nextAt, timeZone)
  if (ymd(collectedAt, timeZone) === day) return { offset: 0 }
  if (ymd(collectedAt + 86_400_000, timeZone) === day) return { offset: 1 }
  return { day }
}

/** "Quarta-feira, 4 de nov" (no locale dado) a partir de AAAA-MM-DD, sem depender do fuso da tela. */
export function longDay(day: string, locale = 'pt-BR'): string {
  const d = new Date(`${day}T12:00:00Z`)
  if (Number.isNaN(d.getTime())) return day
  const text = d.toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'short', timeZone: 'UTC' }).replace('.', '')
  return text.charAt(0).toUpperCase() + text.slice(1)
}

/** Horários de 15 em 15 minutos, de 02:00 a 23:45: o nível mais fundo (T − 2 h) não cruza a meia-noite (o Hub confere). */
export const TIMES: string[] = Array.from({ length: 88 }, (_, i) => `${String(Math.floor(i / 4) + 2).padStart(2, '0')}:${String((i % 4) * 15).padStart(2, '0')}`)

/** Fuso IANA do aparelho; o Hub confere. */
export function deviceTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'
  } catch {
    return 'UTC'
  }
}

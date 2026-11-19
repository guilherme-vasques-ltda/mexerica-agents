// Adaptado de OpenMausBot (Apache-2.0, commit 615fe65e17f88d99f12d34682750d9ae2fa402bc):
// src/lib/routine-calendar.ts (datas, encaixe e colunas de sobreposição) e
// src/components/routines/MiniMonth.tsx (grade de 42 dias). Alterado para o Donnabot (D85):
// semana a partir do domingo, visão Mês, rótulos com Intl no idioma do app (DON-917; `idioma` explícito, sem
// ler a preferência aqui), sem chamadas, limites nem projeção no cliente (as ocorrências vêm do Hub).
import type { AppLanguage } from '../../shared/fork-idioma.ts'
import { localeDe } from './fork-i18n/index.ts'

export type CalendarView = 'day' | 'week' | 'month'

export const DAY_MS = 86_400_000
/** Uma rotina ocupa um marcador de 30 min na grade de horas. */
export const MARKER_MINUTES = 30
/** O cursor do teclado e o arrastar andam de meia em meia hora. */
export const SLOT_MINUTES = 30

export function startOfDay(at: number): number {
  const date = new Date(at)
  date.setHours(0, 0, 0, 0)
  return date.getTime()
}

/**
 * O começo do dia `days` dias de calendário depois do dia de `at` (não 24 h). Sempre passa pelo
 * `startOfDay`: num dia em que o horário de verão começa à meia-noite, 00:00 não existe e o dia
 * começa à 01:00; sem isso, os dias seguintes ficariam à 01:00 e não bateriam com `startOfDay`.
 */
export function addDays(at: number, days: number): number {
  const date = new Date(at)
  date.setDate(date.getDate() + days)
  return startOfDay(date.getTime())
}

/** O domingo da semana de `at`. */
export function startOfWeek(at: number): number {
  const date = new Date(startOfDay(at))
  return addDays(date.getTime(), -date.getDay())
}

export function startOfMonth(at: number): number {
  const date = new Date(at)
  return startOfDay(new Date(date.getFullYear(), date.getMonth(), 1).getTime())
}

export function addMonths(at: number, months: number): number {
  const date = new Date(startOfMonth(at))
  return startOfMonth(new Date(date.getFullYear(), date.getMonth() + months, 1).getTime())
}

/** A grade de um mês: 6 semanas completas a partir do domingo anterior ao dia 1. */
export function monthGrid(month: number): number[] {
  const first = startOfWeek(startOfMonth(month))
  return Array.from({ length: 42 }, (_, index) => addDays(first, index))
}

/** O período que uma visão mostra a partir do dia âncora: `[start, end)` e seus dias. */
export function viewRange(view: CalendarView, anchor: number): { start: number; end: number; days: number[] } {
  if (view === 'month') {
    const days = monthGrid(anchor)
    return { start: days[0]!, end: addDays(days[41]!, 1), days }
  }
  const start = view === 'week' ? startOfWeek(anchor) : startOfDay(anchor)
  const count = view === 'week' ? 7 : 1
  const days = Array.from({ length: count }, (_, index) => addDays(start, index))
  return { start, end: addDays(start, count), days }
}

/** Anterior/próximo: um dia, uma semana ou um mês. */
export function stepAnchor(view: CalendarView, anchor: number, direction: -1 | 1): number {
  if (view === 'month') return addMonths(anchor, direction)
  return addDays(anchor, view === 'week' ? 7 * direction : direction)
}

export function minutesOfDay(at: number): number {
  const date = new Date(at)
  return date.getHours() * 60 + date.getMinutes()
}

/** O instante `minutes` depois da meia-noite local de `day`. */
export function atMinutes(day: number, minutes: number): number {
  const date = new Date(day)
  date.setHours(Math.floor(minutes / 60), minutes % 60, 0, 0)
  return date.getTime()
}

export function snapMinutes(minutes: number, increment = SLOT_MINUTES): number {
  return Math.max(0, Math.min(24 * 60 - increment, Math.floor(minutes / increment) * increment))
}

/** "GMT-3", "GMT+5:30", "GMT+0": o deslocamento em minutos a leste de UTC (`-getTimezoneOffset()`). */
export function formatGmtOffset(offsetMinutes: number): string {
  if (offsetMinutes === 0) return 'GMT+0'
  const sign = offsetMinutes > 0 ? '+' : '-'
  const absolute = Math.abs(offsetMinutes)
  const hours = Math.floor(absolute / 60)
  const minutes = absolute % 60
  return `GMT${sign}${hours}${minutes ? `:${String(minutes).padStart(2, '0')}` : ''}`
}

export interface CollisionItem {
  id: string
  at: number
  durationMinutes: number
}

/** Cartões que se cruzam dividem a largura do dia: cada um vai para a primeira coluna livre. */
export function packCollisions(items: readonly CollisionItem[]): Map<string, { column: number; columns: number }> {
  // O cartão tem altura mínima visual de 15 min: conta essa altura para não sobrepor.
  const visualEnd = (item: CollisionItem) => item.at + Math.max(15, item.durationMinutes) * 60_000
  const sorted = [...items].sort((a, b) => a.at - b.at || visualEnd(a) - visualEnd(b) || a.id.localeCompare(b.id))
  const result = new Map<string, { column: number; columns: number }>()
  for (let cursor = 0; cursor < sorted.length; ) {
    const cluster: CollisionItem[] = []
    let clusterEnd = sorted[cursor]!.at
    while (cursor < sorted.length) {
      const item = sorted[cursor]!
      if (cluster.length > 0 && item.at >= clusterEnd) break
      cluster.push(item)
      clusterEnd = Math.max(clusterEnd, visualEnd(item))
      cursor += 1
    }
    const columnEnds: number[] = []
    const assigned = cluster.map((item) => {
      const free = columnEnds.findIndex((end) => end <= item.at)
      const column = free === -1 ? columnEnds.length : free
      columnEnds[column] = visualEnd(item)
      return { id: item.id, column }
    })
    const columns = Math.max(1, columnEnds.length)
    for (const { id, column } of assigned) result.set(id, { column, columns })
  }
  return result
}

const capitalize = (text: string, locale: string) => text.charAt(0).toLocaleUpperCase(locale) + text.slice(1)

// Os formatadores são criados uma vez por idioma: a grade formata hora e dia de cada cartão a cada desenho.
const formatadores = new Map<string, Intl.DateTimeFormat>()
function formato(idioma: AppLanguage, opcoes: Intl.DateTimeFormatOptions): Intl.DateTimeFormat {
  const chave = `${idioma}|${JSON.stringify(opcoes)}`
  let f = formatadores.get(chave)
  if (!f) formatadores.set(chave, (f = new Intl.DateTimeFormat(localeDe(idioma), opcoes)))
  return f
}

/** O título do período: "terça-feira, 20 de outubro de 2026", "18 – 24 de outubro de 2026", "Outubro de 2026". */
export function rangeLabel(view: CalendarView, anchor: number, idioma: AppLanguage): string {
  const locale = localeDe(idioma)
  if (view === 'month') return capitalize(formato(idioma, { month: 'long', year: 'numeric' }).format(anchor), locale)
  if (view === 'day') return capitalize(formato(idioma, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' }).format(anchor), locale)
  const start = startOfWeek(anchor)
  const end = addDays(start, 6)
  return formato(idioma, { day: 'numeric', month: 'long', year: 'numeric' }).formatRange(start, end)
}

/** "09:30" (ou "9:30 AM") no fuso deste computador. */
export function timeLabel(at: number, idioma: AppLanguage): string {
  return formato(idioma, { hour: '2-digit', minute: '2-digit' }).format(at)
}

/** "quarta-feira, 21 de outubro": para leitores de tela e títulos. */
export function dayLabel(at: number, idioma: AppLanguage): string {
  return formato(idioma, { weekday: 'long', day: 'numeric', month: 'long' }).format(at)
}

/** "Qua": o cabeçalho curto da coluna do dia, em caixa de frase e sem ponto (auditoria D249, F2). */
export function weekdayShort(at: number, idioma: AppLanguage): string {
  return capitalize(formato(idioma, { weekday: 'short' }).format(at).replace('.', ''), localeDe(idioma))
}

/** Um domingo qualquer: os dias da semana saem do Intl a partir dele. */
const DOMINGO = new Date(2026, 10, 1).getTime()
const semana = (idioma: AppLanguage, f: (at: number, idioma: AppLanguage) => string) => Array.from({ length: 7 }, (_, i) => f(addDays(DOMINGO, i), idioma))

/** Iniciais da semana a partir do domingo, para o minicalendário. */
export const weekdayInitials = (idioma: AppLanguage) => semana(idioma, (at, i) => formato(i, { weekday: 'narrow' }).format(at))
export const weekdayNames = (idioma: AppLanguage) => semana(idioma, (at, i) => formato(i, { weekday: 'long' }).format(at))
/** Cabeçalho do mês, em caixa de frase (auditoria D249, F2). */
export const weekdayShortNames = (idioma: AppLanguage) => semana(idioma, weekdayShort)

// Rotinas com vários horários no mesmo dia (docs/propostas/rotinas-varios-horarios-20261116).
// Regras puras do editor: ler os gatilhos de relógio como horários, montar de volta UM gatilho `cron` por horário
// e dizer o que impede de salvar. O Hub continua dono da validação (a tela só confere o que dá para dizer ao leigo
// antes de enviar) e aceita de 1 a 20 gatilhos (`check.rs`); a tela nunca passa disso nem corta o que já existe.
import type { RoutineTrigger } from './routine'

/** O teto de gatilhos por rotina do Hub (`check.rs`): a tela mostra todas as pílulas até aqui e não deixa passar. */
export const MAX_TIMES = 20

export const ALL_DAYS: readonly number[] = [0, 1, 2, 3, 4, 5, 6]
export const WEEKDAYS: readonly number[] = [1, 2, 3, 4, 5]

/** Os estilos de "Repetir" que têm pílulas de horário (a linha "Dias" vale para todos os horários). */
export const CLOCK_STYLES = ['daily', 'weekdays', 'some'] as const
export type ClockStyle = (typeof CLOCK_STYLES)[number]
export const isClockStyle = (style: string): style is ClockStyle => (CLOCK_STYLES as readonly string[]).includes(style)

const pad = (n: number) => String(n).padStart(2, '0')
const TIME = /^([01]\d|2[0-3]):([0-5]\d)$/

/** "HH:MM", a forma do campo de hora do app. */
export const validTime = (time: string) => TIME.test(time)
export const timeOf = (hour: number, minute: number) => `${pad(hour)}:${pad(minute)}`
export const partsOf = (time: string): { hour: number; minute: number } => {
  const [hour = 0, minute = 0] = time.split(':').map(Number)
  return { hour, minute }
}

const ints = (field: string, max: number): number[] | null => {
  const parts = field.split(',')
  if (!parts.every((p) => /^\d{1,2}$/.test(p) && Number(p) <= max)) return null
  return parts.map(Number)
}

const DAY_NAMES: Record<string, number> = { SUN: 0, MON: 1, TUE: 2, WED: 3, THU: 4, FRI: 5, SAT: 6 }

/** Um dia do cron: número de 0 a 7 (0 e 7 são domingo) ou nome (SUN a SAT, sem diferenciar maiúsculas). */
const dayValue = (token: string): number | null => (/^[0-7]$/.test(token) ? Number(token) : (DAY_NAMES[token.toUpperCase()] ?? null))

/**
 * O campo de dia da semana do cron como conjunto (0 = domingo): `*`, `1-5`, `3`, `1,3,6`, `1-3,6`, `7`, `MON-FRI`, `sun,sat`.
 * Faixa que passa do domingo ("FRI-SUN", "6-0") é ambígua: devolve null e a rotina fica como está.
 */
const weekdaySet = (field: string): number[] | null => {
  if (field === '*') return [...ALL_DAYS]
  const days = new Set<number>()
  for (const part of field.split(',')) {
    const [first, last, ...rest] = part.split('-')
    const from = dayValue(first!)
    const to = last === undefined ? from : dayValue(last)
    if (rest.length > 0 || from === null || to === null || from > to) return null
    for (let day = from; day <= to; day++) days.add(day % 7)
  }
  return [...days].sort((a, b) => a - b)
}

export interface ClockSchedule {
  /** "HH:MM", em ordem; um horário repetido continua repetido (a tela o mostra como erro, não o some). */
  times: string[]
  /** Os dias da semana (0 = domingo) que valem para todos os horários. */
  days: number[]
  zone: string
}

/**
 * Os gatilhos que a tela mostra como pílulas: todos de relógio (`cron`), no mesmo fuso, com os mesmos dias e sem dia do
 * mês nem mês; hora e minuto podem ser números ou listas ("0 9,15 * * *", "0,30 9 * * 1-5"). Qualquer outra mistura (intervalo,
 * data única, evento, webhook, passo de minutos, faixa de horas, fusos ou dias diferentes, mais de 20 horários) devolve null:
 * a rotina segue como "Definida pelo bot", intacta.
 */
export function readClockTriggers(triggers: readonly RoutineTrigger[]): ClockSchedule | null {
  if (triggers.length === 0) return null
  const times: string[] = []
  let days: number[] | null = null
  let zone: string | null = null
  for (const trigger of triggers) {
    if (trigger.type !== 'cron' || !trigger.expression || !trigger.timeZone) return null
    if (zone !== null && trigger.timeZone !== zone) return null
    zone = trigger.timeZone
    const fields = trigger.expression.trim().split(/\s+/)
    if (fields.length !== 5 || fields[2] !== '*' || fields[3] !== '*') return null
    const minutes = ints(fields[0]!, 59)
    const hours = ints(fields[1]!, 23)
    const set = weekdaySet(fields[4]!)
    if (!minutes || !hours || !set) return null
    if (days !== null && days.join() !== set.join()) return null
    days = set
    for (const hour of hours) for (const minute of minutes) times.push(timeOf(hour, minute))
    if (times.length > MAX_TIMES) return null
  }
  return { times: times.sort(), days: days!, zone: zone! }
}

/** O estilo de "Repetir" que descreve esses dias. */
export const styleOfDays = (days: readonly number[]): ClockStyle =>
  days.length === 7 ? 'daily' : days.join() === WEEKDAYS.join() ? 'weekdays' : 'some'

export const daysOfStyle = (style: ClockStyle, selected: readonly number[]): number[] =>
  style === 'daily' ? [...ALL_DAYS] : style === 'weekdays' ? [...WEEKDAYS] : [...selected].sort((a, b) => a - b)

const weekdayField = (days: readonly number[]) => (days.length === 7 ? '*' : days.join() === WEEKDAYS.join() ? '1-5' : days.join(','))

/** Um gatilho `cron` por horário, mesmos dias e mesmo fuso (o formato do `editor.rs` do Hub: "0 9 * * 1-5"). */
export function buildClockTriggers({ times, days, zone }: ClockSchedule): RoutineTrigger[] {
  const weekday = weekdayField(days)
  return times.map((time) => {
    const { hour, minute } = partsOf(time)
    return { type: 'cron', expression: `${minute} ${hour} * * ${weekday}`, timeZone: zone }
  })
}

/**
 * O que salvar: os gatilhos que a rotina já tem, exatamente como estão, quando a agenda (horários, dias e fuso) não
 * mudou; só quando algo da agenda mudou, um gatilho por horário. Assim trocar só o nome ou a instrução de "0 9,15 * * *"
 * não reescreve os gatilhos (o Hub não recalcula as próximas execuções nem descarta uma ocorrência perdida pendente).
 */
export function triggersToSave(original: readonly RoutineTrigger[] | null | undefined, current: ClockSchedule): RoutineTrigger[] {
  const was = original ? readClockTriggers(original) : null
  const same = !!was && was.zone === current.zone && was.days.join() === [...current.days].sort((a, b) => a - b).join() && was.times.join() === [...current.times].sort().join()
  return same ? [...original!] : buildClockTriggers(current)
}

export type TimesProblem = { kind: 'empty' } | { kind: 'repeat'; times: string[] } | { kind: 'noDay' }

/** O que impede de salvar, na ordem em que a tela diz: horário vazio, horário repetido, nenhum dia. */
export function timesProblem(times: readonly string[], days: readonly number[]): TimesProblem | null {
  if (times.length === 0 || times.some((time) => !validTime(time))) return { kind: 'empty' }
  const repeated = [...new Set(times.filter((time, i) => times.indexOf(time) !== i))]
  if (repeated.length > 0) return { kind: 'repeat', times: repeated.sort() }
  if (days.length === 0) return { kind: 'noDay' }
  return null
}

/** As pílulas a pintar de erro: vazias ou iguais a outra. */
export const badTimes = (times: readonly string[]): boolean[] =>
  times.map((time, i) => !validTime(time) || times.indexOf(time) !== i || times.lastIndexOf(time) !== i)

/** As pílulas se ordenam ao sair do campo, e só quando todas estão preenchidas (não enquanto digita). */
export const sortedTimes = (times: readonly string[]): string[] => (times.every(validTime) ? [...times].sort() : [...times])

export const setTimeAt = (times: readonly string[], index: number, value: string): string[] => times.map((time, i) => (i === index ? value : time))

/** "+ horário": a hora seguinte à última, no mesmo minuto, sem repetir. Nunca passa do teto. */
export function addTime(times: readonly string[]): string[] {
  if (times.length >= MAX_TIMES) return [...times]
  const last = [...times].filter(validTime).sort().pop() ?? '08:00'
  const { hour, minute } = partsOf(last)
  for (let step = 1; step < 24; step++) {
    const next = timeOf((hour + step) % 24, minute)
    if (!times.includes(next)) return sortedTimes([...times, next])
  }
  return sortedTimes([...times, last])
}

/** "×": tira a pílula; sempre fica pelo menos uma. */
export const removeTimeAt = (times: readonly string[], index: number): string[] => (times.length <= 1 ? [...times] : times.filter((_, i) => i !== index))

/**
 * O cron único que dispara exatamente nos mesmos horários e dias, para "Personalizado" começar do que a tela mostra
 * ("0,30 9,15 * * 1-5"). Só existe quando os horários são o produto de uma lista de minutos por uma de horas (9h00, 9h30, 15h00
 * e 15h30 são; 9h00 e 15h30 sozinhos não): senão devolve null e "Personalizado" não é oferecido, para salvar nunca mudar
 * o conjunto de disparos.
 */
export function singleCron(times: readonly string[], days: readonly number[]): string | null {
  if (timesProblem(times, days) !== null) return null
  const parts = times.map(partsOf)
  const hours = [...new Set(parts.map((p) => p.hour))].sort((a, b) => a - b)
  const minutes = [...new Set(parts.map((p) => p.minute))].sort((a, b) => a - b)
  // Cada horário está no produto; com a mesma quantidade, o produto não tem horário a mais.
  if (hours.length * minutes.length !== times.length) return null
  return `${minutes.join(',')} ${hours.join(',')} * * ${weekdayField(days)}`
}

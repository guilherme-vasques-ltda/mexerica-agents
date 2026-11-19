// O que a tela central "Rotinas" (D85) recebe do Hub (`routinesOverview`) e como a tela o recorta.
// O Hub calcula ocorrências, descrições e seções; o app só filtra e desenha.
import { useEffect, useState, useSyncExternalStore } from 'react'
import { isGroup, type Bot } from '@shared/models'
import { ALL, matchesWords, searchWords } from '../../lib/busca'
import { inGroup } from '../../lib/bot-groups'
import type { BotGroupsSnapshot } from '../../../shared/bot-groups'
import { startOfDay } from '../../lib/routine-calendar'
import type { BotStore } from '../../store/bot-store'
import { rawError, type Routine, type RoutineTrigger } from '../routines/routine'
import type { RunOutcome } from '../routines/run-outcome'
import { currentZone } from '../routines/time-zones'

/** D84: onde a rotina roda. Nulo até existirem os computadores da D83 (então, sempre o Hub). */
export type RunsOn = { kind: 'hub' } | { kind: 'computer'; id: string; name: string }

export interface OverviewRoutine extends Routine {
  /** A agenda em pt-BR, no fuso deste computador. */
  summary: string
  teamId: string
  runsOn: RunsOn | null
}

export interface Occurrence {
  routineId: string
  botId: string
  at: number
  trigger: number
  enabled: boolean
}

export interface OverviewRun {
  id: string
  routineId: string
  botId: string
  status: string
  createdAt: number
  startedAt?: number | null
  finishedAt?: number | null
  detail?: string | null
  /** I-023: desfecho gravado pelo Hub; ausente em execução antiga ou Hub antigo. */
  outcome?: RunOutcome | null
}

export interface Team {
  id: string
  name: string
  botIds: string[]
}

export interface Overview {
  now: number
  teams: Team[]
  routines: OverviewRoutine[]
  occurrences: Occurrence[]
  /** Rodam mais de 2 vezes numa mesma hora: vão para a faixa "Frequentes". */
  dense: string[]
  /** Fora da grade só neste período, pelo teto de cartões do Hub (as execuções continuam). */
  truncated?: string[]
  /** A agenda não pôde ser projetada (cron ou fuso inválido): sem cartões; a Lista avisa. */
  invalid?: string[]
  runs: OverviewRun[]
}

/** Uma marca no calendário: a próxima vez que a rotina roda, ou uma execução (passada ou ativa). */
export interface CalendarItem {
  id: string
  at: number
  routine: OverviewRoutine
  run: OverviewRun | null
  paused: boolean
  globalPaused?: boolean
  pauseUnknown?: boolean
}

/** O tipo do arrastar de um bot para a agenda (um só para a grade e o mês). */
export const BOT_DRAG_TYPE = 'application/x-donnabot-bot'

/** O bot sendo arrastado: o `dragover` só vê os tipos, então a prévia lê o nome daqui. */
export const dragging: { bot: Bot | null } = { bot: null }

export { ALL }

export const localZone = () => currentZone(Intl.DateTimeFormat().resolvedOptions().timeZone)

export const isActiveStatus = (status: string) => ['pending', 'starting', 'running', 'recovering'].includes(status)

/** Só webhook ou evento: não tem horário, fica na faixa "Sob demanda". */
export const isOnDemand = (routine: Routine) =>
  routine.triggers.length > 0 && routine.triggers.every((t) => t.type === 'webhook' || t.type === 'event')

/**
 * Os bots que podem ter rotinas (o Hub recusa rotina em grupo), na ordem da barra lateral (a de
 * "Meus bots"); os ocultos vêm depois, por nome. A mesma ordem em toda a tela.
 */
export function agentBots(store: BotStore): Bot[] {
  const shown = store.roster.filter((bot) => !isGroup(bot))
  const hidden = [...store.bots.values()].filter((bot) => bot.hidden && !isGroup(bot)).sort((a, b) => a.name.localeCompare(b.name, 'pt-BR'))
  return [...shown, ...hidden]
}

/** O começo de uma rotina criada a partir de um horário da agenda: toda semana, naquele dia e hora. */
export function weeklyAt(at: number): RoutineTrigger[] {
  const date = new Date(at)
  return [{ type: 'cron', expression: `${date.getMinutes()} ${date.getHours()} * * ${date.getDay()}`, timeZone: localZone() }]
}

export interface Filters { sectionId: string; botId: string; query: string }

/** D114: filtro e busca usam a organização do Hub, sem tratar seção como Time. */
export function filterRoutines(overview: Overview, filters: Filters, bots: Map<string, Bot>, snapshot: BotGroupsSnapshot | null): OverviewRoutine[] {
  const words = searchWords(filters.query)
  return overview.routines.filter((r) => (filters.sectionId === ALL || !snapshot || inGroup(snapshot, r.botId, filters.sectionId)) && (filters.botId === ALL || filters.botId === r.botId) && matchesWords(words, [r.name, bots.get(r.botId)?.name ?? '', overview.teams.find((team) => team.id === r.teamId)?.name ?? '']))
}

/** As marcas do calendário para estas rotinas: as próximas vezes (menos as frequentes) e as execuções. */
export function calendarItems(overview: Overview, routines: OverviewRoutine[], globalPaused: boolean | null = false): CalendarItem[] {
  const byId = new Map(routines.map((r) => [r.id, r]))
  const dense = new Set(overview.dense)
  const items: CalendarItem[] = []
  for (const o of overview.occurrences) {
    const routine = byId.get(o.routineId)
    if (routine && !dense.has(routine.id)) items.push({ id: `o:${o.routineId}:${o.trigger}:${o.at}`, at: o.at, routine, run: null, paused: !o.enabled || globalPaused !== false, globalPaused: o.enabled && globalPaused === true, pauseUnknown: o.enabled && globalPaused === null })
  }
  for (const run of overview.runs) {
    const routine = byId.get(run.routineId)
    // As execuções das frequentes também ficam na faixa: na grade seriam dezenas por dia.
    if (routine && !dense.has(routine.id)) items.push({ id: `r:${run.id}`, at: run.createdAt, routine, run, paused: false })
  }
  return items.sort((a, b) => a.at - b.at || a.routine.name.localeCompare(b.routine.name))
}

/** As marcas agrupadas pelo começo do dia local (a mesma chave dos dias da grade). */
export function itemsByDay(items: readonly CalendarItem[]): Map<number, CalendarItem[]> {
  const days = new Map<number, CalendarItem[]>()
  for (const item of items) {
    const day = startOfDay(item.at)
    const list = days.get(day)
    if (list) list.push(item)
    else days.set(day, [item])
  }
  return days
}

/**
 * O que a tela usa da store (nomes, cores, ordem, ocultos, conexão). Muda pouco: a tela não
 * re-renderiza com o resto (o streaming das conversas de qualquer bot).
 */
export function useRosterSignature(store: BotStore): string {
  return useSyncExternalStore(store.subscribe, () => rosterSignature(store))
}

function rosterSignature(store: BotStore): string {
  const bots = [...store.bots.values()].map((b) => [b.id, b.name, b.description, b.avatarColor, b.avatarShape, JSON.stringify([b.avatarConfig,b.mexeriquinha]), b.kind, b.hidden ? 1 : 0].join(':'))
  return [store.isOffline ? 'off' : 'on', store.roster.map((b) => b.id).join(','), ...bots].join('|')
}

export interface OverviewState {
  data: Overview | null
  error: string | null
  loading: boolean
  reload: () => void
}

/** `routinesOverview` de `[from, to)`, de novo a cada 5 s enquanto a tela está aberta. */
export function useOverview(store: BotStore | null, from: number, to: number): OverviewState {
  const [data, setData] = useState<Overview | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [nonce, setNonce] = useState(0)
  useEffect(() => {
    if (!store) return
    let stopped = false
    let timer: ReturnType<typeof setTimeout> | undefined
    setLoading(true)
    const tick = async () => {
      try {
        const result = await (await store.ready()).call<Overview>('routinesOverview', { from, to, timeZone: localZone() })
        if (stopped) return
        // Igual à anterior (o caso comum a cada 5 s): mantém o objeto e a grade não se refaz.
        setData((previous) => (previous && JSON.stringify(previous) === JSON.stringify({ ...result, now: previous.now }) ? previous : result))
        setError(null)
      } catch (e) {
        if (stopped) return
        setError(rawError(e))
      }
      setLoading(false)
      if (!stopped) timer = setTimeout(() => void tick(), 5000)
    }
    void tick()
    return () => {
      stopped = true
      clearTimeout(timer)
    }
  }, [store, from, to, nonce])
  return { data, error, loading, reload: () => setNonce((n) => n + 1) }
}

/** O Hub não conhece o método: é anterior a esta tela. */
export const isOldHub = (error: string) => /unknown method/i.test(error)

// Calendário das rotinas: anterior/próximo, "Hoje", Dia/Semana/Mês, a barra lateral (minicalendário e "Meus bots") ao lado de um cartão com a
// grade, a faixa "Sob demanda" e os estados de carregando, erro e vazio. Em janelas estreitas a
// barra lateral vira um botão que a abre por cima da grade (Esc fecha).
// D169 R5 (components/RoutinesCalendar, referência do dono): a barra do período fica dentro do painel
// do calendário, e a barra lateral é um painel ao lado, como na prévia.
import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import type { Bot } from '@shared/models'
import { IconButton, SegmentedChoice } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { localeDe } from '../../lib/fork-i18n/index.ts'
import { atMinutes, dayLabel, rangeLabel, startOfDay, startOfMonth, stepAnchor, timeLabel, type CalendarView } from '../../lib/routine-calendar'
import { BotsPanel } from './BotsPanel'
import { MiniMonth } from './MiniMonth'
import { MonthGrid } from './MonthGrid'
import { isManual, isOnDemand, type CalendarItem, type OverviewRoutine } from './model'
import { LoadState, OnDemandBand } from './parts'
import { idioma, texts } from './texts'
import { TimeGrid } from './TimeGrid'

/**
 * Para onde "Agendar" leva um bot: a próxima hora cheia de hoje quando hoje está no período (ou o
 * período já passou); senão as 09:00 do primeiro dia do período (o dia, no Dia; o dia 1, no Mês).
 */
export function scheduleTarget(view: CalendarView, anchor: number, range: { start: number; end: number }, today: number, now: number): { at: number; when: { dia: string; hora: string } | null } {
  const nextHour = () => {
    const at = new Date(now)
    at.setHours(at.getHours() + 1, 0, 0, 0)
    return { at: at.getTime(), when: null }
  }
  const showsToday = view === 'day' ? anchor === today : view === 'month' ? startOfMonth(anchor) === startOfMonth(today) : today >= range.start && today < range.end
  const day = view === 'day' ? startOfDay(anchor) : view === 'month' ? startOfMonth(anchor) : range.start
  if (showsToday || day < today) return nextHour()
  const at = atMinutes(day, 9 * 60)
  return { at, when: { dia: dayLabel(day, idioma()), hora: timeLabel(at, idioma()) } }
}

export function RoutineCalendar({ view, setView, anchor, setAnchor, range, today, now, routines, items, dense, truncated, bots, panelBots, botFilter, empty, query, clearQuery, hasData, disabled, loading, error, retry, onOpenRoutine, onCreate, onToggleBot }: {
  view: CalendarView
  setView: (view: CalendarView) => void
  anchor: number
  setAnchor: (day: number) => void
  range: { start: number; end: number; days: number[] }
  today: number
  now: number
  routines: OverviewRoutine[]
  items: CalendarItem[]
  dense: Set<string>
  /** Rotinas fora da grade neste período pelo teto de cartões do Hub (os nomes vão num aviso). */
  truncated: OverviewRoutine[]
  bots: Map<string, Bot>
  /** Os bots de "Meus bots" (do Time filtrado, sem os ocultos). */
  panelBots: Bot[]
  botFilter: string | null
  /** Nenhuma rotina em nenhum bot: a dica de como começar. */
  empty: boolean
  /** A busca do cabeçalho: sem resultado, um aviso com "Limpar busca". */
  query: string
  clearQuery: () => void
  /** Já chegou uma resposta: um erro depois disso vira aviso, sem cobrir a grade. */
  hasData: boolean
  disabled: boolean
  loading: boolean
  error: string | null
  retry: () => void
  onOpenRoutine: (routine: OverviewRoutine) => void
  onCreate: (at: number | null, botId: string | null) => void
  onToggleBot: (botId: string) => void
}) {
  const onDemand = routines.filter(isOnDemand)
  const manual = routines.filter(isManual)
  const frequent = routines.filter((r) => dense.has(r.id) && !isOnDemand(r) && !isManual(r))
  const [sidebarOpen, setSidebarOpen] = useState(false)
  const toggle = useRef<HTMLButtonElement>(null)
  const sidebar = useRef<HTMLElement>(null)
  const target = scheduleTarget(view, anchor, range, today, now)

  // A barra lateral por cima da grade (janela estreita): o foco entra nela e volta ao botão.
  useEffect(() => {
    if (sidebarOpen) sidebar.current?.querySelector<HTMLElement>('button:not([disabled]), input')?.focus()
  }, [sidebarOpen])
  const closeSidebar = (returnFocus: boolean) => {
    if (!sidebarOpen) return
    setSidebarOpen(false)
    if (returnFocus) toggle.current?.focus()
  }
  const onSidebarKey = (e: KeyboardEvent) => {
    if (e.key !== 'Escape' || !sidebarOpen) return
    e.preventDefault()
    e.stopPropagation()
    closeSidebar(true)
  }

  const openDay = (day: number) => {
    setAnchor(day)
    setView('day')
  }
  const create = (at: number | null, botId: string | null) => {
    closeSidebar(false)
    onCreate(at, botId)
  }

  return (
    <div className={`rc-calendar${sidebarOpen ? ' sidebar-open' : ''}`}>
      <div className="rc-calendar-body">
        {sidebarOpen ? <div className="rc-scrim" aria-hidden onClick={() => closeSidebar(false)} /> : null}
        <aside id="rc-sidebar" ref={sidebar} className="rc-sidebar" aria-label={texts.sidebar} onKeyDown={onSidebarKey}>
          <MiniMonth anchor={anchor} range={view === 'week' ? range : null} today={today} onSelect={(day) => {
            setAnchor(day)
            closeSidebar(true)
          }} />
          <BotsPanel bots={panelBots} selected={botFilter} onSelect={(botId) => {
            onToggleBot(botId)
            closeSidebar(true)
          }} onSchedule={(botId) => create(target.at, botId)} scheduleWhen={target.when} disabled={disabled} />
        </aside>
        <div className="rc-main">
          <div className="rc-toolbar">
            <button
              ref={toggle}
              className="rc-sidebar-toggle"
              aria-label={sidebarOpen ? texts.hideSidebar : texts.showSidebar}
              title={sidebarOpen ? texts.hideSidebar : texts.showSidebar}
              aria-expanded={sidebarOpen}
              aria-controls="rc-sidebar"
              onClick={() => setSidebarOpen((open) => !open)}
            >
              <Icon name="sidebar.left" size={14} weight="medium" />
            </button>
            <IconButton title={texts.previous[view]} icon="chevron.left" onClick={() => setAnchor(stepAnchor(view, anchor, -1))} />
            <IconButton title={texts.next[view]} icon="chevron.right" onClick={() => setAnchor(stepAnchor(view, anchor, 1))} />
            <button className="rc-today" onClick={() => setAnchor(startOfDay(Date.now()))} aria-label={texts.todayOn(new Date(today).toLocaleDateString(localeDe(idioma()), { day: 'numeric', month: 'long' }))}>{texts.today}</button>
            <h2 className="rc-range" aria-live="polite">{rangeLabel(view, anchor, idioma())}</h2>
            <div className="rc-period" role="group" aria-label={texts.viewLabel}>
              <SegmentedChoice selection={view} options={texts.views} onChange={setView} />
            </div>
          </div>
          <OnDemandBand onDemand={onDemand} manual={manual} frequent={frequent} bots={bots} onOpen={onOpenRoutine} />
          {empty && !loading && !error ? <p className="rc-banner">{texts.emptyCalendar}</p> : null}
          {!empty && hasData && query.trim() && routines.length === 0 ? (
            <p className="rc-banner" role="status">
              {texts.noResultsFor(query.trim())} <button className="rc-link" onClick={clearQuery}>{texts.clearSearch}</button>
            </p>
          ) : null}
          {truncated.length ? <p className="rc-banner" role="status">{texts.truncated(truncated.map((r) => r.name))}</p> : null}
          {error && hasData ? (
            <p className="rc-banner" role="alert">
              {texts.loadError} <button className="rc-link" onClick={retry}>{texts.retry}</button>
            </p>
          ) : error || loading ? (
            <LoadState loading={loading} error={error} retry={retry} />
          ) : null}
          {view === 'month' ? (
            <MonthGrid
              days={range.days}
              month={anchor}
              items={items}
              bots={bots}
              today={today}
              disabled={disabled}
              onOpen={(item) => onOpenRoutine(item.routine)}
              onOpenDay={openDay}
              onCreate={create}
            />
          ) : (
            <TimeGrid
              days={range.days}
              items={items}
              bots={bots}
              today={today}
              now={now}
              ready={!loading}
              disabled={disabled}
              onOpen={(item) => onOpenRoutine(item.routine)}
              onOpenDay={openDay}
              onCreate={create}
            />
          )}
        </div>
      </div>
    </div>
  )
}

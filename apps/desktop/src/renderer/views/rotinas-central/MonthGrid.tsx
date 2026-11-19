// A visão Mês (D85; o OpenMausBot só tem Dia e Semana): 6 semanas a partir do domingo, até 4
// rotinas por dia e "+N mais", que abre o Dia. Soltar um bot num dia agenda às 9 h. No teclado, o
// mês é um só Tab: as setas andam entre os dias e Enter abre o dia, onde cada rotina tem o seu
// Tab; as entradas e o "+N mais" ficam fora da ordem do Tab (o mouse continua abrindo-os).
import { useEffect, useMemo, useRef, useState, type DragEvent, type KeyboardEvent } from 'react'
import type { Bot } from '@shared/models'
import { addDays, atMinutes, dayLabel, startOfDay, timeLabel, weekdayShortNames } from '../../lib/routine-calendar'
import { BOT_DRAG_TYPE, itemsByDay, type CalendarItem } from './model'
import { botName, botStyle, itemClass, itemLabel } from './parts'
import { RunsOnBadge } from './RunsOnBadge'
import { idioma, texts } from './texts'

const SHOWN = 4
const DROP_HOUR = 9

export function MonthGrid({ days, month, items, bots, today, disabled, onOpen, onOpenDay, onCreate }: {
  days: number[]
  /** Um dia do mês mostrado: os de fora ficam apagados. */
  month: number
  items: CalendarItem[]
  bots: Map<string, Bot>
  today: number
  disabled: boolean
  onOpen: (item: CalendarItem) => void
  onOpenDay: (day: number) => void
  onCreate: (at: number, botId: string | null) => void
}) {
  const grid = useRef<HTMLDivElement>(null)
  const [focusDay, setFocusDay] = useState(() => (days.includes(today) ? today : startOfDay(month)))
  const [dropDay, setDropDay] = useState<number | null>(null)
  const moveFocus = useRef(false)
  const shownMonth = new Date(month).getMonth()
  const perDay = useMemo(() => itemsByDay(items), [items])

  useEffect(() => setFocusDay(days.includes(today) ? today : startOfDay(month)), [days[0]])
  useEffect(() => {
    if (!moveFocus.current) return
    moveFocus.current = false
    grid.current?.querySelector<HTMLButtonElement>(`[data-day="${focusDay}"]`)?.focus()
  }, [focusDay])

  const onKey = (e: KeyboardEvent) => {
    if (!(e.target instanceof HTMLElement) || !e.target.dataset.day) return
    const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 }[e.key]
    if (step === undefined) return
    const next = addDays(focusDay, step)
    if (!days.includes(next)) return
    e.preventDefault()
    moveFocus.current = true
    setFocusDay(next)
  }

  const dragOver = (e: DragEvent, day: number) => {
    if (disabled || !e.dataTransfer.types.includes(BOT_DRAG_TYPE)) return
    e.preventDefault()
    e.dataTransfer.dropEffect = 'copy'
    setDropDay(day)
  }
  const drop = (e: DragEvent, day: number) => {
    const botId = e.dataTransfer.getData(BOT_DRAG_TYPE)
    setDropDay(null)
    if (!botId || disabled) return
    e.preventDefault()
    onCreate(atMinutes(day, DROP_HOUR * 60), botId)
  }

  return (
    <div className="rc-month">
      <div className="rc-month-weekdays" aria-hidden>
        {weekdayShortNames(idioma()).map((name) => <span key={name}>{name}</span>)}
      </div>
      <div className="rc-month-grid" ref={grid} role="group" aria-label={`${texts.monthGrid}. ${texts.monthGridHelp}`} onKeyDown={onKey}>
        {days.map((day) => {
          const dayItems = perDay.get(day) ?? []
          const hidden = dayItems.length - SHOWN
          const classes = [
            'rc-month-cell',
            new Date(day).getMonth() !== shownMonth ? 'outside' : '',
            day === today ? 'today' : '',
            dropDay === day ? 'drop' : '',
          ].filter(Boolean).join(' ')
          return (
            <div
              key={day}
              className={classes}
              onDragOver={(e) => dragOver(e, day)}
              onDragLeave={(e) => {
                if (!(e.relatedTarget instanceof Node) || !e.currentTarget.contains(e.relatedTarget)) setDropDay(null)
              }}
              onDrop={(e) => drop(e, day)}
            >
              <button
                className="rc-month-day"
                data-day={day}
                tabIndex={day === focusDay ? 0 : -1}
                aria-current={day === today ? 'date' : undefined}
                aria-label={`${texts.openDay(dayLabel(day, idioma()))}${dayItems.length ? `, ${texts.routineCount(dayItems.length)}` : ''}`}
                onFocus={() => setFocusDay(day)}
                onClick={() => onOpenDay(day)}
              >
                {new Date(day).getDate()}
              </button>
              {dayItems.slice(0, hidden > 0 ? SHOWN - 1 : SHOWN).map((item) => (
                <button
                  key={item.id}
                  tabIndex={-1}
                  className={`rc-month-entry ${itemClass(item)}`}
                  style={botStyle(bots.get(item.routine.botId))}
                  aria-label={itemLabel(item, botName(bots, item.routine.botId))}
                  title={`${item.routine.name} · ${botName(bots, item.routine.botId)}`}
                  onClick={() => onOpen(item)}
                >
                  <span className="rc-dot" aria-hidden />
                  <span className="rc-month-time">{timeLabel(item.at, idioma())}</span>
                  <span className="rc-month-name" dir="auto">{item.routine.name}</span>
                  <RunsOnBadge runsOn={item.routine.runsOn} compact />
                </button>
              ))}
              {hidden > 0 ? (
                <button className="rc-month-more" tabIndex={-1} aria-label={texts.moreLabel(hidden + 1, dayLabel(day, idioma()))} onClick={() => onOpenDay(day)}>
                  {texts.more(hidden + 1)}
                </button>
              ) : null}
            </div>
          )
        })}
      </div>
    </div>
  )
}

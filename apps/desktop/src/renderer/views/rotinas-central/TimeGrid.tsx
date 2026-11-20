// Grade de horas do calendário: sem chamadas, redimensionar nem marcadores de limite; a linha da hora atual se
// atualiza; clicar num horário vazio cria; e um cursor de teclado (setas + Enter) faz o mesmo
// que arrastar um bot ou clicar.
import { useLayoutEffect, useMemo, useRef, useState, type DragEvent, type KeyboardEvent, type MouseEvent } from 'react'
import type { Bot } from '@shared/models'
import {
  atMinutes, dayLabel, formatGmtOffset, MARKER_MINUTES, minutesOfDay, packCollisions, SLOT_MINUTES, snapMinutes,
  timeLabel, weekdayShort,
} from '../../lib/routine-calendar'
import { BotAvatar } from '../../components/Avatar'
import { Icon } from '../../components/Icon'
import { BOT_DRAG_TYPE, dragging, itemsByDay, type CalendarItem } from './model'
import { botName, botStyle, itemClass, itemLabel, itemStatus } from './parts'
import { RunsOnBadge } from './RunsOnBadge'
import { idioma, texts } from './texts'

/** Altura de uma hora na grade (o CSS usa a mesma, `--rc-hour`): 44 px no desenho D165 (RoutinesCalendar). */
const HOUR = 44
const DROP_MINUTES = 15

interface Slot {
  day: number
  minutes: number
}

export function TimeGrid({ days, items, bots, today, now, ready, disabled, onOpen, onOpenDay, onCreate }: {
  days: number[]
  /** Os dados chegaram: a faixa acima muda a altura, então a grade volta para perto da hora atual. */
  ready: boolean
  items: CalendarItem[]
  bots: Map<string, Bot>
  today: number
  now: number
  disabled: boolean
  onOpen: (item: CalendarItem) => void
  onOpenDay: (day: number) => void
  /** Um horário escolhido (clique, Enter ou um bot solto nele); `botId` quando veio de um bot. */
  onCreate: (at: number, botId: string | null) => void
}) {
  const scroll = useRef<HTMLDivElement>(null)
  const [cursor, setCursor] = useState<Slot | null>(null)
  const [focused, setFocused] = useState(false)
  const [preview, setPreview] = useState<Slot | null>(null)
  const first = days[0]!
  // Os cartões de cada dia e suas colunas de sobreposição: só mudam com os dados ou o período.
  const perDay = useMemo(() => {
    const grouped = itemsByDay(items)
    return new Map(days.map((day) => {
      const dayItems = grouped.get(day) ?? []
      return [day, { items: dayItems, layout: packCollisions(dayItems.map((item) => ({ id: item.id, at: item.at, durationMinutes: MARKER_MINUTES }))) }]
    }))
  }, [items, days])

  // Abre perto da hora atual (2 h antes) quando hoje está no período; senão às 7 h. Meia hora de
  // folga: o rótulo da primeira hora fica inteiro abaixo do cabeçalho dos dias.
  useLayoutEffect(() => {
    const hour = days.includes(today) ? Math.max(0, Math.floor(minutesOfDay(now) / 60) - 2) : 7
    scroll.current?.scrollTo({ top: Math.max(0, hour * HOUR - HOUR / 2) })
    setCursor(null)
  }, [first, days.length, ready])

  const slotAt = (e: { clientY: number; currentTarget: HTMLElement }, day: number, step: number): Slot => {
    const rect = e.currentTarget.getBoundingClientRect()
    return { day, minutes: snapMinutes(((e.clientY - rect.top) / HOUR) * 60, step) }
  }

  const reveal = (minutes: number) => {
    const view = scroll.current
    if (!view) return
    const header = 52
    const top = (minutes / 60) * HOUR
    if (top < view.scrollTop) view.scrollTop = top - 8
    else if (top + HOUR / 2 > view.scrollTop + view.clientHeight - header) view.scrollTop = top + HOUR / 2 - view.clientHeight + header + 8
  }

  const startCursor = () => {
    setFocused(true)
    if (cursor) return
    const day = days.includes(today) ? today : first
    const minutes = Math.min(24 * 60 - SLOT_MINUTES, snapMinutes(minutesOfDay(now)) + SLOT_MINUTES)
    setCursor({ day, minutes })
    reveal(minutes)
  }

  const onKey = (e: KeyboardEvent) => {
    if (e.target !== e.currentTarget) return
    if (!cursor || !focused) {
      if (e.key.startsWith('Arrow')) {
        e.preventDefault()
        startCursor()
      }
      return
    }
    const index = days.indexOf(cursor.day)
    let next: Slot | null = null
    if (e.key === 'ArrowUp') next = { ...cursor, minutes: Math.max(0, cursor.minutes - SLOT_MINUTES) }
    else if (e.key === 'ArrowDown') next = { ...cursor, minutes: Math.min(24 * 60 - SLOT_MINUTES, cursor.minutes + SLOT_MINUTES) }
    else if (e.key === 'ArrowLeft') next = { ...cursor, day: days[Math.max(0, index - 1)]! }
    else if (e.key === 'ArrowRight') next = { ...cursor, day: days[Math.min(days.length - 1, index + 1)]! }
    else if ((e.key === 'Enter' || e.key === ' ') && !disabled) {
      e.preventDefault()
      onCreate(atMinutes(cursor.day, cursor.minutes), null)
      return
    }
    if (!next) return
    e.preventDefault()
    setCursor(next)
    reveal(next.minutes)
  }

  const onFocus = (e: React.FocusEvent) => {
    // O cursor é do teclado: um clique (foco sem `:focus-visible`) não o mostra.
    if (e.target === e.currentTarget && e.currentTarget.matches(':focus-visible')) startCursor()
  }

  const dragOver = (e: DragEvent<HTMLDivElement>, day: number) => {
    if (disabled || !e.dataTransfer.types.includes(BOT_DRAG_TYPE)) return
    e.preventDefault()
    e.dataTransfer.dropEffect = 'copy'
    const slot = slotAt(e, day, DROP_MINUTES)
    setPreview((current) => (current?.day === slot.day && current.minutes === slot.minutes ? current : slot))
  }
  const drop = (e: DragEvent<HTMLDivElement>, day: number) => {
    const botId = e.dataTransfer.getData(BOT_DRAG_TYPE)
    setPreview(null)
    if (!botId || disabled) return
    e.preventDefault()
    const slot = slotAt(e, day, DROP_MINUTES)
    onCreate(atMinutes(slot.day, slot.minutes), botId)
  }
  const clickEmpty = (e: MouseEvent<HTMLDivElement>, day: number) => {
    if (disabled || (e.target as HTMLElement).closest('.rc-card')) return
    const slot = slotAt(e, day, SLOT_MINUTES)
    onCreate(atMinutes(slot.day, slot.minutes), null)
  }

  const nowTop = (minutesOfDay(now) / 60) * HOUR
  const cursorAt = cursor ? atMinutes(cursor.day, cursor.minutes) : null
  return (
    <div className="rc-timegrid" style={{ '--rc-days': days.length } as React.CSSProperties}>
      <div className="rc-scroll" ref={scroll}>
        <div className="rc-columns rc-days-header">
          <span className="rc-gmt" title={texts.localZone} dir="ltr">{formatGmtOffset(-new Date(first).getTimezoneOffset())}</span>
          {days.map((day) => (
            <button key={day} className={`rc-day-head ${day === today ? 'today' : ''}`} aria-current={day === today ? 'date' : undefined} aria-label={texts.openDay(dayLabel(day, idioma()))} onClick={() => onOpenDay(day)}>
              <span className="rc-day-weekday">{weekdayShort(day, idioma())}</span>
              <span className="rc-day-number">{new Date(day).getDate()}</span>
            </button>
          ))}
        </div>
        <div
          className="rc-columns rc-hours-body"
          tabIndex={0}
          role="group"
          aria-roledescription={texts.timeGrid}
          aria-label={`${texts.timeGrid}. ${texts.timeGridHelp}`}
          onKeyDown={onKey}
          onFocus={onFocus}
          onBlur={(e) => e.target === e.currentTarget && setFocused(false)}
        >
          <div className="rc-hour-labels" aria-hidden>
            {Array.from({ length: 23 }, (_, index) => (
              <span key={index} style={{ top: (index + 1) * HOUR }}>{`${String(index + 1).padStart(2, '0')}:00`}</span>
            ))}
          </div>
          {days.map((day) => {
            const { items: dayItems, layout } = perDay.get(day) ?? { items: [], layout: new Map() }
            const ghost = preview?.day === day ? preview : null
            return (
              <div
                key={day}
                className={`rc-day-col ${day === today ? 'today' : ''}`}
                onClick={(e) => clickEmpty(e, day)}
                onDragOver={(e) => dragOver(e, day)}
                onDragLeave={(e) => {
                  if (!(e.relatedTarget instanceof Node) || !e.currentTarget.contains(e.relatedTarget)) setPreview(null)
                }}
                onDrop={(e) => drop(e, day)}
              >
                {day === today ? <div className="rc-now" style={{ top: nowTop }} aria-hidden /> : null}
                {ghost ? (
                  <div className={`rc-ghost${dragging.bot ? ' with-bot' : ''}`} style={{ ...(dragging.bot ? botStyle(dragging.bot) : {}), top: (ghost.minutes / 60) * HOUR }} aria-hidden>
                    {timeLabel(atMinutes(day, ghost.minutes), idioma())}
                    {dragging.bot ? ` · ${dragging.bot.name}` : ''}
                  </div>
                ) : null}
                {focused && cursor?.day === day ? (
                  <div className="rc-cursor" style={{ top: (cursor.minutes / 60) * HOUR }} aria-hidden>{timeLabel(cursorAt!, idioma())}</div>
                ) : null}
                {dayItems.map((item) => {
                  const place = layout.get(item.id) ?? { column: 0, columns: 1 }
                  const bot = bots.get(item.routine.botId)
                  // I-023: ✓ só em "Sem novidades" e "Com resultado" (a frase vai no rótulo acessível); falha e
                  // "precisa de você", um alerta e o texto; sem confirmação, só o texto, sem ✓.
                  const classes = itemClass(item).split(' ')
                  const succeeded = classes.includes('ok')
                  const mark = succeeded ? 'checkmark' : classes.includes('failed') || classes.includes('attention') ? 'exclamationmark.circle' : null
                  const status = succeeded ? null : itemStatus(item)
                  // Lado a lado, o cartão fica estreito: só o nome (hora e bot no title e no rótulo).
                  return (
                    <button
                      key={item.id}
                      className={`rc-card ${itemClass(item)}${place.columns > 1 ? ' narrow' : ''}`}
                      style={{
                        ...botStyle(bot),
                        top: (minutesOfDay(item.at) / 60) * HOUR,
                        left: `calc(${(place.column / place.columns) * 100}% + 2px)`,
                        width: `calc(${100 / place.columns}% - 4px)`,
                      }}
                      aria-label={itemLabel(item, botName(bots, item.routine.botId))}
                      title={`${itemLabel(item, botName(bots, item.routine.botId))}\n${item.routine.summary}`}
                      onClick={(e) => {
                        e.stopPropagation()
                        onOpen(item)
                      }}
                    >
                      <span className="rc-card-name">
                        {mark ? <Icon name={mark} size={8} weight="bold" /> : null}
                        {bot && place.columns === 1 ? <span className="rc-card-face" aria-hidden><BotAvatar bot={bot} size={14} animated={false} /></span> : null}
                        <span className="rc-card-title" dir="auto">{item.routine.name}</span>
                        <RunsOnBadge runsOn={item.routine.runsOn} compact />
                      </span>
                      <span className="rc-card-meta">
                        {timeLabel(item.at, idioma())} · <bdi dir="auto">{botName(bots, item.routine.botId)}</bdi>
                        {status ? ` · ${status}` : ''}
                      </span>
                    </button>
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>
      <span className="rc-sr" aria-live="polite">{focused && cursorAt !== null ? texts.createAt(texts.dayTime(dayLabel(cursorAt, idioma()), timeLabel(cursorAt, idioma()))) : ''}</span>
    </div>
  )
}

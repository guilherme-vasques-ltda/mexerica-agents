// Adaptado de OpenMausBot (Apache-2.0, commit 615fe65e17f88d99f12d34682750d9ae2fa402bc),
// src/components/routines/MiniMonth.tsx (alterado): semana a partir do domingo, no idioma do app, CSS do
// Donnabot, o período visível marcado e foco móvel pelas setas (um só Tab para o mês).
import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { IconButton } from '../../components/Controls'
import { addDays, addMonths, monthGrid, startOfDay, startOfMonth, weekdayInitials, weekdayNames } from '../../lib/routine-calendar'
import { localeDe } from '../../lib/fork-i18n/index.ts'
import { idioma, texts } from './texts'

export function MiniMonth({ anchor, range, today, onSelect }: {
  anchor: number
  /** A semana que o calendário mostra, marcada de leve. */
  range: { start: number; end: number } | null
  today: number
  onSelect: (day: number) => void
}) {
  const [month, setMonth] = useState(() => startOfMonth(anchor))
  const [focused, setFocused] = useState(() => startOfDay(anchor))
  const grid = useRef<HTMLDivElement>(null)
  const moveFocus = useRef(false)

  useEffect(() => {
    setMonth(startOfMonth(anchor))
    setFocused(startOfDay(anchor))
  }, [anchor])

  const days = monthGrid(month)
  const locale = localeDe(idioma())
  const label = new Date(month).toLocaleDateString(locale, { month: 'long', year: 'numeric' })
  const names = weekdayNames(idioma())

  useEffect(() => {
    if (!moveFocus.current) return
    moveFocus.current = false
    grid.current?.querySelector<HTMLButtonElement>(`[data-day="${focused}"]`)?.focus()
  }, [focused, month])

  const focusDay = (day: number) => {
    moveFocus.current = true
    setFocused(day)
    if (startOfMonth(day) !== month) setMonth(startOfMonth(day))
  }

  const onKey = (e: KeyboardEvent) => {
    const step = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -7, ArrowDown: 7 }[e.key]
    if (step !== undefined) {
      e.preventDefault()
      focusDay(addDays(focused, step))
    } else if (e.key === 'PageUp' || e.key === 'PageDown') {
      e.preventDefault()
      focusDay(addMonths(focused, e.key === 'PageUp' ? -1 : 1))
    }
  }

  // O dia focável: o focado, se estiver neste mês; senão o dia 1.
  const tabbable = days.includes(focused) ? focused : month
  return (
    <section aria-label={texts.miniCalendar} className="rc-mini">
      <div className="rc-mini-header">
        <span className="rc-mini-title">{label.charAt(0).toLocaleUpperCase(locale) + label.slice(1)}</span>
        <IconButton title={texts.previousMonth} icon="chevron.left" size={26} onClick={() => setMonth(addMonths(month, -1))} />
        <IconButton title={texts.nextMonth} icon="chevron.right" size={26} onClick={() => setMonth(addMonths(month, 1))} />
      </div>
      <div className="rc-mini-grid" aria-hidden>
        {weekdayInitials(idioma()).map((initial, index) => <span key={index} className="rc-mini-weekday" title={names[index]}>{initial}</span>)}
      </div>
      <div className="rc-mini-grid" ref={grid} onKeyDown={onKey}>
        {days.map((day) => {
          const date = new Date(day)
          const classes = [
            'rc-mini-day',
            date.getMonth() !== new Date(month).getMonth() ? 'outside' : '',
            day === today ? 'today' : '',
            day === startOfDay(anchor) ? 'selected' : '',
            range && day >= range.start && day < range.end ? 'in-range' : '',
          ].filter(Boolean).join(' ')
          return (
            <button
              key={day}
              data-day={day}
              className={classes}
              tabIndex={day === tabbable ? 0 : -1}
              aria-label={date.toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' })}
              aria-current={day === today ? 'date' : undefined}
              aria-pressed={day === startOfDay(anchor)}
              onFocus={() => setFocused(day)}
              onClick={() => onSelect(day)}
            >
              <span>{date.getDate()}</span>
            </button>
          )
        })}
      </div>
    </section>
  )
}

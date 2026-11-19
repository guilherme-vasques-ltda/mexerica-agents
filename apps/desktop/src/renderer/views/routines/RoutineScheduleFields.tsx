import { useEffect, useRef, useState, type ReactNode } from 'react'
import { CardSection, ChipChoice, ChoicePicker } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { TimeZonePicker } from './TimeZonePicker'
import { compiledCron, errorText, minuteLabel, repeatChoice, repeatChoices, type RepeatChoice, type RoutineScheduleDraft, type RoutineSchedulePreview } from './routine'
import './routines.css'
import { agendaDoHub, txWeekdays } from '../rotinas-central/editor-textos'
import { MAX_TIMES, addTime, badTimes, daysOfStyle, isClockStyle, removeTimeAt, setTimeAt, singleCron, sortedTimes, styleOfDays, timesProblem, validTime, type TimesProblem } from './routine-times'
import { agendaTexto, horaTexto, nHorariosTexto, problemaTexto, vezesTexto } from './routine-times-texto'
import { localeDe, traduzir } from '../../lib/fork-i18n/index.ts'
import { superficieIdioma, tradutor, useIdioma } from '../../lib/fork-i18n/estado'
const WEEK_ORDER = [1, 2, 3, 4, 5, 6, 0]
const minutes = Array.from({ length: 60 }, (_, m) => ({ id: m, label: minuteLabel(m) }))

/**
 * A schedule's fields in one group (how often, the day and time, the time zone), then what the
 * host read from them. Every day, Weekdays and Some days have one row of days and one pill per time of day
 * (all the days apply to every time). Custom edits the cron directly.
 */
export function RoutineScheduleFields({ schedule, update, preview, error, times, setTimes }: {
  schedule: RoutineScheduleDraft
  update: (change: Partial<RoutineScheduleDraft>) => void
  /** The host's reading of exactly these fields, once it has checked them. */
  preview: RoutineSchedulePreview | null
  error: string | null
  /** The times of day ("HH:MM"), one pill each; only the styles with pills use them. */
  times: string[]
  setTimes: (times: string[]) => void
}) {
  const repeat = repeatChoice(schedule.calendarStyle)
  // DON-917: os dias e os rótulos saem no idioma do app.
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const weekdays = txWeekdays(idioma)
  const clock = isClockStyle(repeat)
  const days = clock ? daysOfStyle(repeat, schedule.selectedDays) : []
  const problem = clock ? timesProblem(times, days) : null
  // Custom starts from the host's cron for the choice it replaces; empty while that check is
  // still out, never the cron the routine opened with.
  // From the pills, Custom starts from the one cron with the same times and days. When the times are not a minutes-by-hours
  // product there is none, and Custom is not offered: saving never changes when the routine runs.
  const customCron = clock ? singleCron(times, days) : null
  const choices = repeatChoices.filter((c) => c.id !== 'custom' || !clock || customCron !== null)
  const choose = (id: RepeatChoice) => {
    if (id === repeat) return
    if (id === 'custom') update({ calendarStyle: id, expression: customCron ?? compiledCron(preview) ?? '' })
    // From Every day, Some days starts with no day chosen (seven marked days would be Every day); from Weekdays it keeps them.
    else if (id === 'some') update({ calendarStyle: id, selectedDays: clock ? (days.length === 7 ? [] : days) : [1, 2, 3, 4, 5] })
    else update({ calendarStyle: id })
  }
  // A day tapped: the days are one set for every time; the choice follows them (all seven are Every day, Mon–Fri Weekdays).
  const toggleDay = (day: number) => {
    const next = days.includes(day) ? days.filter((d) => d !== day) : [...days, day]
    update({ calendarStyle: styleOfDays(next), selectedDays: next })
  }

  return (
    <CardSection footer={<SchedulePreview preview={preview} error={error} clock={clock ? { days, times, problem } : null} />}>
      <LabeledRow key="Repeat" label={t('rotinas.editor.repetir')}>
        <ChipChoice label={t('rotinas.editor.repetir')} selection={repeat} options={choices.map((c) => ({ id: c.id, label: t(`rotinas.editor.repetir.${c.id}`) }))} onChange={choose} />
      </LabeledRow>
      {clock ? (
        <LabeledRow key="Days" label={t('rotinas.editor.dias')}>
          <HStack gap={2} wrap role="group" aria-label={t('rotinas.editor.diasDaSemana')} className="schedule-days">
            {WEEK_ORDER.map((id) => weekdays.find((d) => d.id === id)!).map((d) => (
              <button key={d.id} aria-pressed={days.includes(d.id)} aria-label={d.title} title={d.title} className={`chip-choice${days.includes(d.id) ? ' on' : ''}`} onClick={() => toggleDay(d.id)}>
                {d.label}
              </button>
            ))}
            {times.length > 1 ? <Text size="caption" tone="tertiary" className="schedule-days-hint">{t('rotinas.editor.diasValem', { horarios: nHorariosTexto(idioma, times.length) })}</Text> : null}
          </HStack>
        </LabeledRow>
      ) : null}
      {repeat === 'hourly' ? (
        <LabeledRow key="At minute" label={t('rotinas.editor.noMinuto')}>
          <ChoicePicker language={superficieIdioma(idioma)} selection={schedule.minute} options={minutes} onChange={(minute) => update({ minute })} fill="var(--field)" />
        </LabeledRow>
      ) : repeat === 'custom' ? (
        <LabeledRow key="Cron" label={t('rotinas.editor.cron')} top>
          <VStack gap={6} grow>
            <input
              className="schedule-input routine-cron-input"
              // A expressão é código: sempre da esquerda para a direita e isolada, também em árabe.
              dir="ltr"
              aria-label={t('rotinas.editor.cron')}
              value={schedule.expression}
              placeholder="0 9 * * 1-5"
              spellCheck={false}
              autoCorrect="off"
              onChange={(e) => update({ expression: e.target.value })}
            />
            <Text size="caption" mono tone="tertiary" className="routine-cron-hint" dir="ltr">{t('rotinas.editor.cronCampos')}</Text>
          </VStack>
        </LabeledRow>
      ) : (
        <LabeledRow key="At" label={t('rotinas.editor.as')} top>
          <TimePills times={times} setTimes={setTimes} />
        </LabeledRow>
      )}
      <LabeledRow key="Time zone" label={t('rotinas.editor.fuso')}>
        <TimeZonePicker zone={schedule.zone} onChange={(zone) => update({ zone })} />
      </LabeledRow>
    </CardSection>
  )
}

/** A row of the routine editor's groups (`CardSection`): its label in a fixed column, then the field; `top` for a tall one. */
export function LabeledRow({ label, top = false, className, children }: { label: string; top?: boolean; className?: string; children: ReactNode }) {
  return (
    <HStack align={top ? 'flex-start' : 'center'} className={`labeled-row${top ? ' top' : ''}${className ? ` ${className}` : ''}`}>
      <Text size="callout" weight="medium" tone="secondary" className="labeled-row-label">{label}</Text>
      {children}
    </HStack>
  )
}

// A click on × or + must not first blur (and sort) the field the pointer leaves: the pill under the pointer would change places.
const keepSortOrder = (e: { preventDefault: () => void }) => e.preventDefault()

/**
 * The times of day, one pill each: the app's time field and a "×" (never fewer than one). "+ time" adds the next hour; at
 * the host's limit of 20 it goes away (no disabled button) and comes back when a pill is taken out. The pills sort when a
 * field is left, not while typing. Focus goes to the new pill and a live region says what changed, so nothing depends on motion.
 */
function TimePills({ times, setTimes }: { times: string[]; setTimes: (times: string[]) => void }) {
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const inputs = useRef<(HTMLInputElement | null)[]>([])
  const [focusAt, setFocusAt] = useState<number | null>(null)
  const [note, setNote] = useState('')
  const bad = badTimes(times)
  const many = times.length > 1
  useEffect(() => {
    if (focusAt === null) return
    inputs.current[focusAt]?.focus()
    setFocusAt(null)
  }, [focusAt])

  const add = () => {
    const next = addTime(times)
    const added = next.find((time) => !times.includes(time)) ?? next[next.length - 1]!
    setTimes(next)
    setFocusAt(next.indexOf(added))
    setNote(t('rotinas.editor.adicionado', { hora: horaTexto(idioma, added), n: next.length }))
  }
  const remove = (index: number) => {
    setTimes(removeTimeAt(times, index))
    setFocusAt(Math.min(index, times.length - 2))
    setNote(t('rotinas.editor.tirado', { hora: validTime(times[index]!) ? horaTexto(idioma, times[index]!) : '' }))
  }

  return (
    <HStack gap={8} wrap grow role="group" aria-label={many ? t('rotinas.editor.horarios') : t('rotinas.editor.horario')} className="time-pills">
      {times.map((time, i) => (
        <span key={i} className={`time-pill${bad[i] ? ' bad' : ''}${many ? '' : ' single'}`}>
          <input
            ref={(el) => void (inputs.current[i] = el)}
            type="time"
            className="time-pill-input"
            aria-label={many ? t('rotinas.editor.horarioN', { n: i + 1, total: times.length }) : t('rotinas.editor.horario')}
            aria-invalid={bad[i] || undefined}
            value={time}
            onChange={(e) => setTimes(setTimeAt(times, i, e.target.value))}
            onBlur={() => setTimes(sortedTimes(times))}
          />
          {many ? (
            <button className="press time-pill-x" onMouseDown={keepSortOrder} title={t('rotinas.editor.tirarHorario')} aria-label={t('rotinas.editor.tirarHorarioN', { hora: validTime(time) ? horaTexto(idioma, time) : String(i + 1) })} onClick={() => remove(i)}>
              <Icon name="xmark" size={9} weight="semibold" />
            </button>
          ) : null}
        </span>
      ))}
      {times.length < MAX_TIMES ? (
        <button className="press time-add" onMouseDown={keepSortOrder} aria-label={t('rotinas.editor.maisHorarioAria')} onClick={add}>
          <Icon name="plus" size={10} weight="semibold" />
          {t('rotinas.editor.maisHorario')}
        </button>
      ) : null}
      <span className="routines-sr-only" role="status">{note}</span>
    </HStack>
  )
}

/** One quiet line under the group: what the host read, or why it can't. */
function SchedulePreview({ preview, error, clock }: { preview: RoutineSchedulePreview | null; error: string | null; clock: { days: number[]; times: string[]; problem: TimesProblem | null } | null }) {
  const idioma = useIdioma()
  // The times the editor can say for itself (the host's text is one phrase per trigger): the message, or the schedule and its count.
  if (clock?.problem) return <Text size="callout" tone="danger" role="alert" className="schedule-preview">{problemaTexto(idioma, clock.problem, clock.times.length)}</Text>
  if (error) return <Text size="callout" tone="danger" className="schedule-preview">{errorText(error)}</Text>
  return (
    <VStack gap={2} className="schedule-preview" aria-live="polite">
      {preview ? (
        <>
          <HStack gap={8} wrap>
            <Text size="callout" weight="medium" tone="text">{clock ? agendaTexto(idioma, clock.days, clock.times) : agendaDoHub(preview, idioma)}</Text>
            {preview.nextRunAt ? (
              <Text size="callout" tone="secondary">
                {traduzir(idioma, 'rotinas.editor.proxima', { quando: new Date(preview.nextRunAt).toLocaleString(localeDe(idioma), { dateStyle: 'medium', timeStyle: 'short' }) })}
              </Text>
            ) : null}
          </HStack>
          {clock && clock.times.length > 1 ? <Text size="callout" tone="secondary" className="schedule-count">{vezesTexto(idioma, clock.times.length, clock.days.length)}</Text> : null}
          {preview.warning ? <Text size="callout" tone="secondary">{preview.warning}</Text> : null}
        </>
      ) : null}
    </VStack>
  )
}

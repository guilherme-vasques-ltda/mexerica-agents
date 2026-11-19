import { useEffect, useRef, useState } from 'react'
import { Button, CardSection, IconButton, SegmentedChoice, Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Dialog, ModalHeader } from '../../components/Overlay'
import { HStack, Spacer, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useStore } from '../../store/context'
import { emptyDraft, errorText, rawError, repeatChoice, type Routine, type RoutineScheduleDraft, type RoutineSchedulePreview, type RoutineTrigger } from './routine'
import { Field } from '../bots/parts'
import { LabeledRow, RoutineScheduleFields } from './RoutineScheduleFields'
import { RoutineWebhookPanel } from './RoutineWebhookPanel'
import { canDuplicate, duplicateRoutine, hasPendingEdit } from './routine-duplicate'
import { currentZone } from './time-zones'
import { daysOfStyle, isClockStyle, readClockTriggers, styleOfDays, timeOf, timesProblem, triggersToSave } from './routine-times'
import { horasTexto, nHorariosTexto } from './routine-times-texto'
import { agendaDoHub, txDuration } from '../rotinas-central/editor-textos'
import { superficieIdioma, tradutor, useIdioma } from '../../lib/fork-i18n/estado'
import './routines.css'

const saveShortcut = window.codync.platform === 'darwin' ? '⌘S' : 'Ctrl+S'

export function RoutineEditorView({ botId, routine, saved, duplicated, initialTriggers, subtitle }: {
  botId: string
  routine: Routine | null
  /** Donnabot (D85): a new routine's schedule to start from, and a line under the title. */
  initialTriggers?: RoutineTrigger[]
  subtitle?: string
  /** Called after a save (the routine) or a delete (null). */
  saved: (routine: Routine | null) => void
  duplicated: (routine: Routine) => void
}) {
  const store = useStore()
  // DON-917: o editor está no catálogo; declara o idioma escolhido e re-renderiza quando ele muda.
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const [name, setName] = useState(routine?.name ?? '')
  const [instruction, setInstruction] = useState(routine?.instruction ?? '')
  const [schedule, setSchedule] = useState<RoutineScheduleDraft>(emptyDraft)
  const [timeout, setTimeoutText] = useState(String(routine?.timeoutSeconds ?? 3600))
  const [busy, setBusy] = useState(false)
  const [duplicating, setDuplicating] = useState(false)
  const [savedKey, setSavedKey] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [advanced, setAdvanced] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const [preview, setPreview] = useState<RoutineSchedulePreview | null>(null)
  const [previewError, setPreviewError] = useState<string | null>(null)
  // The times of day ("HH:MM"), one pill each, for Every day / Weekdays / Some days (see routine-times.ts).
  const [times, setTimes] = useState<string[]>(['09:00'])
  const [checkedKey, setCheckedKey] = useState<string | null>(null)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [started, setStarted] = useState(false)
  // What the host checks and what is saved: the schedule fields and the times together.
  const key = JSON.stringify([schedule, times])
  const latest = useRef(key)
  latest.current = key
  const style = repeatChoice(schedule.calendarStyle)
  const clock = schedule.kind === 'cron' && isClockStyle(style) ? style : null
  const clockDays = clock ? daysOfStyle(clock, schedule.selectedDays) : []
  // Unchanged times, days and zone keep the saved triggers as they are (a list like "0 9,15 * * *" is not rewritten by a rename).
  const clockTriggers = () => triggersToSave(routine?.triggers, { times, days: clockDays, zone: schedule.zone })
  const clockProblem = clock ? timesProblem(times, clockDays) : null

  // Anything the bot set up (a one-off time, an interval, events, several triggers) stays as it is until replaced.
  // Cron triggers with the same days and zone (one per time, or a list of hours like "0 9,15 * * *") are the time pills instead.
  const keepsOriginal = routine ? (routine.triggers.length === 1 ? !['cron', 'webhook'].includes(routine.triggers[0]!.type) : routine.triggers.length > 0 && !readClockTriggers(routine.triggers)) : false
  const options = [
    ...(keepsOriginal ? [{ id: 'keep', label: t('rotinas.editor.definidaPeloBot') }] : []),
    { id: 'cron', label: t('rotinas.editor.agenda') },
    { id: 'webhook', label: t('rotinas.editor.webhook') },
  ]
  const pendingEdit = routine ? hasPendingEdit(routine, { name, instruction, timeout, scheduleKey: key }, savedKey) : false
  const canSave = !!name.trim() && !!instruction.trim() && !store.isOffline && !busy && !duplicating && loaded && checkedKey === key && previewError === null && !clockProblem

  const loadSchedule = async () => {
    try {
      const client = await store.ready()
      const triggers = routine?.triggers ?? initialTriggers ?? []
      const result = await client.call<RoutineSchedulePreview>('routineSchedule', {
        triggers,
        timeZone: currentZone(Intl.DateTimeFormat().resolvedOptions().timeZone),
      })
      const draft = { ...result.draft, calendarStyle: repeatChoice(result.draft.calendarStyle) }
      if (!['cron', 'webhook'].includes(draft.kind)) draft.kind = 'keep'
      // Every clock trigger becomes a pill, none cut; anything else stays as the host read it.
      const read = readClockTriggers(triggers)
      if (read) {
        draft.kind = 'cron'
        draft.zone = read.zone
        draft.calendarStyle = styleOfDays(read.days)
        draft.selectedDays = read.days
        setTimes(read.times)
      } else {
        // The host's reading of one clock time the pills do not take (a time of the day it could not read): its own hour and minute.
        setTimes([timeOf(draft.hour, draft.minute)])
        if (draft.kind === 'cron' && result.draft.calendarStyle === 'weekly') draft.selectedDays = [result.draft.weekday]
      }
      setSchedule(draft)
      setPreview(result)
      setCheckedKey(null)
      setLoaded(true)
      setError(null)
    } catch (e) {
      setError(rawError(e))
    }
  }

  const checkSchedule = async () => {
    const requested = latest.current
    setPreview(null)
    setPreviewError(null)
    setCheckedKey(null)
    // The times are checked as the triggers that will be saved: one cron each, the same days and zone.
    if (clockProblem) return
    try {
      const body = clock ? { triggers: clockTriggers(), timeZone: schedule.zone } : { draft: schedule }
      const result = await (await store.ready()).call<RoutineSchedulePreview>('routineSchedule', body)
      if (latest.current !== requested) return
      setPreview(result)
      setCheckedKey(requested)
    } catch (e) {
      if (latest.current !== requested) return
      setPreviewError(rawError(e))
    }
  }

  useEffect(() => {
    void loadSchedule()
  }, [])

  useEffect(() => {
    if (loaded) setSavedKey(key)
  }, [loaded, routine?.id])

  // The host checks each edit to the schedule, after a short pause in typing.
  useEffect(() => {
    if (!loaded) return
    const timer = setTimeout(() => void checkSchedule(), 300)
    return () => clearTimeout(timer)
  }, [key])

  const save = async () => {
    if (!canSave) return
    // Saving ends editing (a new webhook routine stays open on its URL and key).
    ;(document.activeElement as HTMLElement | null)?.blur()
    setBusy(true)
    setError(null)
    try {
      const client = await store.ready()
      const result = await client.call<{ routine: Routine }>('saveRoutine', {
        botId,
        id: routine?.id ?? null,
        name,
        instruction,
        // One routine, one trigger per time when the schedule changed (nothing is cut: up to the host's 20), the saved triggers otherwise; the rest of the schedule kinds go as a draft.
        ...(clock ? { triggers: clockTriggers() } : { schedule }),
        timeoutSeconds: timeout,
      })
      saved(result.routine)
    } catch (e) {
      setError(rawError(e))
    } finally {
      setBusy(false)
    }
  }

  // ⌘S saves.
  const saveRef = useRef(save)
  saveRef.current = save
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault()
        void saveRef.current()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const update = (change: Partial<RoutineScheduleDraft>) => setSchedule((s) => ({ ...s, ...change }))
  const shownPreview = checkedKey === key ? preview : null

  const triggerFields = () => {
    switch (schedule.kind) {
      case 'cron':
        return <RoutineScheduleFields schedule={schedule} update={update} preview={shownPreview} error={previewError} times={times} setTimes={setTimes} />
      case 'webhook':
        return routine && routine.triggers.some((t) => t.type === 'webhook' || t.type === 'event') ? (
          <RoutineWebhookPanel botId={botId} routineId={routine.id} />
        ) : (
          <Note icon="link" title={t('rotinas.editor.webhookRoda')} detail={t('rotinas.editor.salveUrl')} />
        )
      default:
        return <Note icon="sparkles" title={shownPreview ? agendaDoHub(shownPreview, idioma) : (routine?.triggerDescriptions.join(' · ') ?? '')} detail={t('rotinas.editor.criadaPeloBot')} />
    }
  }

  return (
    <VStack className="routine-editor idioma-superficie" {...superficieIdioma(idioma)}>
      <ModalHeader icon={schedule.kind === 'webhook' ? 'link' : 'clock'} title={routine ? t('rotinas.editor.editar') : t('rotinas.editor.nova')} subtitle={subtitle} closeTitle={t('rotinas.editor.fechar')} />
      {/* The card fits the form; it scrolls only when taller than the window allows. */}
      <div className="routine-form-scroll">
        <fieldset className="routine-form" disabled={busy || !loaded}>
          <VStack gap={24} className="routine-form-body">
            <VStack gap={16}>
              <Field label={t('rotinas.editor.nome')}>
                <input className="routine-input" dir="auto" value={name} placeholder={t('rotinas.editor.nomeExemplo')} onChange={(e) => setName(e.target.value)} />
              </Field>
              <Field label={t('rotinas.editor.instrucao')}>
                <textarea className="routine-input" dir="auto" value={instruction} placeholder={t('rotinas.editor.instrucaoDica')} onChange={(e) => setInstruction(e.target.value)} />
              </Field>
            </VStack>
            <VStack gap={10}>
              <HStack gap={12}>
                <Text tone="text" grow>{t('rotinas.editor.quando')}</Text>
                <SegmentedChoice selection={schedule.kind} options={options} onChange={(kind) => update({ kind })} />
              </HStack>
              {loaded ? (
                triggerFields()
              ) : (
                <HStack as="span" gap={6} className="routine-loading">
                  <Icon name="clock" size={12} />
                  {t('rotinas.editor.carregandoAgenda')}
                </HStack>
              )}
            </VStack>
            <VStack gap={10}>
              <button className="press routine-advanced" aria-expanded={advanced} onClick={() => setAdvanced((a) => !a)}>
                {t('rotinas.editor.execucao')}
                <span className={`routine-chevron ${advanced ? 'open' : ''}`}>
                  <Icon name="chevron.down" size={10} weight="semibold" />
                </span>
                <Spacer />
                {!advanced ? <Text tone="tertiary">{t('rotinas.editor.tempoLimiteValor', { duracao: txDuration(timeout, idioma) })}</Text> : null}
              </button>
              {advanced ? (
                <div className="routine-fade">
                  <CardSection footer={t('rotinas.editor.tempoLimiteDica')}>
                    <LabeledRow label={t('rotinas.editor.tempoLimite')}>
                      <input className="schedule-input routine-timeout" aria-label={t('rotinas.editor.tempoLimiteSegundos')} value={timeout} placeholder="3600" onChange={(e) => setTimeoutText(e.target.value)} />
                      <Text size="callout" tone="secondary">{t('rotinas.editor.segundos')}</Text>
                    </LabeledRow>
                  </CardSection>
                </div>
              ) : null}
            </VStack>
          </VStack>
        </fieldset>
      </div>

      <VStack gap={12} className="routine-editor-footer">
        {previewError !== null ? (
          <button className="press routine-retry" disabled={store.isOffline} onClick={() => void checkSchedule()}>
            {t('rotinas.editor.conferirAgenda')}
          </button>
        ) : null}
        {!loaded && error !== null ? (
          <button className="press routine-retry" onClick={() => void loadSchedule()}>
            {t('rotinas.editor.recarregarAgenda')}
          </button>
        ) : null}
        {clock && times.length > 1 ? <Text size="caption" tone="tertiary">{t('rotinas.editor.valeParaTodos', { horarios: nHorariosTexto(idioma, times.length) })}</Text> : null}
        {schedule.kind !== 'cron' && previewError ? <Text size="caption" tone="danger">{errorText(previewError)}</Text> : null}
        {error ? (
          <HStack as="span" gap={6} align="flex-start" className="routine-error" role="alert">
            <Icon name="exclamationmark.circle" size={12} className="routine-error-icon" />
            {errorText(error)}
          </HStack>
        ) : null}
        {store.isOffline ? (
          <HStack as="span" gap={6} className="routine-offline">
            <Icon name="wifi.slash" size={10} />
            {t('rotinas.editor.conecteHub')}
          </HStack>
        ) : null}
        <HStack gap={4}>
          {routine ? (
            <>
              <IconButton title={t('rotinas.editor.apagar')} icon="trash" onClick={() => setConfirmDelete(true)} />
              {canDuplicate(routine, store.isOffline) ? (
                <IconButton
                  title={duplicating ? t('rotinas.editor.duplicando') : pendingEdit ? t('rotinas.editor.duplicarSalveAntes') : t('rotinas.editor.duplicar')}
                  icon="doc.on.doc"
                  disabled={busy || duplicating || pendingEdit}
                  onClick={async () => {
                    setDuplicating(true)
                    setError(null)
                    try {
                      duplicated(await duplicateRoutine(await store.ready(), botId, routine.id))
                    } catch (e) {
                      setError(rawError(e))
                    } finally {
                      setDuplicating(false)
                    }
                  }}
                />
              ) : null}
              <IconButton
                title={started ? t('rotinas.editor.iniciada') : t('rotinas.editor.rodarAgora')}
                icon={started ? 'checkmark' : 'play.circle'}
                disabled={started || store.isOffline}
                onClick={async () => {
                  try {
                    await (await store.ready()).call('runRoutine', { botId, id: routine.id })
                    setStarted(true)
                  } catch (e) {
                    setError(rawError(e))
                  }
                }}
              />
            </>
          ) : null}
          <Spacer />
          <Button disabled={!canSave} onClick={() => void save()} title={`${t('rotinas.editor.salvar')} (${saveShortcut})`}>
            {busy ? <Spinner size={12} /> : null}
            {busy ? t('rotinas.editor.salvando') : routine ? t('rotinas.editor.salvarAlteracoes') : t('rotinas.editor.criar')}
            {busy ? null : <span className="button-shortcut">{saveShortcut}</span>}
          </Button>
        </HStack>
      </VStack>

      <Dialog
        open={confirmDelete}
        title={t('rotinas.editor.apagarPergunta')}
        cancel={t('rotinas.editor.cancelar')}
        language={superficieIdioma(idioma)}
        message={clock && times.length > 1 && !clockProblem ? t('rotinas.editor.apagarAvisoVarios', { horarios: nHorariosTexto(idioma, times.length), horas: horasTexto(idioma, times) }) : t('rotinas.editor.apagarAviso')}
        actions={[
          {
            title: t('rotinas.editor.apagar'),
            destructive: true,
            action: async () => {
              if (!routine) return
              try {
                await (await store.ready()).call('deleteRoutine', { botId, id: routine.id })
                saved(null)
              } catch (e) {
                setError(rawError(e))
              }
            },
          },
        ]}
        onClose={() => setConfirmDelete(false)}
      />
    </VStack>
  )
}

/** A trigger that has no fields here: what it does, and what to do about it. */
function Note({ icon, title, detail }: { icon: string; title: string; detail: string }) {
  return (
    <CardSection>
      <HStack gap={12} align="flex-start" className="routine-note">
        <Icon name={icon} size={13} color="var(--context-ink-2, var(--ink-2))" className="routine-note-icon" />
        <VStack gap={3}>
          <Text size="callout" weight="medium" tone="text">{title}</Text>
          <Text size="callout" tone="secondary">{detail}</Text>
        </VStack>
      </HStack>
    </CardSection>
  )
}

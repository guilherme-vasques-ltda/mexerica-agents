// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useEffect, useMemo, useRef, useState } from 'react'
import { IconButton, Switch } from '../../components/Controls'
import { Sheet } from '../../components/Overlay'
import { HStack, Spacer, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useScreenViewed } from '../../store/analytics'
import { useModel } from '../../lib/observable'
import { RoutinesPauseModel } from '../../store/routines-pause'
import { useStore } from '../../store/context'
import { errorText, rawError, isActiveRun, isManual, type Routine, type RoutineListing, type RoutineRun } from './routine'
import { awaitingRelease } from './routine-duplicate'
import { outcomeLabel, runOutcomeText } from './run-outcome'
import { RoutineEditorView } from './RoutineEditorView'
import { useRoutineCopyFeedback } from './useRoutineCopyFeedback'
import { routinesScreen } from '../rotinas-central/navigation'
import type { AppLanguage } from '../../../shared/fork-idioma.ts'
import { localeDe } from '../../lib/fork-i18n/index.ts'
import { superficieIdioma, tradutor, useIdioma } from '../../lib/fork-i18n/estado'
import './routines.css'

/** The open editor: on a routine, or (`routineId` null) setting up a new one. `id` is one per opening. */
interface EditorTarget {
  id: string
  routineId: string | null
}

/** I-023: o rótulo vem do desfecho (`runOutcomeText`); "Concluída" e "Encerrada" deixam de existir. */
const runLabel = (run: RoutineRun, idioma: AppLanguage) => runOutcomeText(run.status, run.outcome, idioma).label

/** The row's second line: what's happening now, else when it runs. I-023 R2: `detail` keeps the outcome's sentence
 * (what is missing, why it failed, "Segundo o bot") for the hint and the screen reader. */
export function routineLine(routine: Routine, runs: RoutineRun[], idioma: AppLanguage): { text: string; detail?: string } {
  const t = tradutor(idioma)
  const active = runs.find((r) => r.routineId === routine.id && isActiveRun(r))
  if (active) return { text: runLabel(active, idioma) }
  if (routine.lastError) return { text: t('rotinas.painel.atencao') }
  const last = runs.find((r) => r.routineId === routine.id)
  const outcome = last ? runOutcomeText(last.status, last.outcome, idioma) : null
  if (outcome && (outcome.tone === 'danger' || outcome.tone === 'attention' || outcome.unconfirmed)) {
    return { text: t('rotinas.painel.ultima', { estado: outcome.label.toLocaleLowerCase(localeDe(idioma)) }), detail: outcome.sentence ? outcomeLabel(outcome) : undefined }
  }
  if (awaitingRelease(routine)) return { text: t('rotinas.central.aguardaLiberacao') }
  if (isManual(routine)) return { text: t('rotinas.central.manual') }
  return { text: routine.triggerDescriptions.join(' · ') }
}

/** The routine list in the details panel (and the routines sheet); a row opens its editor. */
export function RoutinesView({ botId, initialId, onClose, previewLimit }: { botId: string; initialId: string | null; onClose?: () => void; previewLimit?: number }) {
  const store = useStore()
  const pause = useMemo(() => new RoutinesPauseModel(store), [store])
  useModel(pause)
  useEffect(() => { pause.start(); return () => pause.stop() }, [pause])
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const [expanded, setExpanded] = useState(false)
  const [routines, setRoutines] = useState<Routine[]>([])
  const [runs, setRuns] = useState<RoutineRun[]>([])
  const [editing, setEditing] = useState<EditorTarget | null>(null)
  const currentEditor = useRef(editing)
  currentEditor.current = editing
  const { copyId, setCopyId, copyRow } = useRoutineCopyFeedback()
  const [loaded, setLoaded] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const current = useRef(botId)
  current.current = botId

  const open = (routineId: string | null) => {
    setCopyId(null)
    setEditing({ id: crypto.randomUUID(), routineId })
  }
  // The list is part of the details panel; a routine's editor is the routines screen.
  useScreenViewed(editing ? 'routines' : null, editing?.id)

  const load = async () => {
    const asked = botId
    try {
      const result = await (await store.ready()).call<RoutineListing>('routines', { botId: asked })
      if (current.current !== asked) return
      setRoutines(result.routines)
      setRuns(result.runs)
      setLoaded(true)
      setLoadError(null)
    } catch (e) {
      if (current.current === asked) setLoadError(rawError(e))
    }
  }

  useEffect(() => {
    if (initialId) open(initialId)
  }, [initialId])

  useEffect(() => {
    setCopyId(null)
    setRoutines([])
    setRuns([])
    setLoaded(false)
    let stopped = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const tick = async () => {
      await load()
      if (!stopped) timer = setTimeout(tick, 3000)
    }
    void tick()
    return () => {
      stopped = true
      clearTimeout(timer)
    }
  }, [botId])

  const action = async (method: string, routine: Routine, enabled?: boolean) => {
    if (busy) return
    setBusy(true)
    try {
      await (await store.ready()).call(method, { botId, id: routine.id, enabled: enabled ?? null })
      setError(null)
      await load()
    } catch (e) {
      setError(rawError(e))
    } finally {
      setBusy(false)
    }
  }

  const summary = (routine: Routine) => routineLine(routine, runs, idioma)

  const onSaved = (target: EditorTarget, saved: Routine | null) => {
    if (saved) setRoutines((list) => (list.some((r) => r.id === saved.id) ? list.map((r) => (r.id === saved.id ? saved : r)) : [...list, saved]))
    // A new webhook routine stays open: its URL and key exist only now.
    if (target.routineId === null && saved && saved.triggers.some((t) => t.type === 'webhook')) {
      setEditing({ id: target.id, routineId: saved.id })
    } else {
      setEditing(null)
    }
    void load()
  }

  const editingRoutine = routines.find((r) => r.id === editing?.routineId) ?? null
  return (
    <VStack gap={onClose ? 16 : 10} className="routines idioma-superficie" {...superficieIdioma(idioma)}>
      <HStack>
        <Text size="body" weight="semibold" className="routines-title">{t('rotinas.central.titulo')}</Text>
        <Spacer />
        <IconButton title={t('rotinas.painel.pedir')} icon="text.bubble" disabled={store.isOffline} onClick={() => {
          store.setRoutineDraft(botId, `${t('rotinas.painel.rascunho')} `)
          onClose?.()
        }} />
        <IconButton title={t('rotinas.painel.criar')} icon="plus" disabled={store.isOffline} onClick={() => open(null)} />
        <IconButton title={t('rotinas.central.abrir')} icon="calendar" onClick={() => routinesScreen.show(botId)} />
        {onClose ? <IconButton title={t('rotinas.painel.fechar')} icon="xmark" onClick={onClose} /> : null}
      </HStack>
      {loadError ? <Text size="caption" tone="danger">{errorText(loadError)}</Text> : null}
      {error ? <Text size="caption" tone="danger" className="routines-error">{errorText(error)}</Text> : null}
      <Text as="div" size="callout" tone="secondary" className={copyId ? undefined : 'routines-sr-only'} role="status">
        {copyId ? t('rotinas.editor.duplicada') : ''}
      </Text>
      {routines.length === 0 ? (
        <Text as="div" size="callout" tone="secondary" className="outlined-card routines-empty">
          {t(loaded ? 'rotinas.painel.vazio' : 'rotinas.central.carregando')}
        </Text>
      ) : (
        <VStack gap={8}>
          {(previewLimit && !expanded ? routines.slice(0, previewLimit) : routines).map((routine) => (
            <HStack key={routine.id} gap={12} className="outlined-card routines-card">
              <button ref={copyId === routine.id ? copyRow : undefined} className="routine-row" onClick={() => open(routine.id)}>
                <Text tone="text" dir="auto">{routine.name}</Text>
                {(() => {
                  const line = summary(routine)
                  return (
                    <Text size="caption" tone={routine.lastError ? 'danger' : 'secondary'} title={line.detail}>
                      {line.text}
                      {line.detail ? <span className="routines-sr-only">. {line.detail}</span> : null}
                    </Text>
                  )
                })()}
              </button>
              <HStack as="span" align="stretch" aria-label={routine.name}>
                {isManual(routine) && routine.enabled ? <IconButton title={t('rotinas.central.rodarAgoraNome', { nome: routine.name })} icon="play.circle" disabled={busy || store.isOffline || pause.paused !== false} onClick={() => void action('runRoutine', routine)} /> : null}
                <Switch label={t('rotinas.painel.ativar', { nome: routine.name })} on={routine.enabled} disabled={busy || store.isOffline} onChange={(on) => void action('setRoutineEnabled', routine, on)} />
              </HStack>
            </HStack>
          ))}
        </VStack>
      )}
      {previewLimit && routines.length > previewLimit ? <button className="details-show-more" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>{t(expanded ? 'rotinas.painel.mostrarMenos' : 'rotinas.painel.mostrarMais')}</button> : null}
      {/* A routine opened before the list arrived waits for it. */}
      <Sheet language={superficieIdioma(idioma)} open={editing !== null && (editing.routineId === null || editingRoutine !== null)} onClose={() => setEditing(null)} width={540}>
        {editing ? (
          <RoutineEditorView
            key={editing.id}
            botId={botId}
            routine={editingRoutine}
            saved={(saved) => onSaved(editing, saved)}
            duplicated={(routine) => {
              if (current.current !== botId) return
              if (currentEditor.current?.id === editing.id) {
                setExpanded(true)
                setCopyId(routine.id)
                onSaved(editing, routine)
              } else {
                void load()
              }
            }}
          />
        ) : null}
      </Sheet>
    </VStack>
  )
}

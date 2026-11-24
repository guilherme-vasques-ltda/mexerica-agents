// Donnabot (D85): a tela central "Rotinas", com as rotinas de todos os bots: Voltar, "Buscar rotinas", filtros por seção
// e por bot, Lista ou Calendário e "+ Nova". A lista de rotinas de cada bot (painel de detalhes)
// continua.
import { memo, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { isGroup, type Bot } from '@shared/models'
import { BotAvatar } from '../../components/Avatar'
import { Button, ChoicePicker, IconButton, SearchField, SegmentedChoice } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { CascaVazia } from '../../components/toques-mexerica/Toques'
import { groupRoster, inGroup, UNGROUPED } from '../../lib/bot-groups'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { useModel } from '../../lib/observable'
import { pref, usePref } from '../../lib/prefs'
import { addDays, startOfDay, viewRange, type CalendarView } from '../../lib/routine-calendar'
import type { AppModel } from '../../store/app-model'
import type { BotStore } from '../../store/bot-store'
import { StoreContext, useApp, useStoreRef } from '../../store/context'
import { errorText, rawError } from '../routines/routine'
import { useRoutineCopyFeedback } from '../routines/useRoutineCopyFeedback'
import { ALL, agentBots, calendarItems, filterRoutines, type Filters, type OverviewRoutine, useOverview, useRosterSignature } from './model'
import { routinesScreen } from './navigation'
import { LoadState } from './parts'
import { RoutineCalendar } from './RoutineCalendar'
import { RoutineList } from './RoutineList'
import { GlobalPause } from './GlobalPause'
import { RoutinesPauseModel } from '../../store/routines-pause'
import { RoutineSheet, type EditorTarget } from './RoutineSheet'
import { texts } from './texts'
import { AvisoInstalacaoLocal } from '../fork/aviso-instalacao-local/AvisoInstalacaoLocal'
import { avisaPrimeiraAgendada, ehHubDesteComputador } from '../fork/aviso-instalacao-local/aviso-instalacao-local'
import './routines-central.css'

export { routinesScreen }

type Mode = 'list' | 'calendar'
const modePref = pref<Mode>('routinesCentralMode', 'calendar')
const viewPref = pref<CalendarView>('routinesCentralView', 'week')

/** O botão que abre a tela (barra lateral e trilho compacto); o foco volta a ele ao fechar. */
export function RoutinesButton({ size, className }: { size: number; className?: string }) {
  useIdioma()
  return <IconButton title={texts.open} icon="calendar" size={size} className={`rc-open ${className ?? ''}`} onClick={() => routinesScreen.show()} />
}

/** O Hub da tela: o do bot selecionado, se estiver conectado; senão o primeiro conectado (D76: um só). */
function pickStore(app: AppModel): BotStore | null {
  const stores = app.computers.map((c) => app.store(c.id)).filter((s): s is BotStore => !!s)
  const online = stores.filter((s) => s.connection.kind === 'online' && !s.mismatch)
  const selected = app.selectedStore
  return (selected && online.includes(selected) ? selected : null) ?? online[0] ?? selected ?? stores[0] ?? null
}

export function RoutinesCentralView() {
  const app = useApp()
  const store = pickStore(app)
  if (!store) {
    return (
      <Page>
        <div className="rc-content">
          <div className="rc-state"><span>{texts.noHub}</span></div>
        </div>
      </Page>
    )
  }
  return (
    <StoreContext.Provider value={store}>
      <RoutinesPage />
    </StoreContext.Provider>
  )
}

/** A hora atual, a cada 30 s: a linha da hora e o "hoje" andam sozinhos. */
function useNow() {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 30_000)
    return () => clearInterval(timer)
  }, [])
  return now
}

function Page({ header, children }: { header?: ReactNode; children: ReactNode }) {
  const back = useRef<HTMLButtonElement>(null)
  // DON-917: a tela toda está no catálogo; declara o idioma escolhido (e a direção) e re-renderiza quando ele muda.
  const idioma = useIdioma()
  // O foco vai para Voltar ao abrir.
  useEffect(() => back.current?.focus({ preventScroll: true }), [])
  return (
    <div className="rc-page idioma-superficie" {...superficieIdioma(idioma)}>
      <header className="rc-header drag">
        <button ref={back} className="rc-back no-drag" aria-label={texts.back} title={texts.back} onClick={() => routinesScreen.close()}>
          <Icon name="arrow.left" size={14} weight="medium" />
        </button>
        <h1 className="rc-title">{texts.title}</h1>
        <div className="rc-header-end no-drag">{header}</div>
      </header>
      {children}
    </div>
  )
}

/** A tela segue só o que usa da store (nomes, ordem, conexão), não o streaming das conversas. */
const RoutinesPage = memo(function RoutinesPage() {
  const t = useT()
  const app = useApp()
  const store = useStoreRef()
  // O memo não re-renderiza com o idioma sozinho: os textos são lidos na hora (texts.ts).
  const idioma = useIdioma()
  const pause = useMemo(() => new RoutinesPauseModel(store), [store])
  useModel(pause)
  useEffect(() => { pause.start(); return () => pause.stop() }, [pause])
  const sectionSnapshot = useModel(store.botGroups).snapshot
  const sectionChoices = sectionSnapshot ? groupRoster([], sectionSnapshot) : []
  const signature = useRosterSignature(store)
  const [mode, setMode] = usePref(modePref)
  const [view, setView] = usePref(viewPref)
  const [anchor, setAnchor] = useState(() => startOfDay(Date.now()))
  const [filters, setFilters] = useState<Filters>(() => ({ sectionId: ALL, botId: routinesScreen.botId ?? ALL, query: '' }))
  const [editor, setEditor] = useState<EditorTarget | null>(null)
  const currentEditor = useRef(editor)
  currentEditor.current = editor
  const { copyId, setCopyId, copyRow } = useRoutineCopyFeedback()
  const [busy, setBusy] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const search = useRef<HTMLInputElement>(null)
  const now = useNow()
  const today = startOfDay(now)
  const range = useMemo(() => viewRange(view, anchor), [view, anchor])
  // A Lista não depende do período: as execuções dos últimos 30 dias dão o estado de cada rotina.
  const [from, to] = mode === 'calendar' ? [range.start, range.end] : [addDays(today, -30), addDays(today, 1)]
  const overview = useOverview(store, from, to)
  const data = overview.data

  // Um bot que não existe mais (apagado) sai do filtro, em vez de deixar o seletor sem nome.
  useEffect(() => {
    if (filters.botId !== ALL && store.bots.size && !store.bots.has(filters.botId)) setFilters((f) => ({ ...f, botId: ALL }))
  }, [signature, filters.botId])

  // Se uma seção for apagada em outro aparelho, o filtro volta para todas.
  useEffect(() => {
    if (filters.sectionId !== ALL && !sectionChoices.some((section) => section.id === filters.sectionId)) {
      setFilters((f) => ({ ...f, sectionId: ALL }))
    }
  }, [sectionSnapshot, filters.sectionId])

  // ⌘F (menu "Search Bots"): com a tela aberta, vai para "Buscar rotinas".
  useEffect(() => window.codync.app.onCommand((c) => {
    if (c.kind !== 'search') return
    search.current?.focus()
    search.current?.select()
  }), [])

  const { agents, bots } = useMemo(() => ({
    agents: agentBots(store),
    bots: new Map<string, Bot>([...store.bots.values()].map((b) => [b.id, b])),
  }), [store, signature])
  const routines = useMemo(() => (data ? filterRoutines(data, filters, bots, sectionSnapshot) : []), [data, filters, bots, sectionSnapshot])
  const items = useMemo(() => (data ? calendarItems(data, routines, store.isOffline ? null : pause.paused) : []), [data, routines, pause.paused, store.isOffline])
  const dense = useMemo(() => new Set(data?.dense), [data])
  const invalid = useMemo(() => new Set(data?.invalid), [data])
  const truncated = useMemo(() => routines.filter((r) => data?.truncated?.includes(r.id)), [data, routines])
  const inSection = (bot: Bot) => filters.sectionId === ALL || !sectionSnapshot || inGroup(sectionSnapshot, bot.id, filters.sectionId)
  const withRoutines = new Set(data?.routines.map((r) => r.botId))
  // O bot do filtro fica sempre entre as opções (mesmo oculto e sem rotinas): o seletor nunca fica sem nome.
  const avisaCriacao = avisaPrimeiraAgendada({ local: app.local, store, visao: overview.error ? null : data, botsDaTela: agents })
  const filterBots = agents.filter((b) => b.id === filters.botId || (inSection(b) && (!b.hidden || withRoutines.has(b.id))))
  const panelBots = store.roster.filter((b) => !isGroup(b) && inSection(b))
  const disabled = store.isOffline

  const create = (at: number | null, botId: string | null) => {
    const only = agents.length === 1 ? agents[0]!.id : null
    setCopyId(null)
    setEditor({ id: crypto.randomUUID(), routine: null, botId: botId ?? only, at })
  }
  const open = (routine: OverviewRoutine) => {
    setCopyId(null)
    setEditor({ id: crypto.randomUUID(), routine, botId: routine.botId, at: null })
  }
  const toggle = async (routine: OverviewRoutine, enabled: boolean) => {
    setBusy(routine.id)
    try {
      await (await store.ready()).call('setRoutineEnabled', { botId: routine.botId, id: routine.id, enabled })
      setActionError(null)
      overview.reload()
    } catch (e) {
      setActionError(rawError(e))
    } finally {
      setBusy(null)
    }
  }
  const run = async (routine: OverviewRoutine) => {
    setBusy(routine.id)
    try {
      await (await store.ready()).call('runRoutine', { botId: routine.botId, id: routine.id })
      setActionError(null)
      overview.reload()
    } catch (e) {
      setActionError(rawError(e))
    } finally {
      setBusy(null)
    }
  }
  const setSection = (sectionId: string) => {
    setFilters((f) => ({ ...f, sectionId, botId: f.botId === ALL || sectionId === ALL || !sectionSnapshot || inGroup(sectionSnapshot, f.botId, sectionId) ? f.botId : ALL }))
  }
  const setQuery = (query: string) => setFilters((f) => ({ ...f, query }))
  const toggleBot = (botId: string) => setFilters((f) => ({ ...f, botId: f.botId === botId ? ALL : botId }))
  const filtered = filters.sectionId !== ALL || filters.botId !== ALL || filters.query.trim() !== ''

  const header = (
    <>
      <div className="rc-search">
        <SearchField
          value={filters.query}
          onChange={setQuery}
          prompt={texts.searchRoutines}
          clearLabel={texts.clearSearch}
          inputRef={search}
          inputProps={{
            'aria-label': texts.searchRoutinesLabel,
            onKeyDown: (e) => {
              if (e.key !== 'Escape' || !filters.query) return
              e.preventDefault()
              e.stopPropagation()
              setQuery('')
            },
          }}
        />
      </div>
      <div className="rc-filters" role="group" aria-label={texts.filters}>
        {sectionSnapshot ? <ChoicePicker
          language={superficieIdioma(idioma)}
          selection={filters.sectionId}
          options={[{ id: ALL, label: t('grupos.filtro.todos') }, ...sectionChoices.map((section) => ({ id: section.id, label: section.id === UNGROUPED ? t('grupos.foraDeGrupo') : section.name }))]}
          onChange={setSection}
        /> : null}
        <ChoicePicker
          language={superficieIdioma(idioma)}
          selection={filters.botId}
          options={[{ id: ALL, label: texts.allBots }, ...filterBots.map((b) => ({ id: b.id, label: b.name, leading: <BotAvatar bot={b} size={24} animated={false} /> }))]}
          onChange={(botId) => setFilters((f) => ({ ...f, botId }))}
        />
      </div>
      <div className="rc-mode" role="group" aria-label={texts.modeLabel}>
        <SegmentedChoice selection={mode} options={[{ id: 'list', label: texts.list }, { id: 'calendar', label: texts.calendar }]} onChange={setMode} />
      </div>
      <Button className="rc-new" disabled={disabled} onClick={() => create(null, filters.botId === ALL ? null : filters.botId)}>
        <Icon name="plus" size={12} weight="semibold" />
        {texts.newRoutine}
      </Button>
    </>
  )

  return (
    <Page header={header}>
      <GlobalPause model={pause} offline={store.isOffline} />
      {actionError ? <p className="rc-action-error" role="alert">{errorText(actionError)}</p> : null}
      <div className={copyId ? 'rc-banner' : 'rc-sr-only'} role="status">{copyId ? t('rotinas.editor.duplicada') : ''}</div>
      {mode === 'calendar' ? (
        <RoutineCalendar
          view={view}
          setView={setView}
          anchor={anchor}
          setAnchor={setAnchor}
          range={range}
          today={today}
          now={now}
          routines={routines}
          items={items}
          dense={dense}
          truncated={truncated}
          bots={bots}
          panelBots={panelBots}
          botFilter={filters.botId === ALL ? null : filters.botId}
          empty={!!data && data.routines.length === 0}
          query={filters.query}
          clearQuery={() => setQuery('')}
          hasData={!!data}
          disabled={disabled}
          loading={overview.loading && !data}
          error={overview.error}
          retry={overview.reload}
          onOpenRoutine={open}
          onCreate={create}
          onToggleBot={toggleBot}
        />
      ) : (
        <div className="rc-content">
          {ehHubDesteComputador(app.local, store) ? <AvisoInstalacaoLocal /> : null}
          {!data ? (
            <LoadState loading={overview.loading} error={overview.error} retry={overview.reload} />
          ) : data.routines.length === 0 ? (
            <div className="rc-empty">
              <CascaVazia fallback={<span className="rc-empty-icon" aria-hidden><Icon name="calendar" size={20} /></span>} />
              <h2>{texts.emptyTitle}</h2>
              <p>{texts.emptyBody}</p>
              <Button disabled={disabled} onClick={() => create(null, null)}>
                <Icon name="plus" size={12} weight="semibold" />
                {texts.newRoutineTitle}
              </Button>
            </div>
          ) : routines.length === 0 ? (
            <div className="rc-empty" role="status">
              {filters.query.trim() ? (
                <>
                  <CascaVazia fallback={<span className="rc-empty-icon" aria-hidden><Icon name="magnifyingglass" size={18} /></span>} />
                  <h2>{texts.noResults}</h2>
                  <p>{texts.noResultsFor(filters.query.trim())}</p>
                  <Button kind="secondary" onClick={() => setQuery('')}>{texts.clearSearch}</Button>
                </>
              ) : (
                <>
                  <p>{texts.filteredEmpty}</p>
                  {filtered ? <Button kind="secondary" onClick={() => setFilters({ sectionId: ALL, botId: ALL, query: '' })}>{texts.clearFilters}</Button> : null}
                </>
              )}
            </div>
          ) : (
            <>
              {overview.error ? <LoadState loading={false} error={overview.error} retry={overview.reload} /> : null}
              <RoutineList globalPause={store.isOffline ? null : pause.paused} routines={routines} runs={data.runs} bots={bots} invalid={invalid} busy={busy} disabled={disabled} onOpen={open} onToggle={(r, on) => void toggle(r, on)} onRun={(r) => void run(r)} copyId={copyId} copyRow={copyRow} />
            </>
          )}
        </div>
      )}
      <RoutineSheet target={editor} setTarget={setEditor} bots={agents} avisaCriacao={avisaCriacao} saved={overview.reload} duplicated={(target, routine) => {
        overview.reload()
        if (currentEditor.current?.id !== target.id) return
        setEditor(null)
        setMode('list')
        setCopyId(routine.id)
      }} />
    </Page>
  )
})

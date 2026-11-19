// MX-142 etapa 1 (docs/reference/redesenho/components/Tarefas/README.md; proposta aprovada tarefas-dos-agentes-20261111 e
// decisões do dono em MX-112): a área **Tarefas** do Mission Board, sobre os Projetos do #620. Lista (padrão) e Quadro
// leem as MESMAS tarefas (todas as dos Projetos ativos do Space); o Projeto vira filtro e etiqueta. A tela desenha o que
// o Hub devolve e envia intenções do dono; atribuir não avisa nem inicia bot. Toda gravação relê o Projeto inteiro e
// usa a revisão dele (CAS por Projeto). Função que o Hub não anuncia não aparece (fail-closed).
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { draftOf, PROJECT_LIMITS, withTask, type ProjectBot, type ProjectSummary, type ProjectTaskState } from '@shared/fork-projetos'
import {
  agendaPending, boardColumns, CLOSED_STATES, declaredBy, entriesOf, isFiltered, isOverdue, lastUpdateOf, listGroups, NO_FILTER, progressOf,
  type TaskAgenda, type TaskEntry, type TaskFilter, type TaskPriority,
} from '@shared/fork-tarefas'
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import { Icon } from '../../../components/Icon'
import { AnchoredMenu, type MenuItem } from '../../../components/Overlay'
import { DropdownMenu, SearchField, SegmentedChoice } from '../../../components/Controls'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import { useIdioma, useT, superficieIdioma, type Tradutor } from '../../../lib/fork-i18n/estado'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import type { TowerSource } from '../torre/torre-session'
import { boardKey } from './board-keys'
import { columnName, failureText, ProjectFormSheet, sessionFor, useClock, WriteAlert } from './ProjetosView'
import type { ProjectsSession } from './projetos-session'
import { TarefaDetalhe } from './TarefaDetalhe'
import { TarefaSheet, type TaskSheetTarget } from './TarefaSheet'
import { rememberViewMode, viewModeFor, type TaskViewMode } from './tarefas-prefs'
import './tarefas.css'

/** Rosto do bot pelo espelho do computador (sem ele, a inicial). */
export type FaceOf = (botId: string, size: number) => ReactNode | null

/** A tela ao vivo: as Tarefas do Space aberto no Mission Board deste computador. */
export function ProjetosLive({ store, spaceId, spaceName, onBack }: { store: BotStore; spaceId: string; spaceName: string; onBack: () => void }) {
  const session = useMemo(() => sessionFor(store), [store])
  useEffect(() => {
    session.open(store as unknown as TowerSource, spaceId)
    return () => session.close()
  }, [session, store, spaceId])
  const face = useCallback<FaceOf>((botId, size) => {
    const bot = store.bots.get(botId)
    return bot ? <DonnabotAvatar config={normalizarAvatarDoBot(avatarDoBot(bot), bot.name)} size={size} animated={false} label={bot.name} /> : null
  }, [store])
  return <TarefasPage session={session} spaceId={spaceId} spaceName={spaceName} computerName={store.computer.name} onBack={onBack} onRetry={() => store.restartStream()} face={face} />
}

export interface TarefasPageProps {
  session: ProjectsSession
  /** O Space pedido; enquanto a sessão não está nele, nada da sessão é pintado. */
  spaceId: string
  spaceName: string
  computerName: string
  onBack: () => void
  onRetry: () => void
  face?: FaceOf
}

/** Hub + Space + fonte remontam a página: nenhum filtro, folha ou rascunho passa de um Space para outro. */
export function TarefasPage(props: TarefasPageProps) {
  useModel(props.session)
  return <TarefasPageIn key={JSON.stringify([sessionKey(props.session), props.session.identity, props.session.hub, props.spaceId])} {...props} />
}
const sessionKeys = new WeakMap<ProjectsSession, number>()
let sessionSeq = 0
function sessionKey(s: ProjectsSession) {
  let k = sessionKeys.get(s)
  if (k === undefined) sessionKeys.set(s, (k = ++sessionSeq))
  return k
}

type Opened = { projectId: string; taskId: string }
/** Chave da linha (Lista) ou do cartão (Quadro) de uma tarefa, em `data-open`, para devolver o foco ao voltar. */
const openKey = (o: Opened) => `${o.projectId}:${o.taskId}`
type ProjectSheet = { mode: 'create' } | { mode: 'edit'; projectId: string }

function TarefasPageIn({ session, spaceId, spaceName, computerName, onBack, onRetry, face }: TarefasPageProps) {
  useModel(session)
  const t = useT()
  const idioma = useIdioma()
  const { time } = useClock()
  const [mode, setMode] = useState<TaskViewMode>(() => viewModeFor(spaceId))
  const [filter, setFilter] = useState<TaskFilter>(NO_FILTER)
  const [opened, setOpened] = useState<Opened | null>(null)
  const [sheet, setSheet] = useState<TaskSheetTarget | null>(null)
  const [projectSheet, setProjectSheet] = useState<ProjectSheet | null>(null)
  const back = useRef<HTMLButtonElement>(null)
  const page = useRef<HTMLDivElement>(null)
  /** A tarefa aberta antes desta renderização e a linha da Lista/Quadro de onde o detalhe foi aberto. */
  const shown = useRef<Opened | null>(null)
  const fromRow = useRef<string | null>(null)
  useEffect(() => {
    session.watchTasks(true)
    return () => session.watchTasks(false)
  }, [session])
  useEffect(() => {
    const was = shown.current
    shown.current = opened
    if (opened || !was) {
      // Abrir (o detalhe já leva o foco ao próprio título ao montar) ou entrar na página: o Voltar só pega o foco se ele
      // ficou fora da página (o botão clicado sumiu e o detalhe ainda está carregando).
      if (!page.current?.contains(document.activeElement)) back.current?.focus({ preventScroll: true })
      return
    }
    // Voltar à Lista/Quadro: a linha da tarefa que estava aberta; senão a de onde o detalhe foi aberto; senão o Voltar.
    const row = [openKey(was), fromRow.current].map((k) => (k ? page.current?.querySelector<HTMLElement>(`[data-open="${CSS.escape(k)}"]`) : null)).find(Boolean)
    ;(row ?? back.current)?.focus()
  }, [opened])

  const here = session.space === spaceId
  const list = here ? session.list : null
  const stale = session.stale && !!list
  const at = here && session.readAt ? time(session.readAt) : null
  const entries = useMemo(() => (here && session.tasks ? entriesOf(session.tasks) : null), [here, session.tasks])
  const bots = list?.bots ?? []
  const active = (list?.projects ?? []).filter((p) => p.state === 'active')

  const open = (o: Opened | null) => {
    setOpened(o)
    session.openProject(o?.projectId ?? null)
    // Comentários, sessões vinculadas e agenda: cada um só é pedido com o anúncio do Hub.
    session.openComments(o)
  }
  const openRow = (e: TaskEntry) => {
    const o = { projectId: e.project.id, taskId: e.task.id }
    fromRow.current = openKey(o)
    open(o)
  }
  const editProject = (projectId: string) => {
    session.openProject(projectId)
    setProjectSheet({ mode: 'edit', projectId })
  }
  const closeProjectSheet = () => {
    setProjectSheet(null)
    // A folha de Projeto abriu o Projeto só para editar; sem tarefa aberta, volta à lista.
    if (!opened) session.openProject(null)
  }
  const changeMode = (m: TaskViewMode) => {
    setMode(m)
    rememberViewMode(spaceId, m)
  }

  let body: ReactNode
  if (!here) body = <p className="torre-state" role="status">{t('projetos.carregando')}</p>
  else if (session.phase.kind === 'moved') body = <p className="torre-state" role="alert">{t('projetos.mudou')}</p>
  else if (session.phase.kind === 'error') body = <p className="torre-state" role="status">{failureText(t, session.phase.reason)}</p>
  else if (!list) body = <p className="torre-state" role="status">{t('projetos.carregando')}</p>
  else if (opened) {
    const snap = session.view.kind === 'ready' && session.project?.project.id === opened.projectId ? session.project : null
    if (session.view.kind === 'error') body = <p className="torre-state" role="status">{failureText(t, session.view.reason)}</p>
    else if (!snap) body = <p className="torre-state" role="status">{t('projetos.carregando')}</p>
    else body = (
      // Uma montagem por tarefa (Projeto + tarefa): rascunho de comentário, menus e abas de uma nunca passam para outra.
      <TarefaDetalhe key={`${opened.projectId}:${opened.taskId}`} session={session} snap={snap} taskId={opened.taskId} face={face}
        onOpen={(taskId) => open({ projectId: opened.projectId, taskId })}
        onEdit={() => setSheet({ kind: 'edit', projectId: opened.projectId, taskId: opened.taskId })}
        onAddChild={() => setSheet({ kind: 'new', projectId: opened.projectId, parentTaskId: opened.taskId })}
        onEditProject={() => editProject(opened.projectId)}
        onBack={() => open(null)} />
    )
  } else if (!entries) body = session.tasksError ? <p className="torre-state" role="status">{failureText(t, session.tasksError)}</p> : <p className="torre-state" role="status">{t('projetos.carregando')}</p>
  else if (!entries.length) body = (
    <section className="pk-empty" aria-labelledby="tf-empty-title">
      <span className="pk-empty-icon" aria-hidden><Icon name="checklist" size={22} /></span>
      <h2 id="tf-empty-title">{t('projetos.tarefas.vazioTitulo', { space: spaceName })}</h2>
      <p>{t('projetos.tarefas.vazioTexto')}</p>
      <button className="torre-button is-primary" disabled={!session.canWrite} onClick={() => setSheet({ kind: 'new' })}>{t('projetos.tarefas.criarPrimeira')}</button>
    </section>
  )
  else if (mode === 'board') body = <TaskBoard session={session} entries={entries} filter={filter} bots={bots} face={face} onOpen={openRow} />
  else body = <TaskList session={session} entries={entries} filter={filter} bots={bots} face={face} onOpen={openRow} />

  const filteredProject = filter.projectId ? active.find((p) => p.id === filter.projectId) ?? null : null
  const filteredBot = filter.botId ? bots.find((b) => b.id === filter.botId) ?? null : null
  const archived = (list?.projects ?? []).filter((p) => p.state === 'archived')
  const atLimit = (list?.projects.length ?? 0) >= PROJECT_LIMITS.projects
  const projectItems = (): MenuItem[] => [
    { title: t('projetos.tarefas.todosProjetos'), selected: !filter.projectId, action: () => setFilter((f) => ({ ...f, projectId: null })) },
    ...active.map((p, i) => ({ title: p.name, selected: p.id === filter.projectId, divider: i === 0, action: () => setFilter((f) => ({ ...f, projectId: p.id })) })),
    { title: t('projetos.novo'), icon: 'plus', divider: true, disabled: !session.canWrite || atLimit, detail: atLimit ? t('projetos.limite', { n: PROJECT_LIMITS.projects }) : null, action: () => setProjectSheet({ mode: 'create' }) },
    ...(filteredProject ? [{ title: t('projetos.tarefas.editarProjeto', { nome: filteredProject.name }), icon: 'pencil', disabled: !session.canWrite, action: () => editProject(filteredProject.id) }] : []),
    // Arquivados não entram na Lista; daqui o dono os abre para reativar.
    ...archived.map((p, i) => ({ title: t('projetos.tarefas.arquivado', { nome: p.name }), icon: 'archivebox', divider: i === 0, disabled: !session.canWrite, action: () => editProject(p.id) })),
  ]
  const botItems = (): MenuItem[] => [
    { title: t('projetos.tarefas.todosBots'), selected: !filter.botId, action: () => setFilter((f) => ({ ...f, botId: null })) },
    ...bots.map((b, i) => ({ title: b.name, selected: b.id === filter.botId, divider: i === 0, action: () => setFilter((f) => ({ ...f, botId: b.id })) })),
  ]
  const language = superficieIdioma(idioma)

  return (
    <div ref={page} className="torre-page pk-page tf-page" role="region" aria-label={t('projetos.regiao', { space: spaceName })} {...language}>
      <header className="torre-header drag tf-header">
        <button ref={back} className="torre-back no-drag" aria-label={opened ? t('projetos.voltarLista') : t('projetos.voltar')} title={opened ? t('projetos.voltarLista') : t('projetos.voltar')}
          onClick={() => (opened ? open(null) : onBack())}>
          <Icon name="arrow.left" size={14} weight="medium" />
        </button>
        <h1 className="torre-title">{t('projetos.titulo')}</h1>
        <span className="torre-chip"><span className="torre-chip-key">{t('projetos.chipSpace')}</span> {spaceName}</span>
        {list && !opened ? (
          <div className="tf-tools no-drag">
            <SearchField value={filter.query} onChange={(query) => setFilter((f) => ({ ...f, query }))} prompt={t('projetos.tarefas.buscar')} clearLabel={t('projetos.tarefas.limparBusca')} inputProps={{ 'aria-label': t('projetos.tarefas.buscar') }} />
            <DropdownMenu className={`tf-filter${filteredProject ? ' is-on' : ''}`} items={projectItems} language={language} restoreFocusOnClose
              label={t('projetos.tarefas.filtroProjeto', { valor: filteredProject?.name ?? t('projetos.tarefas.todosProjetos') })}>
              <span className="truncate">{filteredProject?.name ?? t('projetos.tarefas.todosProjetos')}</span> <Icon name="chevron.down" size={10} />
            </DropdownMenu>
            <DropdownMenu className={`tf-filter${filteredBot ? ' is-on' : ''}`} items={botItems} language={language} restoreFocusOnClose
              label={t('projetos.tarefas.filtroBot', { valor: filteredBot?.name ?? t('projetos.tarefas.todosBots') })}>
              <span className="truncate">{filteredBot?.name ?? t('projetos.tarefas.todosBots')}</span> <Icon name="chevron.down" size={10} />
            </DropdownMenu>
          </div>
        ) : null}
        <div className="torre-header-end no-drag">
          {at ? <span className="torre-updated">{stale ? t('projetos.ultimaAtualizacao', { hora: at }) : t('projetos.atualizado', { hora: at })}</span> : null}
          {list && !opened ? (
            <>
              <div className="tf-mode" role="group" aria-label={t('projetos.tarefas.vista')}>
                <SegmentedChoice<TaskViewMode> selection={mode} onChange={changeMode}
                  options={[{ id: 'list', label: t('projetos.tarefas.lista') }, { id: 'board', label: t('projetos.tarefas.quadro') }]} />
              </div>
              <button className="torre-button is-primary" disabled={!session.canWrite} onClick={() => setSheet({ kind: 'new', projectId: filter.projectId ?? undefined })}>
                <Icon name="plus" size={12} /> {t('projetos.novaTarefa')}
              </button>
            </>
          ) : null}
        </div>
      </header>
      {list && session.phase.kind === 'ready' ? (
        stale ? (
          <div className="torre-banner is-offline" role="status">
            <Icon name="wifi.slash" size={15} />
            <p><b>{session.authority === 'pending' ? t('projetos.autoridadeDuvida', { computador: computerName })
              : session.confirming ? t('projetos.conferindo', { computador: computerName })
              : session.offlineSince ? t('projetos.semConexaoDesde', { computador: computerName, hora: time(session.offlineSince) }) : t('projetos.semConexao', { computador: computerName })}</b> {session.authority === 'pending' || session.confirming ? t('projetos.autoridadeNota') : t('projetos.semConexaoNota')}</p>
            <button className="torre-button" onClick={() => ((session.authority === 'pending' || session.confirming) && session.link === 'live' ? session.reread() : onRetry())}>{t('projetos.tentarLer')}</button>
          </div>
        ) : (
          <div className="torre-banner pk-manual">
            <Icon name="hand.raised" size={15} />
            <p>{mode === 'board' && !opened ? t('projetos.tarefas.faixaQuadro') : t('projetos.manual')}</p>
          </div>
        )
      ) : null}
      {session.rereadFailed && list ? <p className="torre-notice" role="status">{t('projetos.releituraFalhou')}</p> : null}
      {!sheet && !projectSheet && here ? <WriteAlert session={session} /> : null}
      {body}
      <TarefaSheet session={session} target={list ? sheet : null} spaceId={spaceId} spaceName={spaceName} face={face}
        onNewProject={() => { setSheet(null); setProjectSheet({ mode: 'create' }) }}
        onClose={() => setSheet(null)} />
      <ProjectFormSheet open={projectSheet?.mode === 'create' && !!list} mode="create" bots={bots} spaceName={spaceName} session={session} onClose={closeProjectSheet} />
      <ProjectFormSheet open={projectSheet?.mode === 'edit' && session.view.kind === 'ready' && session.project?.project.id === projectSheet.projectId} mode="edit" bots={session.project?.bots ?? bots} spaceName={spaceName} session={session}
        snap={session.project ?? undefined} onClose={closeProjectSheet} />
    </div>
  )
}

// MARK: peças

export function BotFace({ bots, id, face, size = 20 }: { bots: readonly ProjectBot[]; id: string | null; face?: FaceOf; size?: number }) {
  if (!id) return null
  const name = bots.find((b) => b.id === id)?.name
  const real = face?.(id, size)
  if (real) return <span className="tf-face" title={name}>{real}</span>
  return <span className="tf-face is-initial" title={name} aria-hidden>{(name ?? '?').trim().charAt(0).toLocaleUpperCase() || '?'}</span>
}

const PRIORITY_KEY = {
  low: 'projetos.tarefas.prioridade.low',
  normal: 'projetos.tarefas.prioridade.normal',
  high: 'projetos.tarefas.prioridade.high',
  urgent: 'projetos.tarefas.prioridade.urgent',
} as const
export const priorityName = (t: Tradutor, p: TaskPriority) => t(PRIORITY_KEY[p])

/** Barras de prioridade (como no Tasks): só quando há prioridade gravada. */
export function PriorityMark({ priority }: { priority: TaskPriority | null }) {
  const t = useT()
  if (!priority) return <span className="tf-priority is-none" aria-hidden />
  const level = { low: 1, normal: 2, high: 3, urgent: 3 }[priority]
  return (
    <span className={`tf-priority is-${priority}`} role="img" aria-label={priorityName(t, priority)} title={priorityName(t, priority)}>
      {[1, 2, 3].map((n) => <i key={n} className={n <= level ? 'is-on' : ''} />)}
    </span>
  )
}

/** Estado da tarefa; `declared`: Concluída declarada por um bot (decisão 2), não conferida pelo dono. */
export function StateDot({ state, declared = false }: { state: ProjectTaskState; declared?: boolean }) {
  const t = useT()
  return <span className={`tf-state is-${state}`}><i aria-hidden />{declared ? t('projetos.tarefas.declarada') : columnName(t, state)}</span>
}

export function ProjectTag({ project }: { project: ProjectSummary }) {
  return <span className="tf-tag" title={project.name}>{project.name}</span>
}

/** O início agendado (se o Hub tem agenda), senão o prazo, senão quem mudou por último e quando. */
function infoLine(t: Tradutor, e: TaskEntry, bots: readonly ProjectBot[], history: Parameters<typeof lastUpdateOf>[0], when: (ms: number) => string, day: (ms: number) => string, now: number, agenda: TaskAgenda | null): { text: string; tone?: 'late' | 'start' } | null {
  if (agenda && agendaPending(agenda) && !CLOSED_STATES.includes(e.task.state)) return { text: t('projetos.tarefas.comeca', { quando: day(agenda.startAt) }), tone: 'start' }
  if (e.meta.dueAt !== null && !CLOSED_STATES.includes(e.task.state)) {
    return { text: t('projetos.tarefas.prazo', { quando: day(e.meta.dueAt) }), ...(isOverdue(e.meta, e.task.state, now) ? { tone: 'late' as const } : {}) }
  }
  const last = lastUpdateOf(history, e.task.id)
  if (!last) return null
  return { text: t('projetos.tarefas.atualizou', { quem: authorName(t, last.author, bots), quando: when(last.at) }) }
}

export function authorName(t: Tradutor, author: string, bots: readonly ProjectBot[]) {
  if (author === 'owner') return t('projetos.tarefas.voce')
  if (author === 'system') return t('projetos.tarefas.sistema')
  const id = author.startsWith('bot:') ? author.slice(4) : author
  return bots.find((b) => b.id === id)?.name ?? t('projetos.tarefas.botIndisponivel')
}

// MARK: Lista

function TaskList({ session, entries, filter, bots, face, onOpen }: { session: ProjectsSession; entries: TaskEntry[]; filter: TaskFilter; bots: readonly ProjectBot[]; face?: FaceOf; onOpen: (e: TaskEntry) => void }) {
  const t = useT()
  const { when, day } = useClock()
  const [folded, setFolded] = useState<ReadonlySet<string>>(new Set())
  const [showClosed, setShowClosed] = useState(false)
  const { waiting, groups } = useMemo(() => listGroups(entries, filter), [entries, filter])
  const historyOf = useMemo(() => new Map((session.tasks ?? []).map((b) => [b.project.id, b.history])), [session.tasks])
  const declarationsOf = useMemo(() => new Map((session.tasks ?? []).map((b) => [b.project.id, b.declarations])), [session.tasks])
  const tasksOf = useMemo(() => new Map((session.tasks ?? []).map((b) => [b.project.id, b.tasks])), [session.tasks])
  const now = Date.now()
  const filtered = isFiltered(filter)
  const open = groups.filter((g) => !CLOSED_STATES.includes(g.state) && g.rows.length)
  const closed = groups.filter((g) => CLOSED_STATES.includes(g.state))
  const closedCount = closed.reduce((n, g) => n + g.count, 0)
  const toggle = (id: string) => setFolded((f) => { const n = new Set(f); if (n.has(id)) n.delete(id); else n.add(id); return n })

  const rows = (list: TaskEntry[]) => {
    let hiddenUnder: number | null = null
    let rootFolded = false
    return list.map((e) => {
      if (e.depth === 0) { rootFolded = folded.has(e.task.id); hiddenUnder = null }
      if (e.depth > 0 && rootFolded) return null
      if (hiddenUnder !== null && e.depth > hiddenUnder) return null
      hiddenUnder = null
      const tasks = tasksOf.get(e.project.id) ?? []
      const kids = tasks.some((x) => x.parentTaskId === e.task.id)
      const progress = kids ? progressOf(tasks, e.task.id) : null
      const parent = filtered && e.task.parentTaskId ? tasks.find((x) => x.id === e.task.parentTaskId) : null
      const info = infoLine(t, e, bots, historyOf.get(e.project.id) ?? null, when, day, now, session.agendaOf(e.project.id, e.task.id))
      return (
        <li key={`${e.project.id}:${e.task.id}`} className={`tf-row${e.depth === 0 ? ' is-root' : ''}`} style={{ ['--tf-depth' as string]: e.depth }}>
          {kids && !filtered && e.depth === 0 ? (
            <button className="tf-fold" aria-expanded={!folded.has(e.task.id)} aria-label={t('projetos.tarefas.subtarefasDe', { titulo: e.task.title })} onClick={() => toggle(e.task.id)}>
              <Icon name={folded.has(e.task.id) ? 'chevron.right' : 'chevron.down'} size={10} />
            </button>
          ) : <span className="tf-fold" aria-hidden />}
          <button className="tf-row-open" data-open={`${e.project.id}:${e.task.id}`} onClick={() => onOpen(e)}>
            <PriorityMark priority={e.meta.priority} />
            <span className="tf-row-title">
              <b className="truncate">{e.task.title}</b>
              {e.depth === 0 ? <ProjectTag project={e.project} /> : null}
              {parent ? <span className="tf-muted truncate">{t('projetos.parteDe', { titulo: parent.title })}</span> : null}
            </span>
            <span className="tf-row-progress">
              {progress && progress.total ? (
                <>
                  <span className="tf-bar" aria-hidden><i style={{ width: `${Math.round((progress.done / progress.total) * 100)}%` }} /></span>
                  <span className="tf-muted">{t('projetos.tarefas.progresso', { feitas: progress.done, total: progress.total })}</span>
                </>
              ) : null}
            </span>
            <span className="tf-row-bot"><BotFace bots={bots} id={e.task.assigneeBotId} face={face} /></span>
            <StateDot state={e.task.state} declared={!!declaredBy(declarationsOf.get(e.project.id), e.task)} />
            <span className={`tf-row-info truncate${info?.tone ? ` is-${info.tone}` : ''}`}>{info?.text ?? (e.task.assigneeBotId ? '' : t('projetos.semResponsavel'))}</span>
          </button>
        </li>
      )
    })
  }

  return (
    <div className="tf-list">
      {waiting.length ? (
        <section className="tf-group is-waiting" aria-label={t('projetos.tarefas.esperando')}>
          <h2 className="tf-group-title"><Icon name="hand.raised" size={12} /> {t('projetos.tarefas.esperando')} <span>{waiting.length}</span> <em>{t('projetos.tarefas.esperandoNota')}</em></h2>
          <ul className="tf-rows">{rows(waiting)}</ul>
        </section>
      ) : null}
      {open.map((g) => (
        <section key={g.state} className="tf-group" aria-label={columnName(t, g.state)}>
          <h2 className="tf-group-title"><span className={`tf-state is-${g.state}`}><i aria-hidden /></span>{columnName(t, g.state)} <span>{g.count}</span></h2>
          <ul className="tf-rows">{rows(g.rows)}</ul>
        </section>
      ))}
      {!open.length && !waiting.length ? <p className="tf-none">{filtered ? t('projetos.tarefas.nadaFiltro') : t('projetos.tarefas.nadaAberto')}</p> : null}
      {closedCount ? (
        <section className="tf-group is-closed">
          <button className="pk-disclosure" aria-expanded={showClosed} onClick={() => setShowClosed((v) => !v)}>
            <Icon name={showClosed ? 'chevron.down' : 'chevron.right'} size={11} /> {closed.map((g) => `${columnName(t, g.state)} ${g.count}`).join(' · ')}
          </button>
          {showClosed ? closed.filter((g) => g.rows.length).map((g) => <ul key={g.state} className="tf-rows" aria-label={columnName(t, g.state)}>{rows(g.rows)}</ul>) : null}
        </section>
      ) : null}
    </div>
  )
}

// MARK: Quadro

/** Mover de coluna a partir da Lista/Quadro: relê o Projeto da tarefa e grava só o estado dela. */
export function moveTask(session: ProjectsSession, projectId: string, taskId: string, state: ProjectTaskState) {
  return session.saveFresh(projectId, (snap) => {
    const cur = snap.tasks.find((x) => x.id === taskId)
    if (!cur || cur.state === state) return null
    return withTask(draftOf(snap), { ...cur, state })
  })
}

function TaskBoard({ session, entries, filter, bots, face, onOpen }: { session: ProjectsSession; entries: TaskEntry[]; filter: TaskFilter; bots: readonly ProjectBot[]; face?: FaceOf; onOpen: (e: TaskEntry) => void }) {
  const t = useT()
  const idioma = useIdioma()
  const { when, day } = useClock()
  const grid = useRef<HTMLDivElement>(null)
  const anchorRef = useRef<HTMLElement | null>(null)
  const [menu, setMenu] = useState<TaskEntry | null>(null)
  const [blocked, setBlocked] = useState<{ n: number } | null>(null)
  const columns = useMemo(() => boardColumns(entries, filter), [entries, filter])
  const byId = useMemo(() => new Map(entries.map((e) => [e.task.id, e])), [entries])
  const tasksOf = useMemo(() => new Map((session.tasks ?? []).map((b) => [b.project.id, b])), [session.tasks])
  const canMove = session.canWrite
  const now = Date.now()
  const move = (e: TaskEntry, state: ProjectTaskState) => {
    if (!session.canWrite || e.task.state === state) return
    void moveTask(session, e.project.id, e.task.id, state).then(() => {
      requestAnimationFrame(() => grid.current?.querySelector<HTMLElement>(`[data-task="${e.task.id}"]`)?.focus())
    })
  }
  const onKey = (ev: KeyboardEvent<HTMLDivElement>) => boardKey(ev, {
    direction: getComputedStyle(ev.currentTarget).direction === 'rtl' ? 'rtl' : 'ltr',
    cards: () => [...(grid.current?.querySelectorAll<HTMLElement>('[data-task]') ?? [])],
    stateOf: (id) => byId.get(id)?.task.state,
    canMove,
    move: (id, to) => { const e = byId.get(id); if (e) move(e, to) },
    blocked: () => setBlocked((b) => ({ n: (b?.n ?? 0) + 1 })),
  })
  return (
    <div className="tf-board">
      <p className="pk-hint" id="tf-keys"><Icon name="info.circle" size={12} /> {t('projetos.tecladoDica')}</p>
      {blocked ? <p key={blocked.n} className="pk-blocked" role="status">{t('projetos.bloqueado.mover')}</p> : null}
      <div ref={grid} className="pk-columns tf-columns" role="group" aria-label={t('projetos.tarefas.quadroAria')} aria-describedby="tf-keys" onKeyDown={onKey}>
        {columns.map((c, col) => {
          const name = columnName(t, c.state)
          return (
            <section key={c.state} className={`pk-column is-${c.state}`} aria-label={t.plural('projetos.colunaAria', c.cards.length, { coluna: name })}>
              <h2 className="pk-column-title"><i aria-hidden />{name}<span>{c.cards.length}</span></h2>
              {c.cards.length ? (
                <ul className="pk-cards">
                  {c.cards.map((e, row) => {
                    const b = tasksOf.get(e.project.id)
                    const parent = e.task.parentTaskId ? b?.tasks.find((x) => x.id === e.task.parentTaskId) : null
                    const info = infoLine(t, e, bots, b?.history ?? null, when, day, now, session.agendaOf(e.project.id, e.task.id))
                    const assignee = e.task.assigneeBotId ? bots.find((x) => x.id === e.task.assigneeBotId)?.name ?? t('projetos.responsavelIndisponivel') : t('projetos.semResponsavel')
                    return (
                      <li key={`${e.project.id}:${e.task.id}`} className="pk-card">
                        <button className="pk-card-open" data-task={e.task.id} data-open={`${e.project.id}:${e.task.id}`} data-col={col} data-row={row} onClick={() => onOpen(e)}>
                          <span className="tf-card-head"><b className="pk-card-title">{e.task.title}</b><ProjectTag project={e.project} /></span>
                          <span className="pk-card-line"><BotFace bots={bots} id={e.task.assigneeBotId} face={face} size={16} /> {assignee}{parent ? <span className="pk-muted"> · {t('projetos.parteDe', { titulo: parent.title })}</span> : null}</span>
                          {info || e.meta.priority ? (
                            <span className={`pk-card-line tf-card-info${info?.tone ? ` is-${info.tone}` : ''}`}><PriorityMark priority={e.meta.priority} />{info?.text ?? ''}</span>
                          ) : null}
                        </button>
                        <button className="pk-card-move" aria-label={t('projetos.moverAria', { titulo: e.task.title })} aria-haspopup="menu" disabled={!canMove} title={t('projetos.mover')}
                          onClick={(ev) => { anchorRef.current = ev.currentTarget; setMenu(e) }}>
                          <Icon name="ellipsis" size={13} />
                        </button>
                      </li>
                    )
                  })}
                </ul>
              ) : <p className="pk-column-empty">{t('projetos.colunaVazia')}</p>}
            </section>
          )
        })}
      </div>
      <AnchoredMenu open={!!menu} onClose={() => setMenu(null)} anchor={anchorRef} language={superficieIdioma(idioma)}
        items={() => menu ? [
          ...columns.map((c) => ({ title: columnName(t, c.state), selected: c.state === menu.task.state, disabled: !canMove, action: () => move(menu, c.state) })),
          { title: t('projetos.abrirTarefa'), divider: true, icon: 'square.and.pencil', action: () => onOpen(menu) },
        ] : []} />
    </div>
  )
}

// MX-142 etapa 1: o detalhe de uma tarefa (vindo da Lista ou do Quadro). Cabeçalho com estado, prioridade, responsável,
// coordenador e prazo; fases (só exibição); subtarefas; Atividade (histórico do Hub + comentários do dono) e o campo de
// comentário. Decisão do dono (MX-112, 10): comentário sem @ só anota e nenhum bot o vê; com @, o Hub confere quem pode
// ser avisado e devolve o status — a tela nunca deduz que avisou. Sem `taskComments`, não há campo (fail-closed).
import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from 'react'
import { depthOf, PROJECT_LIMITS, PROJECT_TASK_STATES, type ProjectBot, type ProjectHistoryItem, type ProjectSnapshot } from '@shared/fork-projetos'
import {
  agendaPending, CLOSED_STATES, declaredBy, EMPTY_META, isOverdue, joinNames, mentionQuery, mentionsIn, pendingDraft, progressOf, TASK_LIMITS,
  type AgendaState, type CommentNotify, type LinkedSessionState, type RecipientState, type TaskAgenda, type TaskComment,
} from '@shared/fork-tarefas'
import { isolado } from '@shared/bidi'
import type { AppLanguage } from '@shared/fork-idioma'
import { Icon } from '../../../components/Icon'
import { AnchoredMenu } from '../../../components/Overlay'
import { Switch } from '../../../components/Controls'
import { useIdioma, useT, superficieIdioma, type Tradutor } from '../../../lib/fork-i18n/estado'
import { localeDe } from '../../../lib/fork-i18n/index'
import { columnName, failureText, useClock } from './ProjetosView'
import type { ProjectsSession } from './projetos-session'
import { authorName, BotFace, moveTask, PriorityMark, priorityName, StateDot, type FaceOf } from './TarefasView'

export interface TarefaDetalheProps {
  session: ProjectsSession
  snap: ProjectSnapshot
  taskId: string
  face?: FaceOf
  onOpen: (taskId: string) => void
  onEdit: () => void
  onAddChild: () => void
  onEditProject: () => void
  onBack: () => void
}

export function TarefaDetalhe({ session, snap, taskId, face, onOpen, onEdit, onAddChild, onEditProject, onBack }: TarefaDetalheProps) {
  const t = useT()
  const idioma = useIdioma()
  const { day } = useClock()
  const [menu, setMenu] = useState<{ taskId: string } | null>(null)
  const anchorRef = useRef<HTMLElement | null>(null)
  const title = useRef<HTMLHeadingElement>(null)
  // Uma montagem por tarefa (key na página): abrir da Lista, do pai ou de uma subtarefa leva o foco ao título — o botão
  // clicado some com a troca e o foco não pode cair no body.
  useEffect(() => { title.current?.focus() }, [])
  const task = snap.tasks.find((x) => x.id === taskId)
  if (!task) {
    return (
      <section className="pk-empty">
        <h2 ref={title} tabIndex={-1}>{t('projetos.tarefas.sumiu')}</h2>
        <button className="torre-button" onClick={onBack}>{t('projetos.voltarLista')}</button>
      </section>
    )
  }
  const meta = session.metaOf(snap.project.id, task.id) ?? EMPTY_META
  const bots = snap.bots
  const parent = task.parentTaskId ? snap.tasks.find((x) => x.id === task.parentTaskId) : null
  const kids = snap.tasks.filter((x) => x.parentTaskId === task.id)
  const progress = progressOf(snap.tasks, task.id)
  const archived = snap.project.state !== 'active'
  const canWrite = session.canWrite && !archived
  const canAddChild = canWrite && depthOf(snap.tasks, task.id) < PROJECT_LIMITS.depth && snap.tasks.length < PROJECT_LIMITS.tasks
  const coordinator = snap.project.coordinatorBotId
  const declared = declaredBy(snap.declarations, task)
  const agenda = session.agendaOf(snap.project.id, task.id)
  // Iniciar (decisão 5): só com `taskStart`, com responsável e tarefa aberta. Atribuir sozinho nunca dispara.
  const canStart = session.has('taskStart') && !!task.assigneeBotId && !CLOSED_STATES.includes(task.state)
  const moveMenu = (id: string) => {
    const cur = snap.tasks.find((x) => x.id === id)
    return PROJECT_TASK_STATES.map((s) => ({ title: columnName(t, s), selected: s === cur?.state, disabled: !canWrite, action: () => void moveTask(session, snap.project.id, id, s) }))
  }

  return (
    <div className="tf-detail">
      <div className="tf-detail-main">
        <nav className="tf-crumbs" aria-label={t('projetos.tarefas.caminho')}>
          <button className="tf-tag" onClick={onEditProject} disabled={!session.canWrite} title={t('projetos.editar')}>{snap.project.name}</button>
          {parent ? <><Icon name="chevron.right" size={9} /><button className="tf-link" onClick={() => onOpen(parent.id)}>{parent.title}</button></> : null}
        </nav>
        <h2 ref={title} tabIndex={-1} className="tf-detail-title">{task.title}</h2>
        <div className="tf-facts">
          <button className="tf-fact tf-fact-button" aria-haspopup="menu" disabled={!canWrite} title={t('projetos.mover')}
            onClick={(e) => { anchorRef.current = e.currentTarget; setMenu({ taskId: task.id }) }}>
            <StateDot state={task.state} declared={!!declared} /> <Icon name="chevron.down" size={9} />
          </button>
          {declared ? <span className="tf-fact tf-muted">{t('projetos.tarefas.declaradaPor', { bot: bots.find((b) => b.id === declared)?.name ?? t('projetos.tarefas.botIndisponivel') })}</span> : null}
          {meta.priority ? <span className="tf-fact"><PriorityMark priority={meta.priority} /> {priorityName(t, meta.priority)}</span> : null}
          <span className="tf-fact"><span className="tf-muted">{t('projetos.tarefa.responsavel')}</span> <BotFace bots={bots} id={task.assigneeBotId} face={face} size={16} /> <b>{task.assigneeBotId ? bots.find((b) => b.id === task.assigneeBotId)?.name ?? t('projetos.responsavelIndisponivel') : t('projetos.semResponsavel')}</b></span>
          {coordinator ? <span className="tf-fact"><span className="tf-muted">{t('projetos.tarefas.coordena')}</span> <BotFace bots={bots} id={coordinator} face={face} size={16} /> <b>{bots.find((b) => b.id === coordinator)?.name ?? t('projetos.coordenadorIndisponivel')}</b></span> : null}
          {meta.dueAt !== null ? <span className={`tf-fact${isOverdue(meta, task.state, Date.now()) ? ' is-late' : ''}`}><Icon name="calendar" size={12} /> {t('projetos.tarefas.prazo', { quando: day(meta.dueAt) })}</span> : null}
          {agenda ? <AgendaFact agenda={agenda} /> : null}
          <span className="tf-spacer" />
          {canStart ? (
            <button className="torre-button" disabled={!canWrite} title={t('projetos.tarefas.iniciarNota')} onClick={() => void session.startTask(snap.project.id, task.id)}>
              <Icon name="play.circle" size={12} /> {t('projetos.tarefas.iniciar')}
            </button>
          ) : null}
          <button className="torre-button" disabled={!canWrite} onClick={onEdit}><Icon name="pencil" size={12} /> {t('projetos.tarefas.editarTarefa')}</button>
        </div>
        {archived ? <p className="pk-blocked" role="status">{t('projetos.bloqueado.arquivado')}</p> : null}
        {task.description ? <p className="tf-text">{task.description}</p> : null}
        {task.acceptance ? <p className="tf-text"><span className="tf-muted">{t('projetos.tarefa.aceite')}:</span> {task.acceptance}</p> : null}
        {task.evidence ? <p className="tf-text"><span className="tf-muted">{t('projetos.tarefa.evidencia')}:</span> {task.evidence}</p> : null}
        {meta.phases.length ? <Phases phases={meta.phases} current={meta.currentPhase} /> : null}

        <section className="tf-card" aria-labelledby="tf-kids">
          <header className="tf-card-header">
            <h3 id="tf-kids">{t('projetos.tarefas.subtarefas')}</h3>
            {progress.total ? <span className="tf-muted">{t('projetos.tarefas.concluidasDe', { feitas: progress.done, total: progress.total })}</span> : null}
            <span className="tf-spacer" />
            <button className="torre-button" disabled={!canAddChild} title={!canAddChild && canWrite ? t('projetos.problema.badParent') : undefined} onClick={onAddChild}><Icon name="plus" size={12} /> {t('projetos.tarefas.novaSubtarefa')}</button>
          </header>
          {kids.length ? (
            <ul className="tf-kids">
              {kids.map((k) => (
                <li key={k.id}>
                  <BotFace bots={bots} id={k.assigneeBotId} face={face} />
                  <button className="tf-link tf-kid-title" onClick={() => onOpen(k.id)}>{k.title}</button>
                  <span className="tf-muted truncate">{k.assigneeBotId ? bots.find((b) => b.id === k.assigneeBotId)?.name ?? t('projetos.responsavelIndisponivel') : t('projetos.semResponsavel')}</span>
                  <span className="tf-spacer" />
                  <StateDot state={k.state} declared={!!declaredBy(snap.declarations, k)} />
                  <button className="pk-card-move tf-kid-move" aria-label={t('projetos.moverAria', { titulo: k.title })} aria-haspopup="menu" disabled={!canWrite}
                    onClick={(e) => { anchorRef.current = e.currentTarget; setMenu({ taskId: k.id }) }}>
                    <Icon name="ellipsis" size={13} />
                  </button>
                </li>
              ))}
            </ul>
          ) : <p className="tf-muted tf-card-empty">{t('projetos.tarefas.semSubtarefas')}</p>}
        </section>

        <Activity session={session} snap={snap} taskId={task.id} face={face} />
      </div>
      <aside className="tf-detail-side" aria-label={t('projetos.tarefas.regrasTitulo')}>
        {session.has('taskSessions') ? <LinkedSessions session={session} bots={bots} face={face} /> : null}
        <h3><Icon name="checkmark.shield" size={13} /> {t('projetos.tarefas.regrasTitulo')}</h3>
        <ul className="tf-rules">
          <li><b>{t('projetos.tarefas.voce')}</b>: {t('projetos.tarefas.regraVoce')}</li>
          <li>{t('projetos.tarefas.regraAtribuir')}</li>
          <li>{t('projetos.tarefas.regraComentario')}</li>
          <li>{t('projetos.tarefas.regraBotDono')}</li>
          <li>{t('projetos.tarefas.regraBotsOutros')}</li>
          <li>{t('projetos.tarefas.regraBotsCriam')}</li>
        </ul>
      </aside>
      <AnchoredMenu open={!!menu} onClose={() => setMenu(null)} anchor={anchorRef} language={superficieIdioma(idioma)} items={() => (menu ? moveMenu(menu.taskId) : [])} />
    </div>
  )
}

const AGENDA_KEY: Record<AgendaState, 'projetos.tarefas.agenda.scheduled' | 'projetos.tarefas.agenda.started' | 'projetos.tarefas.agenda.refused' | 'projetos.tarefas.agenda.cancelled' | 'projetos.tarefas.agenda.failed' | 'projetos.tarefas.agenda.unknown'> = {
  scheduled: 'projetos.tarefas.agenda.scheduled',
  starting: 'projetos.tarefas.agenda.scheduled',
  started: 'projetos.tarefas.agenda.started',
  refused: 'projetos.tarefas.agenda.refused',
  cancelled: 'projetos.tarefas.agenda.cancelled',
  failed: 'projetos.tarefas.agenda.failed',
  unknown: 'projetos.tarefas.agenda.unknown',
}

/** O início agendado, como o Hub diz (agendado, começou, recusado, falhou com novas tentativas…). */
function AgendaFact({ agenda }: { agenda: TaskAgenda }) {
  const t = useT()
  const { day } = useClock()
  const pending = agendaPending(agenda)
  return (
    <span className={`tf-fact${pending ? ' is-start' : agenda.state === 'refused' || agenda.state === 'failed' ? ' is-late' : ''}`} title={agenda.lastError ?? undefined}>
      <Icon name="clock" size={12} /> {t(AGENDA_KEY[agenda.state], { quando: day(agenda.startAt) })}
    </span>
  )
}

const SESSION_KEY: Record<LinkedSessionState, 'projetos.tarefas.sessao.idle' | 'projetos.tarefas.sessao.working' | 'projetos.tarefas.sessao.waiting_owner' | 'projetos.tarefas.sessao.done' | 'projetos.tarefas.sessao.not_opened' | 'projetos.tarefas.sessao.unknown'> = {
  idle: 'projetos.tarefas.sessao.idle',
  working: 'projetos.tarefas.sessao.working',
  waiting_owner: 'projetos.tarefas.sessao.waiting_owner',
  done: 'projetos.tarefas.sessao.done',
  not_opened: 'projetos.tarefas.sessao.not_opened',
  unknown: 'projetos.tarefas.sessao.unknown',
}

/** Sessões vinculadas da tarefa (`taskSessions`): só leitura nesta etapa; o estado é o que o Hub mediu. */
function LinkedSessions({ session, bots, face }: { session: ProjectsSession; bots: readonly ProjectBot[]; face?: FaceOf }) {
  const t = useT()
  const l = session.linked
  return (
    <section aria-labelledby="tf-sessions-title">
      <h3 id="tf-sessions-title"><Icon name="bubble.left.and.bubble.right" size={13} /> {t('projetos.tarefas.sessoes')} {l?.state === 'ready' ? <span className="tf-muted">{l.sessions.length}</span> : null}</h3>
      {!l || l.state === 'loading' ? <p className="tf-muted">{t('projetos.carregando')}</p>
        : l.state === 'error' ? <p className="tf-muted">{l.reason ? failureText(t, l.reason) : t('projetos.erro.failed')}</p>
          : l.sessions.length ? (
            <ul className="tf-sessions">
              {l.sessions.map((x, i) => (
                <li key={`${x.botId}:${x.sessionId ?? i}`} className={x.parentSessionId ? 'is-child' : ''}>
                  <BotFace bots={bots} id={x.botId} face={face} />
                  <span className="tf-session-text">
                    <b>{sessionTitle(t, bots.find((b) => b.id === x.botId)?.name ?? t('projetos.tarefas.botIndisponivel'), x.title)}</b>
                    <span className="tf-muted">{t(SESSION_KEY[x.state])}</span>
                  </span>
                </li>
              ))}
            </ul>
          ) : <p className="tf-muted">{t('projetos.tarefas.semSessoes')}</p>}
    </section>
  )
}

/** Nome do bot e, se houver, o título da sessão: aspas e separador vêm do catálogo; o título entra isolado (bidi). */
function sessionTitle(t: Tradutor, name: string, title: string | null | undefined) {
  return title ? t('projetos.tarefas.sessao.titulo', { nome: name, titulo: title }) : name
}

const nameList = (idioma: AppLanguage, names: readonly string[]) => joinNames(localeDe(idioma), names)

function Phases({ phases, current }: { phases: readonly string[]; current: number | null }) {
  const t = useT()
  return (
    <div className="tf-phases" role="list" aria-label={t('projetos.tarefas.fases')}>
      <span className="tf-phases-label"><Icon name="checklist" size={12} /> {t('projetos.tarefas.fases')}</span>
      {phases.map((p, i) => {
        const state = current === null ? 'next' : i < current ? 'done' : i === current ? 'now' : 'next'
        return (
          <span key={i} role="listitem" className={`tf-phase is-${state}`} aria-current={state === 'now' ? 'step' : undefined}>
            {state === 'done' ? <Icon name="checkmark" size={10} /> : null}{p}
          </span>
        )
      })}
      {current !== null ? <span className="tf-muted">{t('projetos.tarefas.faseDe', { n: current + 1, total: phases.length })}</span> : null}
    </div>
  )
}

// MARK: Atividade

type ActivityItem = { kind: 'history'; at: number; item: ProjectHistoryItem } | { kind: 'comment'; at: number; item: TaskComment }

function Activity({ session, snap, taskId, face }: { session: ProjectsSession; snap: ProjectSnapshot; taskId: string; face?: FaceOf }) {
  const t = useT()
  const { when } = useClock()
  const [tab, setTab] = useState<ActivityTab>('all')
  const hasComments = session.has('taskComments')
  const comments = hasComments && session.comments?.taskId === taskId && session.comments.projectId === snap.project.id ? session.comments : null
  const items = useMemo<ActivityItem[]>(() => {
    const out: ActivityItem[] = []
    if (tab === 'all') for (const h of snap.history) if (h.taskId === taskId) out.push({ kind: 'history', at: h.at, item: h })
    for (const c of comments?.list ?? []) out.push({ kind: 'comment', at: c.at, item: c })
    return out.sort((a, b) => a.at - b.at)
  }, [snap.history, taskId, comments?.list, tab])
  const bots = snap.bots
  return (
    <section className="tf-activity" aria-labelledby="tf-activity-title">
      <header className="tf-card-header">
        <h3 id="tf-activity-title">{t('projetos.tarefas.atividade')}</h3>
        <span className="tf-spacer" />
        {hasComments ? (
          <ActivityTabs tab={tab} onChange={setTab} />
        ) : null}
      </header>
      <div {...(hasComments ? { role: 'tabpanel', id: 'tf-activity-panel', 'aria-labelledby': `tf-activity-tab-${tab}`, tabIndex: -1 } : {})}>
        {comments?.state === 'error' && comments.reason ? <p className="tf-muted" role="status">{t('projetos.tarefas.comentariosFalha', { motivo: failureText(t, comments.reason) })}</p> : null}
        {comments?.omitted ? <p className="tf-muted">{t.plural('projetos.tarefas.comentariosOmitidos', comments.omitted)}</p> : null}
        {items.length ? (
          <ol className="tf-feed">
            {items.map((a) => a.kind === 'history' ? (
              <li key={`h${a.item.id}`} className="tf-event">
                <span className="tf-event-who">{authorName(t, a.item.author, bots)}</span>
                {/* Texto do Hub como texto: sem HTML, sem link, sem abrir nada. */}
                <span className="tf-event-text">{a.item.summary}</span>
                <time dateTime={new Date(a.at).toISOString()}>{when(a.at)}</time>
              </li>
            ) : (
              <li key={`c${a.item.id}`} className="tf-comment">
                <span className="tf-comment-face">{a.item.author.kind === 'bot' ? <BotFace bots={bots} id={a.item.author.botId} face={face} size={24} /> : <span className="tf-face is-initial is-owner" aria-hidden><Icon name="person.fill" size={11} /></span>}</span>
                <div className="tf-comment-body">
                  <p className="tf-comment-head"><b>{a.item.author.kind === 'bot' ? authorName(t, `bot:${a.item.author.botId}`, bots) : authorName(t, a.item.author.kind, bots)}</b> <time dateTime={new Date(a.at).toISOString()}>{when(a.at)}</time></p>
                  <p className="tf-comment-text"><CommentText text={a.item.text} bots={bots} /></p>
                  {a.item.author.kind === 'owner' ? <NotifyLine comment={a.item} bots={bots} /> : null}
                </div>
              </li>
            ))}
          </ol>
        ) : <p className="tf-muted tf-card-empty">{comments?.state === 'loading' ? t('projetos.carregando') : t('projetos.historicoVazio')}</p>}
      </div>
      {hasComments ? <Composer session={session} target={{ projectId: snap.project.id, taskId }} bots={bots} disabled={!comments || comments.state === 'loading' || snap.project.state !== 'active'} /> : <p className="tf-muted tf-note"><Icon name="info.circle" size={12} /> {t('projetos.tarefas.semComentarios')}</p>}
    </section>
  )
}

type ActivityTab = 'all' | 'comments'
const ACTIVITY_TABS: readonly ActivityTab[] = ['all', 'comments']

/**
 * Abas Tudo | Comentários no padrão completo: nome do grupo, cada aba controla o painel da atividade, uma parada de Tab
 * (a escolhida) e ←/→ (na direção visual, também em RTL), Home/End movem e escolhem.
 */
function ActivityTabs({ tab, onChange }: { tab: ActivityTab; onChange: (tab: ActivityTab) => void }) {
  const t = useT()
  const list = useRef<HTMLDivElement>(null)
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const rtl = !!list.current && getComputedStyle(list.current).direction === 'rtl'
    const step = e.key === 'ArrowRight' ? (rtl ? -1 : 1) : e.key === 'ArrowLeft' ? (rtl ? 1 : -1) : 0
    const i = ACTIVITY_TABS.indexOf(tab)
    const next = step ? ACTIVITY_TABS[(i + step + ACTIVITY_TABS.length) % ACTIVITY_TABS.length]
      : e.key === 'Home' ? ACTIVITY_TABS[0] : e.key === 'End' ? ACTIVITY_TABS[ACTIVITY_TABS.length - 1] : undefined
    if (!next) return
    e.preventDefault()
    onChange(next)
    list.current?.querySelector<HTMLElement>(`#tf-activity-tab-${next}`)?.focus()
  }
  return (
    <div ref={list} className="tf-tabs" role="tablist" aria-label={t('projetos.tarefas.atividadeMostrar')} onKeyDown={onKey}>
      {ACTIVITY_TABS.map((id) => (
        <button key={id} id={`tf-activity-tab-${id}`} role="tab" aria-selected={tab === id} aria-controls="tf-activity-panel" tabIndex={tab === id ? 0 : -1}
          className={tab === id ? 'is-on' : ''} onClick={() => onChange(id)}>
          {id === 'all' ? t('projetos.tarefas.tudo') : t('projetos.tarefas.comentarios')}
        </button>
      ))}
    </div>
  )
}

/** O texto do comentário, com as @ de bots do Space destacadas (só aparência; quem é avisado é o Hub que diz). */
function CommentText({ text, bots }: { text: string; bots: readonly ProjectBot[] }) {
  const names = [...bots].map((b) => b.name).filter(Boolean).sort((a, b) => b.length - a.length)
  if (!names.length) return <>{text}</>
  const esc = names.map((n) => n.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
  const parts = text.split(new RegExp(`(@(?:${esc.join('|')}))(?![\\p{L}\\p{N}_])`, 'giu'))
  return <>{parts.map((p, i) => (i % 2 ? <span key={i} className="tf-mention">{p}</span> : p))}</>
}

const RECIPIENT_KEY: Record<RecipientState, 'projetos.tarefas.aviso.r.avisando' | 'projetos.tarefas.aviso.r.avisado' | 'projetos.tarefas.aviso.r.respondeu' | 'projetos.tarefas.aviso.r.naoAvisado' | 'projetos.tarefas.aviso.r.incerto'> = {
  queued: 'projetos.tarefas.aviso.r.avisando',
  sending: 'projetos.tarefas.aviso.r.avisando',
  sent: 'projetos.tarefas.aviso.r.avisado',
  answered: 'projetos.tarefas.aviso.r.respondeu',
  failed: 'projetos.tarefas.aviso.r.naoAvisado',
  not_notified: 'projetos.tarefas.aviso.r.naoAvisado',
  unknown: 'projetos.tarefas.aviso.r.incerto',
}

function reasonText(t: Tradutor, r: string | null): string | null {
  return r === 'no_mention' ? t('projetos.tarefas.aviso.motivo.semMencao')
    : r === 'only_note' ? t('projetos.tarefas.aviso.motivo.soAnotar')
      : r === 'delivery_unavailable' ? t('projetos.tarefas.aviso.motivo.entregaIndisponivel')
        : r ? isolado(r) : null
}

function notifyText(t: Tradutor, idioma: AppLanguage, c: TaskComment, n: CommentNotify | null, bots: readonly ProjectBot[]): { text: string; tone?: 'live' | 'warn' } {
  const nameOf = (id: string) => bots.find((b) => b.id === id)?.name ?? t('projetos.tarefas.botIndisponivel')
  const names = nameList(idioma, (n?.botIds.length ? n.botIds : c.mentions).map(nameOf))
  if (!n) return c.mentions.length && !c.onlyNote ? { text: t('projetos.tarefas.aviso.semStatus'), tone: 'warn' } : { text: t('projetos.tarefas.aviso.soAnotado') }
  // Por destinatário (TEC-722-15): cada @ com o que o Hub diz dele; o resumo nunca vira “ninguém” se alguém recebeu.
  if (n.recipients.length && (n.state === 'partial' || n.recipients.length > 1)) {
    const parts = n.recipients.map((r) => {
      const why = r.state === 'failed' || r.state === 'not_notified' ? reasonText(t, r.reason) : null
      const line = t(RECIPIENT_KEY[r.state], { nome: nameOf(r.botId) })
      return why ? t('projetos.tarefas.aviso.r.comMotivo', { linha: line, motivo: why }) : line
    })
    const warn = n.recipients.some((r) => r.state === 'failed' || r.state === 'not_notified' || r.state === 'unknown')
    const live = n.recipients.some((r) => r.state === 'queued' || r.state === 'sending')
    return { text: parts.join(' · '), ...(warn ? { tone: 'warn' as const } : live ? { tone: 'live' as const } : {}) }
  }
  switch (n.state) {
    case 'partial': return { text: t('projetos.tarefas.aviso.parcial'), tone: 'warn' }
    case 'notifying': return { text: t('projetos.tarefas.aviso.avisando', { nomes: names }), tone: 'live' }
    case 'responding': return { text: t('projetos.tarefas.aviso.respondendo', { nomes: names }), tone: 'live' }
    case 'responded': return { text: t('projetos.tarefas.aviso.respondeu', { nomes: names }) }
    case 'uncertain': return { text: t('projetos.tarefas.aviso.incerto'), tone: 'warn' }
    case 'not_notified': {
      const why = reasonText(t, n.reason)
      return { text: why ? t('projetos.tarefas.aviso.ninguemMotivo', { motivo: why }) : t('projetos.tarefas.aviso.ninguem') }
    }
  }
}

function NotifyLine({ comment, bots }: { comment: TaskComment; bots: readonly ProjectBot[] }) {
  const t = useT()
  const idioma = useIdioma()
  const { text, tone } = notifyText(t, idioma, comment, comment.notify, bots)
  return <p className={`tf-notify${tone ? ` is-${tone}` : ''}`}><Icon name={tone === 'warn' ? 'exclamationmark.triangle' : 'bell.badge'} size={11} /> {text}</p>
}

// MARK: campo de comentário

/** `target`: a tarefa em que o rascunho nasceu; o envio leva essa identidade e a sessão recusa se a aberta for outra. */
function Composer({ session, target, bots, disabled }: { session: ProjectsSession; target: { projectId: string; taskId: string }; bots: readonly ProjectBot[]; disabled: boolean }) {
  const t = useT()
  const idioma = useIdioma()
  const [text, setText] = useState('')
  const [onlyNote, setOnlyNote] = useState(false)
  const [caret, setCaret] = useState(0)
  const [pick, setPick] = useState(0)
  // Envio deste rascunho que ficou incerto: o texto só sai do campo quando o Hub confirmar ESSE pedido (pelo Tentar de
  // novo de qualquer faixa) e se o dono não escreveu outra coisa nesse meio-tempo.
  const [pending, setPending] = useState<{ id: string; text: string } | null>(null)
  const area = useRef<HTMLTextAreaElement>(null)
  const query = mentionQuery(text, caret)
  const options = query ? bots.filter((b) => b.name.toLowerCase().startsWith(query.query.toLowerCase())).slice(0, 8) : []
  const mentions = mentionsIn(text, bots)
  const sending = session.write?.op === 'comment' && session.write.status === 'sending'
  const blocked = disabled || !session.canWrite
  useEffect(() => {
    if (!pending) return
    const w = session.write
    const open = w?.op === 'comment' && w.clientRequestId === pending.id && (w.status === 'uncertain' || w.status === 'sending')
    const next = pendingDraft(pending.text, text, session.commentConfirmed(pending.id), open)
    if (next === 'wait') return
    setPending(null)
    if (next === 'clear') { setText(''); setOnlyNote(false); setCaret(0) }
  })
  const insert = (b: ProjectBot) => {
    if (!query || blocked || sending) return
    const next = `${text.slice(0, query.start)}@${b.name} ${text.slice(caret)}`
    const at = query.start + b.name.length + 2
    setText(next)
    setPick(0)
    requestAnimationFrame(() => { area.current?.focus(); area.current?.setSelectionRange(at, at); setCaret(at) })
  }
  const submit = async () => {
    const body = text.trim()
    if (!body || blocked || sending || body.length > TASK_LIMITS.comment) return
    // “Só anotar” só vale com @: sem menção o comentário já não avisa ninguém.
    const r = await session.sendComment(body, mentions, onlyNote && mentions.length > 0, target)
    if (r.ok) { setText(''); setOnlyNote(false); setCaret(0) }
    else if (r.status === 'uncertain' && r.requestId) setPending({ id: r.requestId, text: body })
  }
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (options.length) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); setPick((p) => (p + (e.key === 'ArrowDown' ? 1 : options.length - 1)) % options.length); return }
      if (e.key === 'Enter' || e.key === 'Tab') { e.preventDefault(); insert(options[Math.min(pick, options.length - 1)]!); return }
      if (e.key === 'Escape') { e.preventDefault(); setCaret(-1); return }
    }
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); void submit() }
  }
  const names = nameList(idioma, mentions.map((id) => bots.find((b) => b.id === id)?.name ?? '').filter(Boolean))
  const active = options.length ? options[Math.min(pick, options.length - 1)]! : null
  const hint = !mentions.length ? t('projetos.tarefas.composer.semMencao')
    : onlyNote ? t('projetos.tarefas.composer.soAnotar')
      : t('projetos.tarefas.composer.comMencao', { nomes: names })
  return (
    <div className="tf-composer">
      <div className="tf-composer-box">
        <textarea ref={area} className="tf-composer-input" rows={2} value={text} maxLength={TASK_LIMITS.comment} readOnly={blocked || sending} aria-disabled={blocked || undefined}
          placeholder={t('projetos.tarefas.composer.placeholder')} aria-label={t('projetos.tarefas.composer.rotulo')} aria-describedby="tf-composer-hint"
          role="combobox" aria-multiline aria-haspopup="listbox" aria-autocomplete="list" aria-expanded={options.length > 0} aria-controls={options.length ? 'tf-mention-list' : undefined}
          aria-activedescendant={active ? `tf-mention-${active.id}` : undefined}
          onChange={(e) => { setText(e.target.value); setCaret(e.target.selectionStart ?? e.target.value.length); setPick(0) }}
          onSelect={(e) => setCaret(e.currentTarget.selectionStart ?? 0)} onKeyDown={onKey} />
        <button className="tf-send" aria-label={t('projetos.tarefas.composer.enviar')} title={t('projetos.tarefas.composer.enviar')} aria-disabled={blocked || sending || !text.trim() || undefined} onClick={() => void submit()}>
          <Icon name="arrow.up" size={13} weight="semibold" />
        </button>
        {options.length ? (
          <ul id="tf-mention-list" className="tf-mentions" role="listbox" aria-label={t('projetos.tarefas.composer.mencionar')}>
            {options.map((b) => (
              // Sem botão dentro da opção: nada do menu entra na ordem do Tab; o clique não tira o foco do campo.
              <li key={b.id} id={`tf-mention-${b.id}`} role="option" aria-selected={b.id === active?.id} className={b.id === active?.id ? 'is-on' : ''}
                onMouseDown={(e) => { e.preventDefault(); insert(b) }}>@{b.name}</li>
            ))}
          </ul>
        ) : null}
      </div>
      <div className="tf-composer-foot">
        <p id="tf-composer-hint" className="tf-muted">{hint}</p>
        <Switch on={onlyNote} onChange={setOnlyNote} label={t('projetos.tarefas.composer.soAnotarRotulo')} disabled={blocked || !mentions.length}>{t('projetos.tarefas.composer.soAnotarRotulo')}</Switch>
      </div>
    </div>
  )
}

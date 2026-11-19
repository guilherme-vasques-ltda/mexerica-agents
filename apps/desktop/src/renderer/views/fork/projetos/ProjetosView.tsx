// D541 fatia 1 + MX-142 (docs/reference/redesenho/components/Tarefas/README.md): peças da área Tarefas que vêm do #620 —
// sessão por fonte, relógio, textos de falha, aviso de gravação, folha de Projeto e campos. A página (Lista, Quadro e
// detalhe) mora em TarefasView.tsx. Nada aqui inicia bot ou envia mensagem; tudo que muda espera a resposta do Hub.
import { useEffect, useMemo, useState, type KeyboardEvent, type ReactNode } from 'react'
import {
  botLabel, draftOf, draftProblem, editState, keepMine,
  projectFields, sameProjectFields, PROJECT_LIMITS, type ProjectFields, type ProjectBot, type ProjectSnapshot,
  type ProjectTaskState, type DraftProblem,
} from '@shared/fork-projetos'
import { Icon } from '../../../components/Icon'
import { Dialog, ModalHeader, Sheet } from '../../../components/Overlay'
import { useIdioma, useT, superficieIdioma, type Tradutor } from '../../../lib/fork-i18n/estado'
import type { BotStore } from '../../../store/bot-store'
import { ProjectsSession, type ProjectsFailure, type WriteState } from './projetos-session'
import '../torre/torre.css'
import './projetos.css'

/**
 * Uma sessão por fonte (o BotStore de um contexto/conta + computador) enquanto o app está aberto (só memória): outra
 * conta no mesmo computador nunca reaproveita a fotografia de outra; outro Hub no lugar começa outra. Trocar de Space
 * reaproveita a mesma sessão, que apaga a projeção e o pedido do Space anterior (nada de outro Space fica guardado).
 */
const sessions = new WeakMap<BotStore, ProjectsSession>()
export function sessionFor(store: BotStore) {
  let s = sessions.get(store)
  if (!s || s.phase.kind === 'moved') {
    s = new ProjectsSession()
    sessions.set(store, s)
  }
  return s
}

type Clock = (ms: number) => string

export function useClock(): { time: Clock; when: Clock; day: Clock } {
  const idioma = useIdioma()
  return useMemo(() => {
    const time = (ms: number) => new Date(ms).toLocaleTimeString(idioma, { hour: '2-digit', minute: '2-digit' })
    const when = (ms: number) => {
      const d = new Date(ms)
      return d.toDateString() === new Date().toDateString() ? time(ms) : d.toLocaleString(idioma, { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' })
    }
    const day = (ms: number) => new Date(ms).toLocaleString(idioma, { weekday: 'short', day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' })
    return { time, when, day }
  }, [idioma])
}

export function failureText(t: Tradutor, reason: ProjectsFailure) {
  switch (reason) {
    case 'offline': return t('projetos.erro.offline')
    case 'unavailable': return t('projetos.erro.unavailable')
    case 'lostAccess': return t('projetos.erro.lostAccess')
    case 'authority': return t('projetos.erro.authority')
    case 'invalid': return t('projetos.erro.invalid')
    case 'failed': return t('projetos.erro.failed')
  }
}

function stepText(t: Tradutor, w: WriteState, why: string): string | null {
  if (w.op === 'start') {
    if (w.start === 'failed') return t('projetos.gravar.inicioRetenta')
    if (w.start === 'refused') return `${t('projetos.gravar.inicioRecusado')}${why}`
    return w.status === 'uncertain' ? t('projetos.gravar.inicioIncerto') : `${t('projetos.gravar.inicioFalhou')}${why}`
  }
  if (w.op === 'schedule') return w.status === 'uncertain' ? t('projetos.gravar.agendaIncerta') : `${t('projetos.gravar.agendaRecusada')}${why}`
  if (w.op === 'meta') return w.status === 'uncertain' ? t('projetos.gravar.metaIncerta') : w.status === 'conflict' ? t('projetos.gravar.metaConflito') : `${t('projetos.gravar.metaRecusada')}${why}`
  return null
}

function writeText(t: Tradutor, w: WriteState) {
  const why = w.detail ? t('projetos.gravar.motivo', { motivo: w.detail }) : ''
  const step = stepText(t, w, why)
  // Criação/edição em passos: o 1º gravou; só o passo seguinte (metadados, agenda ou iniciar) ficou de fora.
  if (w.partial && step) return `${w.partial === 'created' ? t('projetos.gravar.parcialCriada') : t('projetos.gravar.parcialSalva')} ${step}`
  if (w.op === 'comment') {
    return w.status === 'uncertain' ? t('projetos.gravar.comentarioIncerto') : `${t('projetos.gravar.comentarioRecusado')}${why}`
  }
  if (w.op === 'meta' && w.status === 'uncertain') return t('projetos.gravar.uncertain')
  if (step) return step
  return w.status === 'uncertain' ? t('projetos.gravar.uncertain')
    : w.status === 'conflict' ? t('projetos.gravar.conflict')
      : w.detail ? t('projetos.gravar.rejectedDetalhe', { motivo: w.detail }) : t('projetos.gravar.rejected')
}

/**
 * O estado da última gravação. Incerta: Tentar de novo reenvia o MESMO pedido. Metadados em conflito ou recusados
 * (inclusive no sucesso parcial de uma criação): Tentar de novo relê o Projeto e manda os mesmos metadados.
 */
export function WriteAlert({ session, onDone }: { session: ProjectsSession; onDone?: () => void }) {
  const t = useT()
  const w: WriteState | null = session.write
  if (!w) return null
  if (w.status === 'sending') return <p className="pk-write" role="status"><Icon name="arrow.triangle.2.circlepath" size={13} /> {w.op === 'comment' ? t('projetos.gravar.enviandoComentario') : w.op === 'start' ? t('projetos.gravar.iniciando') : t('projetos.gravar.sending')}</p>
  const canRetryMeta = !!w.meta && (w.status === 'conflict' || w.status === 'rejected')
  const retry = w.status === 'uncertain' ? () => (w.meta ? session.retryMeta() : session.retry()) : canRetryMeta ? () => session.retryMeta() : null
  return (
    <div className={`pk-write is-${(w.partial || w.start === 'failed') && w.status !== 'uncertain' ? 'uncertain' : w.status}`} role="alert">
      <Icon name="exclamationmark.triangle" size={14} />
      <p>{writeText(t, w)}</p>
      {retry ? <button className="torre-button is-primary" disabled={session.stale} onClick={() => void retry().then((r) => { if (r.ok && w.op !== 'ensureDefault') onDone?.() })}>{t('projetos.gravar.tentar')}</button> : null}
      <button className="torre-button" onClick={() => session.dismissWrite()}>{w.status === 'uncertain' ? t('projetos.gravar.descartar') : t('projetos.gravar.fechar')}</button>
    </div>
  )
}
export function BotName({ bots, id, none, unavailable }: { bots: readonly ProjectBot[]; id: string | null; none: string; unavailable: string }) {
  const label = botLabel(bots, id)
  if (label.kind === 'bot') return <>{label.name}</>
  return <span className={label.kind === 'unavailable' ? 'pk-unavailable' : 'pk-muted'}>{label.kind === 'unavailable' ? unavailable : none}</span>
}

const COLUMN_KEY = {
  backlog: 'projetos.coluna.backlog',
  in_progress: 'projetos.coluna.in_progress',
  review: 'projetos.coluna.review',
  done: 'projetos.coluna.done',
  blocked: 'projetos.coluna.blocked',
  cancelled: 'projetos.coluna.cancelled',
} as const

export function columnName(t: Tradutor, s: ProjectTaskState) {
  return t(COLUMN_KEY[s])
}

// MARK: folhas

export function problemText(t: Tradutor, p: DraftProblem) {
  switch (p) {
    case 'nameRequired': return t('projetos.problema.nameRequired')
    case 'titleRequired': return t('projetos.problema.titleRequired')
    case 'tooLong': return t('projetos.problema.tooLong')
    case 'tooManyTasks': return t('projetos.problema.tooManyTasks')
    case 'badParent': return t('projetos.problema.badParent')
    case 'removedTask': return t('projetos.problema.removedTask')
  }
}

export function Field({ label, note, children, id }: { label: string; note?: string; children: ReactNode; id: string }) {
  return (
    <div className="pk-field">
      <label htmlFor={id}>{label}</label>
      {children}
      {note ? <p className="pk-muted" id={`${id}-note`}>{note}</p> : null}
    </div>
  )
}

/** Por que o formulário não salva agora (sem gravação liberada ou Projeto arquivado); Enter repete o aviso. */
export function BlockedNote({ text, nudge }: { text: string | null; nudge: number }) {
  return text ? <p key={nudge} className="pk-blocked" role="status">{text}</p> : null
}

/** Mudou em outro aparelho com a folha aberta: nada é salvo até o dono escolher qual versão fica. */
export function ConflictNote({ text, nudge, onUseCurrent, onKeepMine }: { text: string; nudge: number; onUseCurrent: () => void; onKeepMine: () => void }) {
  const t = useT()
  return (
    <div key={nudge} className="pk-write is-conflict pk-edit-conflict" role="alert">
      <Icon name="exclamationmark.triangle" size={14} />
      <p>{text}</p>
      <button type="button" className="torre-button" onClick={onUseCurrent}>{t('projetos.conflito.usarAtual')}</button>
      <button type="button" className="torre-button" onClick={onKeepMine}>{t('projetos.conflito.manterMinha')}</button>
    </div>
  )
}

/** Enter num campo de uma linha com o formulário bloqueado: explica em vez de não fazer nada. */
export function onLockedEnter(locked: boolean, nudge: () => void) {
  return (e: KeyboardEvent<HTMLFormElement>) => {
    if (!locked || e.key !== 'Enter' || (e.target as HTMLElement).tagName !== 'INPUT') return
    e.preventDefault()
    nudge()
  }
}

export function BotSelect({ id, bots, value, none, unavailable, disabled, onChange, describedBy }: { id: string; bots: readonly ProjectBot[]; value: string | null; none: string; unavailable: string; disabled: boolean; onChange: (v: string | null) => void; describedBy?: string }) {
  const missing = value && !bots.some((b) => b.id === value)
  return (
    <select id={id} className="pk-input" value={value ?? ''} disabled={disabled} aria-describedby={describedBy} onChange={(e) => onChange(e.target.value || null)}>
      <option value="">{none}</option>
      {/* Referência a bot que não está mais visível: fica como está, sem nome. */}
      {missing ? <option value={value}>{unavailable}</option> : null}
      {bots.map((b) => <option key={b.id} value={b.id}>{b.name}</option>)}
    </select>
  )
}

export function ProjectFormSheet({ open, mode, bots, spaceName, session, snap, onClose }: { open: boolean; mode: 'create' | 'edit'; bots: readonly ProjectBot[]; spaceName: string; session: ProjectsSession; snap?: ProjectSnapshot; onClose: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  const [name, setName] = useState('')
  const [objective, setObjective] = useState('')
  const [coordinator, setCoordinator] = useState<string | null>(null)
  const [problem, setProblem] = useState<DraftProblem | null>(null)
  const [confirmArchive, setConfirmArchive] = useState(false)
  /** O que o Hub tinha quando a folha abriu (ou quando o dono escolheu uma versão): a base da edição. */
  const [base, setBase] = useState<ProjectFields | null>(null)
  const [nudge, setNudge] = useState(0)
  const fill = (f: ProjectFields | null) => {
    setName(f?.name ?? '')
    setObjective(f?.objective ?? '')
    setCoordinator(f?.coordinatorBotId ?? null)
    setBase(f)
  }
  useEffect(() => {
    if (!open) return
    fill(snap && mode === 'edit' ? projectFields(snap.project) : null)
    setProblem(null)
    // Só ao abrir: uma releitura com a folha aberta não apaga o que o dono digitou.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])
  const now = snap && mode === 'edit' ? projectFields(snap.project) : undefined
  const mine: ProjectFields | null = base ? { name, objective, coordinatorBotId: coordinator, state: base.state } : null
  const edit = open && base && mine ? editState(base, mine, now, sameProjectFields) : 'clean'
  // Sem mudança do dono, a folha acompanha a versão atual; com mudança dos dois lados, é conflito explícito.
  useEffect(() => { if (edit === 'follow') fill(now ?? null) })
  const conflict = edit === 'conflict' || edit === 'gone'
  const sending = session.write?.status === 'sending'
  const locked = !session.canWrite
  const blockedText = locked && !sending && session.write?.status !== 'uncertain' ? t('projetos.bloqueado.salvar') : null
  const submit = async (state?: 'active' | 'archived') => {
    if (locked) return setNudge((n) => n + 1)
    if (mode === 'edit' && conflict) return setNudge((n) => n + 1)
    if (mode === 'create') {
      const p = draftProblem({ name, objective, coordinatorBotId: coordinator, state: 'active', tasks: [] })
      setProblem(p)
      if (p) return
      const r = await session.create({ name: name.trim(), objective: objective.trim(), coordinatorBotId: coordinator })
      if (r.ok) onClose()
      return
    }
    // Compara e monta sobre a fotografia ATUAL da sessão (não a do render): tarefas e estado partem do que o Hub tem
    // agora, então mudança em tarefa feita em outro aparelho não se perde; campo do Projeto mudado lá é conflito.
    const cur = session.project
    if (!cur || !base || !mine) return
    const at = editState(base, mine, projectFields(cur.project), sameProjectFields)
    if (at !== 'clean') return setNudge((n) => n + 1)
    const draft = { ...draftOf(cur), name: name.trim(), objective: objective.trim(), coordinatorBotId: coordinator, ...(state ? { state } : {}) }
    const p = draftProblem(draft, cur.tasks)
    setProblem(p)
    if (p) return
    const r = await session.save(draft, cur)
    if (r.ok) onClose()
  }
  const archived = snap?.project.state === 'archived'
  return (
    <Sheet language={superficieIdioma(idioma)} open={open} onClose={onClose} width={480}>
      <div className="pk-sheet" {...superficieIdioma(idioma)}>
        <ModalHeader title={mode === 'create' ? t('projetos.form.tituloNovo') : t('projetos.form.tituloEditar')} closeTitle={t('projetos.form.cancelar')} icon="list.bullet.rectangle" />
        <form className="pk-form" onKeyDown={onLockedEnter(locked, () => setNudge((n) => n + 1))} onSubmit={(e) => { e.preventDefault(); void submit() }}>
          <Field id="pk-name" label={t('projetos.form.nome')}>
            <input id="pk-name" className="pk-input" data-autofocus value={name} maxLength={PROJECT_LIMITS.name} readOnly={sending} required onChange={(e) => setName(e.target.value)} />
          </Field>
          <Field id="pk-objective" label={t('projetos.form.objetivo')}>
            <textarea id="pk-objective" className="pk-input" rows={3} value={objective} maxLength={PROJECT_LIMITS.objective} readOnly={sending} onChange={(e) => setObjective(e.target.value)} />
          </Field>
          <Field id="pk-coordinator" label={t('projetos.form.coordenador')} note={bots.length ? t('projetos.form.coordenadorNota') : t('projetos.semBotsSpace', { space: spaceName })}>
            <BotSelect id="pk-coordinator" bots={bots} value={coordinator} none={t('projetos.semCoordenador')} unavailable={t('projetos.coordenadorIndisponivel')} disabled={sending} onChange={setCoordinator} describedBy="pk-coordinator-note" />
          </Field>
          {problem ? <p className="pk-problem" role="alert">{problemText(t, problem)}</p> : null}
          {conflict ? <ConflictNote text={t('projetos.conflito.projeto')} nudge={nudge} onUseCurrent={() => fill(now ?? null)} onKeepMine={() => {
            if (!now || !base || !mine) return
            const kept = keepMine(base, mine, now)
            setName(kept.name); setObjective(kept.objective); setCoordinator(kept.coordinatorBotId); setBase(now)
          }} /> : null}
          <BlockedNote text={blockedText} nudge={nudge} />
          <WriteAlert session={session} onDone={onClose} />
          {mode === 'edit' ? (
            <div className="pk-archive">
              <p className="pk-muted">{t('projetos.form.arquivarNota')}</p>
              {archived
                ? <button type="button" className="torre-button" disabled={locked} onClick={() => void submit('active')}>{t('projetos.form.reativar')}</button>
                : <button type="button" className="torre-button pk-danger" disabled={locked} onClick={() => setConfirmArchive(true)}>{t('projetos.form.arquivar')}</button>}
            </div>
          ) : null}
          <div className="pk-actions">
            <button type="button" className="torre-button" onClick={onClose}>{t('projetos.form.cancelar')}</button>
            <button type="submit" className="torre-button is-primary" disabled={locked} aria-busy={sending || undefined}>
              {sending ? t('projetos.form.salvando') : mode === 'create' ? t('projetos.form.criar') : t('projetos.form.salvar')}
            </button>
          </div>
        </form>
      </div>
      <Dialog open={confirmArchive} onClose={() => setConfirmArchive(false)} language={superficieIdioma(idioma)}
        title={t('projetos.form.arquivarConfirma', { nome: snap?.project.name ?? '' })} message={t('projetos.form.arquivarNota')} cancel={t('projetos.form.cancelar')}
        actions={[{ title: t('projetos.form.arquivarAcao'), destructive: true, action: () => void submit('archived') }]} />
    </Sheet>
  )
}

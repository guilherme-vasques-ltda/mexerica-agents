// MX-142 etapa 1: folha de Nova tarefa e de Editar tarefa. A tarefa SEMPRE tem Projeto (decisão 7): já vem o último
// usado neste aparelho, senão o padrão do Hub (“Geral”, só se o Hub o anunciar), senão o primeiro ativo; sem nenhum,
// a folha leva a Novo projeto. Prioridade, prazo e fases só aparecem com `taskMeta` e são gravados num 2º pedido
// (o Hub não promete os dois juntos): se só ele falhar, a tela diz que a tarefa foi gravada e o resto não.
import { useEffect, useMemo, useState } from 'react'
import {
  blankTask, depthOf, draftOf, draftProblem, editState, keepMine, parentOptions, PROJECT_LIMITS, PROJECT_TASK_STATES,
  sameTaskContent, withTask, type DraftProblem, type ProjectTask, type ProjectTaskState,
} from '@shared/fork-projetos'
import {
  agendaPending, EMPTY_META, followCurrentPhase, fromLocalInput, metaProblem, phasesFromText, pickProject, sameMeta, sheetMeta, syncSheetMeta, TASK_LIMITS, TASK_PRIORITIES, toLocalInput,
  type MetaProblem, type TaskMeta, type TaskPriority,
} from '@shared/fork-tarefas'
import { Icon } from '../../../components/Icon'
import { ModalHeader, Sheet } from '../../../components/Overlay'
import { useIdioma, useT, superficieIdioma, type Tradutor } from '../../../lib/fork-i18n/estado'
import { BlockedNote, BotSelect, columnName, ConflictNote, Field, onLockedEnter, problemText, WriteAlert } from './ProjetosView'
import type { ProjectsSession } from './projetos-session'
import { priorityName, type FaceOf } from './TarefasView'
import { lastProjectFor, rememberLastProject } from './tarefas-prefs'

export type TaskSheetTarget =
  | { kind: 'new'; projectId?: string; parentTaskId?: string }
  | { kind: 'edit'; projectId: string; taskId: string }

function metaProblemText(t: Tradutor, p: MetaProblem) {
  switch (p) {
    case 'tooManyPhases': return t('projetos.tarefas.problema.fasesDemais', { n: TASK_LIMITS.phases })
    case 'phaseTooLong': return t('projetos.problema.tooLong')
    case 'phaseEmpty': return t('projetos.tarefas.problema.faseVazia')
    case 'phasesOnlyRoot': return t('projetos.tarefas.problema.fasesSoPrincipal')
    case 'badDue': return t('projetos.tarefas.problema.prazo')
  }
}

export function TarefaSheet({ session, target, spaceId, spaceName, onNewProject, onClose }: { session: ProjectsSession; target: TaskSheetTarget | null; spaceId: string; spaceName: string; face?: FaceOf; onNewProject: () => void; onClose: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  const open = !!target
  const isNew = target?.kind === 'new'
  const hasMeta = session.has('taskMeta')
  // Hub B: início agendado e “Criar e já iniciar” só com o anúncio (fail-closed).
  const hasSchedule = session.has('taskSchedule')
  const hasStart = session.has('taskStart')
  const projects = (session.list?.projects ?? []).filter((p) => p.state === 'active')
  const [projectId, setProjectId] = useState<string | null>(null)
  const [task, setTask] = useState<ProjectTask | null>(null)
  const [base, setBase] = useState<ProjectTask | null>(null)
  const [meta, setMeta] = useState<TaskMeta>(EMPTY_META)
  const [baseMeta, setBaseMeta] = useState<TaskMeta>(EMPTY_META)
  const [phasesText, setPhasesText] = useState('')
  const [problem, setProblem] = useState<DraftProblem | MetaProblem | null>(null)
  const [nudge, setNudge] = useState(0)
  const [busy, setBusy] = useState(false)
  const [startAt, setStartAt] = useState<number | null>(null)
  const [baseStartAt, setBaseStartAt] = useState<number | null>(null)

  // As tarefas do Projeto escolhido: as do Projeto aberto (mais novas) ou as da Lista.
  const tasksOf = (id: string | null): ProjectTask[] => {
    if (!id) return []
    if (session.project?.project.id === id) return session.project.tasks
    return session.tasks?.find((b) => b.project.id === id)?.tasks ?? []
  }
  const fillMeta = (m: TaskMeta) => { setMeta(m); setBaseMeta(m); setPhasesText(m.phases.join('\n')) }

  useEffect(() => {
    if (!target) return
    setProblem(null)
    if (target.kind === 'new') {
      const pid = target.projectId && projects.some((p) => p.id === target.projectId) ? target.projectId
        : pickProject(projects, lastProjectFor(spaceId), session.list?.defaultProjectId ?? null)
      setProjectId(pid)
      const fresh = { ...blankTask(), parentTaskId: target.parentTaskId ?? null }
      setTask(fresh)
      setBase(null)
      fillMeta(EMPTY_META)
      setStartAt(null)
      setBaseStartAt(null)
    } else {
      const cur = tasksOf(target.projectId).find((x) => x.id === target.taskId) ?? null
      setProjectId(target.projectId)
      setTask(cur ? { ...cur } : null)
      setBase(cur ? { ...cur } : null)
      fillMeta(session.metaOf(target.projectId, target.taskId) ?? EMPTY_META)
      const agenda = session.agendaOf(target.projectId, target.taskId)
      const at = agenda && agendaPending(agenda) ? agenda.startAt : null
      setStartAt(at)
      setBaseStartAt(at)
    }
    // Só ao abrir: uma releitura com a folha aberta não apaga o que o dono digitou.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target])

  const projectTasks = tasksOf(projectId)
  const now = !isNew && task ? projectTasks.find((x) => x.id === task.id) : undefined
  const nowMeta = !isNew && task && projectId ? session.metaOf(projectId, task.id) ?? EMPTY_META : EMPTY_META
  const edit = open && !isNew && task && base ? editState(base, task, now, sameTaskContent) : 'clean'
  // Releitura com a folha aberta: o que o dono mexeu (inclusive fases só digitadas) fica e o resto segue a versão
  // nova, campo a campo; o texto das fases nunca é sobrescrito por uma mudança em outro campo.
  const metaSync = open && !isNew && hasMeta ? syncSheetMeta(baseMeta, meta, phasesText, nowMeta) : null
  useEffect(() => {
    if (edit === 'follow' && now) { setTask({ ...now }); setBase({ ...now }) }
    if (metaSync?.kind === 'rebase') { setMeta(metaSync.meta); setBaseMeta(nowMeta); setPhasesText(metaSync.phasesText) }
  })
  // Nova tarefa aberta sem Projeto: quando a lista passa a ter um (ex.: o Geral garantido pelo Hub), ele é escolhido.
  useEffect(() => {
    if (isNew && !projectId && projects.length) setProjectId(pickProject(projects, lastProjectFor(spaceId), session.list?.defaultProjectId ?? null))
  })
  const conflict = edit === 'conflict' || edit === 'gone' || metaSync?.kind === 'conflict'
  const sending = busy || session.write?.status === 'sending'
  // Sem nenhum Projeto e com o Geral do Hub (`defaultProject`): a tarefa vai para o Geral, criado (ou adotado) pelo
  // Hub ao salvar, de forma idempotente. Sem a capacidade, a folha leva a Novo projeto, como antes.
  const viaGeral = isNew && !projectId && !projects.length && session.has('defaultProject')
  const locked = !session.canWrite || (!projectId && !viaGeral)
  const blockedText = locked && projectId && !sending && session.write?.status !== 'uncertain' ? t('projetos.bloqueado.salvar') : null
  const options = useMemo(() => (task ? parentOptions(isNew ? [...projectTasks, task] : projectTasks, task.id) : []), [projectTasks, task, isNew])
  const set = <K extends keyof ProjectTask>(k: K, v: ProjectTask[K]) => setTask((cur) => (cur ? { ...cur, [k]: v } : cur))
  const bots = session.project?.project.id === projectId ? session.project.bots : session.list?.bots ?? []
  const isRoot = !task?.parentTaskId

  const submit = async (start = false) => {
    if (!task || (!projectId && !viaGeral)) return
    if (locked || conflict) return setNudge((n) => n + 1)
    const mine = { ...task, title: task.title.trim() }
    const m = sheetMeta(meta, phasesText)
    const mp = hasMeta ? metaProblem(m, mine) : null
    if (!mine.title) return setProblem('titleRequired')
    if (mp) return setProblem(mp)
    const metaChanged = hasMeta && !sameMeta(isNew ? EMPTY_META : baseMeta, m)
    let late: DraftProblem | null = null
    const build = (snap: Parameters<Parameters<ProjectsSession['saveFresh']>[1]>[0]) => {
      const d = withTask(draftOf(snap), mine)
      late = draftProblem(d, snap.tasks)
      return late ? null : d
    }
    setProblem(null)
    setBusy(true)
    try {
      if (isNew) {
        let pid = projectId
        if (!pid) {
          // Jornada sem Projeto (decisão 7): o Hub garante o Geral; a tarefa só é criada depois da resposta dele.
          const g = await session.ensureDefaultProject()
          if (!g.ok) return
          pid = g.projectId
          setProjectId(pid)
        }
        const r = await session.createTask(pid, build, mine.id, metaChanged ? m : null, start && hasStart)
        if (late) return setProblem(late)
        rememberLastProject(spaceId, pid)
        if (!r.ok) return
        // Início agendado: outro pedido, depois da tarefa criada (sucesso parcial visível se só ele falhar).
        if (hasSchedule && startAt !== null && !(await session.scheduleTask(pid, mine.id, startAt, 'created')).ok) return
        onClose()
        return
      }
      if (!projectId) return
      const fieldsChanged = !base || !sameTaskContent(base, mine)
      if (fieldsChanged) {
        const r = await session.saveFresh(projectId, (snap) => {
          const cur = snap.tasks.find((x) => x.id === mine.id)
          if (!cur) return null
          // Mudou lá enquanto a folha estava aberta: só o dono decide (nada é enviado).
          if (base && editState(base, mine, cur, sameTaskContent) === 'conflict') return 'outdated'
          return build(snap)
        })
        if (late) return setProblem(late)
        if (!r.ok) { if (r.status === 'outdated') setNudge((n) => n + 1); return }
      }
      if (metaChanged) {
        // Base: o meta que o dono viu ao abrir (ou escolheu no conflito); a sessão confere contra o snapshot relido.
        const r = await session.saveMeta(projectId, mine.id, m, fieldsChanged ? 'saved' : undefined, baseMeta)
        if (!r.ok) return
      }
      if (hasSchedule && startAt !== baseStartAt) {
        const r = await session.scheduleTask(projectId, mine.id, startAt, fieldsChanged || metaChanged ? 'saved' : undefined)
        if (!r.ok) return
      }
      onClose()
    } finally {
      setBusy(false)
    }
  }

  const phases = phasesFromText(phasesText, meta.currentPhase).phases
  // Editar as fases: a fase atual acompanha o nome dela, não a posição.
  const changePhases = (text: string) => {
    const next = phasesFromText(text, null).phases
    setMeta((m) => ({ ...m, currentPhase: followCurrentPhase(phases, m.currentPhase, next) }))
    setPhasesText(text)
  }
  return (
    <Sheet language={superficieIdioma(idioma)} open={open} onClose={onClose} width={560} maxHeight="88vh">
      {task ? (
        <div className="pk-sheet" {...superficieIdioma(idioma)}>
          <ModalHeader title={isNew ? (task.parentTaskId ? t('projetos.tarefas.novaSubtarefa') : t('projetos.tarefa.tituloNova')) : task.title || t('projetos.tarefa.tituloEditar')}
            subtitle={projects.find((p) => p.id === projectId)?.name} closeTitle={t('projetos.form.cancelar')} icon="square.and.pencil" />
          <form className="pk-form" onKeyDown={onLockedEnter(locked, () => setNudge((n) => n + 1))} onSubmit={(e) => { e.preventDefault(); void submit() }}>
            {isNew && !target?.parentTaskId ? (
              projects.length ? (
                <Field id="tf-project" label={t('projetos.tarefas.projeto')} note={t('projetos.tarefas.projetoNota')}>
                  <select id="tf-project" className="pk-input" value={projectId ?? ''} disabled={sending} aria-describedby="tf-project-note"
                    onChange={(e) => { setProjectId(e.target.value); set('parentTaskId', null) }}>
                    {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
                  </select>
                </Field>
              ) : viaGeral ? (
                <Field id="tf-project" label={t('projetos.tarefas.projeto')} note={t('projetos.tarefas.geralAoSalvar')}>
                  <input id="tf-project" className="pk-input" value={t('projetos.tarefas.geral')} readOnly aria-describedby="tf-project-note" />
                </Field>
              ) : (
                <div className="pk-archive">
                  <p>{t('projetos.tarefas.semProjeto', { space: spaceName })}</p>
                  <button type="button" className="torre-button is-primary" disabled={!session.canWrite} onClick={onNewProject}><Icon name="plus" size={12} /> {t('projetos.novo')}</button>
                </div>
              )
            ) : null}
            <Field id="tf-title" label={t('projetos.tarefa.titulo')}>
              <input id="tf-title" className="pk-input" data-autofocus value={task.title} maxLength={PROJECT_LIMITS.title} readOnly={sending || locked} required onChange={(e) => set('title', e.target.value)} />
            </Field>
            <div className="pk-row">
              <Field id="tf-state" label={t('projetos.tarefa.estado')}>
                <select id="tf-state" className="pk-input" value={task.state} disabled={sending || locked} onChange={(e) => set('state', e.target.value as ProjectTaskState)}>
                  {PROJECT_TASK_STATES.map((s) => <option key={s} value={s}>{columnName(t, s)}</option>)}
                </select>
              </Field>
              <Field id="tf-assignee" label={t('projetos.tarefa.responsavel')}>
                <BotSelect id="tf-assignee" bots={bots} value={task.assigneeBotId} none={t('projetos.semResponsavel')} unavailable={t('projetos.responsavelIndisponivel')} disabled={sending || locked} onChange={(v) => set('assigneeBotId', v)} describedBy="tf-assignee-hint" />
              </Field>
            </div>
            <p className="pk-muted" id="tf-assignee-hint">{bots.length ? t('projetos.tarefa.responsavelNota') : t('projetos.semBotsSpace', { space: spaceName })}</p>
            <Field id="tf-parent" label={t('projetos.tarefa.pai')}>
              <select id="tf-parent" className="pk-input" value={task.parentTaskId ?? ''} disabled={sending || locked} onChange={(e) => set('parentTaskId', e.target.value || null)}>
                <option value="">{t('projetos.tarefa.semPai')}</option>
                {task.parentTaskId && !options.some((o) => o.id === task.parentTaskId) ? <option value={task.parentTaskId}>{projectTasks.find((x) => x.id === task.parentTaskId)?.title ?? ''}</option> : null}
                {options.map((o) => <option key={o.id} value={o.id}>{'— '.repeat(depthOf(projectTasks, o.id) - 1)}{o.title}</option>)}
              </select>
            </Field>
            {hasMeta ? (
              <>
                <div className="pk-row">
                  <Field id="tf-priority" label={t('projetos.tarefas.prioridadeRotulo')}>
                    <select id="tf-priority" className="pk-input" value={meta.priority ?? ''} disabled={sending || locked} onChange={(e) => setMeta((m) => ({ ...m, priority: (e.target.value || null) as TaskPriority | null }))}>
                      <option value="">{t('projetos.tarefas.semPrioridade')}</option>
                      {TASK_PRIORITIES.map((p) => <option key={p} value={p}>{priorityName(t, p)}</option>)}
                    </select>
                  </Field>
                  <Field id="tf-due" label={t('projetos.tarefas.prazoRotulo')}>
                    <input id="tf-due" type="datetime-local" className="pk-input" value={toLocalInput(meta.dueAt)} readOnly={sending || locked} onChange={(e) => setMeta((m) => ({ ...m, dueAt: fromLocalInput(e.target.value) }))} />
                  </Field>
                </div>
                <p className="pk-muted">{t('projetos.tarefas.metaNota')}</p>
                {isRoot ? (
                  <div className="pk-row">
                    <Field id="tf-phases" label={t('projetos.tarefas.fases')} note={t('projetos.tarefas.fasesNota')}>
                      <textarea id="tf-phases" className="pk-input" rows={3} value={phasesText} readOnly={sending || locked} aria-describedby="tf-phases-note" onChange={(e) => changePhases(e.target.value)} />
                    </Field>
                    <Field id="tf-phase" label={t('projetos.tarefas.faseAtual')}>
                      <select id="tf-phase" className="pk-input" value={meta.currentPhase ?? ''} disabled={sending || locked || !phases.length} onChange={(e) => setMeta((m) => ({ ...m, currentPhase: e.target.value === '' ? null : Number(e.target.value) }))}>
                        <option value="">{t('projetos.tarefas.semFaseAtual')}</option>
                        {phases.map((p, i) => <option key={i} value={i}>{p}</option>)}
                      </select>
                    </Field>
                  </div>
                ) : null}
              </>
            ) : null}
            {hasSchedule ? (
              <Field id="tf-start" label={t('projetos.tarefas.inicioRotulo')} note={t('projetos.tarefas.inicioNota')}>
                <input id="tf-start" type="datetime-local" className="pk-input" value={toLocalInput(startAt)} readOnly={sending || locked} aria-describedby="tf-start-note" onChange={(e) => setStartAt(fromLocalInput(e.target.value))} />
              </Field>
            ) : null}
            <Field id="tf-description" label={t('projetos.tarefa.descricao')}>
              <textarea id="tf-description" className="pk-input" rows={3} value={task.description} maxLength={PROJECT_LIMITS.description} readOnly={sending || locked} onChange={(e) => set('description', e.target.value)} />
            </Field>
            <Field id="tf-acceptance" label={t('projetos.tarefa.aceite')}>
              <textarea id="tf-acceptance" className="pk-input" rows={2} value={task.acceptance} maxLength={PROJECT_LIMITS.acceptance} readOnly={sending || locked} onChange={(e) => set('acceptance', e.target.value)} />
            </Field>
            <Field id="tf-evidence" label={t('projetos.tarefa.evidencia')} note={t('projetos.tarefa.evidenciaNota')}>
              <textarea id="tf-evidence" className="pk-input" rows={2} value={task.evidence} maxLength={PROJECT_LIMITS.evidence} readOnly={sending || locked} aria-describedby="tf-evidence-note" onChange={(e) => set('evidence', e.target.value)} />
            </Field>
            <p className="pk-muted"><Icon name="info.circle" size={12} /> {t('projetos.tarefa.cancelada')}</p>
            {problem ? <p className="pk-problem" role="alert">{problem in META_PROBLEMS ? metaProblemText(t, problem as MetaProblem) : problemText(t, problem as DraftProblem)}</p> : null}
            {conflict ? <ConflictNote text={t('projetos.conflito.tarefa')} nudge={nudge} onUseCurrent={() => { if (now) { setTask({ ...now }); setBase({ ...now }) } fillMeta(nowMeta) }} onKeepMine={() => {
              if (now && base && task) { setTask(keepMine(base, task, now)); setBase({ ...now }) }
              // Metadados: o campo do dono vale no conflito; os outros seguem a versão atual; as fases digitadas ficam.
              if (metaSync?.kind === 'conflict') { setMeta(metaSync.meta); setPhasesText(metaSync.phasesText) }
              setBaseMeta(nowMeta)
            }} /> : null}
            <BlockedNote text={blockedText} nudge={nudge} />
            <WriteAlert session={session} onDone={onClose} />
            <div className="pk-actions">
              <button type="button" className="torre-button" onClick={onClose}>{t('projetos.form.cancelar')}</button>
              {isNew && hasStart ? (
                <button type="button" className="torre-button" disabled={locked || sending || !task.assigneeBotId} title={task.assigneeBotId ? t('projetos.tarefas.iniciarNota') : t('projetos.tarefas.criarIniciarSemBot')} onClick={() => void submit(true)}>
                  {t('projetos.tarefas.criarIniciar')}
                </button>
              ) : null}
              <button type="submit" className="torre-button is-primary" disabled={locked} aria-busy={sending || undefined}>
                {sending ? t('projetos.form.salvando') : isNew ? t('projetos.tarefas.criarTarefa') : t('projetos.form.salvar')}
              </button>
            </div>
          </form>
        </div>
      ) : open ? (
        <div className="pk-sheet"><p className="torre-state" role="status">{t('projetos.tarefas.sumiu')}</p></div>
      ) : null}
    </Sheet>
  )
}

const META_PROBLEMS: Record<MetaProblem, true> = { tooManyPhases: true, phaseTooLong: true, phaseEmpty: true, phasesOnlyRoot: true, badDue: true }

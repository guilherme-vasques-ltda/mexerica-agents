import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { TowerEvent, TowerLink } from '../torre/torre-session.ts'
import { legacyHistoryRefusal, ProjectsSession } from './projetos-session.ts'
import { draftOf, withTask } from '../../../../shared/fork-projetos.ts'
import { declaredBy, pendingDraft } from '../../../../shared/fork-tarefas.ts'

// MX-142: a sessão da área Tarefas. Lê todas as tarefas só quando a tela pede; toda gravação relê o Projeto inteiro e usa
// a revisão dele; criação em 2 passos mostra sucesso parcial e refaz só os metadados; comentário só com o anúncio do Hub
// e incerto reenvia o MESMO pedido.
const P = '11111111-1111-4111-8111-111111111111'
const Q = '22222222-2222-4222-8222-222222222222'
const T1 = '33333333-3333-4333-8333-333333333333'
const G = '88888888-8888-4888-8888-888888888888'
const project = (id: string, rev: number, state = 'active') => ({ id, name: id === P ? 'Canal' : 'Loja', objective: '', coordinatorBotId: null, state, revision: rev, createdAt: 1, updatedAt: rev })
const task = { id: T1, parentTaskId: null, assigneeBotId: 'b1', title: 'Roteiro', description: '', acceptance: '', evidence: '', state: 'backlog' as const }
class HostErr extends Error { status: number; constructor(status: number, m = 'x') { super(m); this.status = status } }

class Hub {
  caps = ['manualProjects']
  rev = new Map([[P, 4], [Q, 2]])
  tasks = new Map<string, unknown[]>([[P, [task]], [Q, []]])
  meta: Record<string, unknown> = {}
  /** Contrato novo: `meta` também no snapshot do Projeto (mesma revisão). */
  snapMeta = true
  failSaveOnce = false
  /** Space sem nenhum Projeto (jornada do Geral). */
  empty = false
  log: string[] = []
  fail: Record<string, HostErr | undefined> = {}
  comments: unknown[] = []
  /** Grava o comentário e perde a resposta uma vez (incerto de verdade); o reenvio do MESMO pedido devolve o mesmo. */
  loseCommentAnswer = false
  commentByRequest = new Map<unknown, unknown>()
  startAnswer: unknown = null
  scheduleAnswer: unknown = null
  ensured = 0
  ensuredOnce = 0
  /** Hub anterior ao TEC-722-16: recusa o campo desconhecido `historyAuthors` (serde deny_unknown_fields). */
  old = false
  call(method: string, b: Record<string, unknown>): unknown {
    // Erro REAL do Hub da base (projetos/store.rs `parse`, serde deny_unknown_fields): frase genérica, sem o campo.
    if (this.old && 'historyAuthors' in b) throw new HostErr(400, 'invalid_field: solicitação de Projetos inválida.')
    this.log.push(method === 'donnaProject' ? `${method} ${b.projectId}` : method)
    const f = this.fail[method]
    if (f) { delete this.fail[method]; throw f }
    const snap = (id: string) => ({ version: 1, hubId: 'h1', spaceId: 'p', project: project(id, this.rev.get(id)!), tasks: this.tasks.get(id), history: [], historyOmitted: 0, bots: [{ id: 'b1', name: 'Editor' }], ...(this.snapMeta ? { meta: this.meta } : {}) })
    switch (method) {
      case 'donnaProjects': {
        if (this.empty && !this.rev.has(G)) return { version: 1, hubId: 'h1', spaceId: 'p', projects: [], bots: [{ id: 'b1', name: 'Editor' }], capabilities: this.caps, defaultProjectId: null }
        const ps = this.empty ? [project(G, this.rev.get(G)!)] : [project(P, this.rev.get(P)!), project(Q, this.rev.get(Q)!), project('44444444-4444-4444-8444-444444444444', 1, 'archived')]
        return { version: 1, hubId: 'h1', spaceId: 'p', projects: ps, bots: [{ id: 'b1', name: 'Editor' }], capabilities: this.caps, defaultProjectId: this.rev.has(G) ? G : null }
      }
      case 'donnaProject': return snap(b.projectId as string)
      case 'donnaTasks': {
        const after = b.afterProjectId as string | null
        const id = after === null ? P : after === P ? Q : null
        return { version: 1, hubId: 'h1', spaceId: 'p', page: id ? { project: project(id, this.rev.get(id)!), tasks: this.tasks.get(id), meta: id === P ? { [T1]: { priority: 'high', dueAt: null, phases: [], currentPhase: null } } : {} } : null, next: id === P ? P : null }
      }
      case 'donnaProjectCreate': {
        this.rev.set(Q, this.rev.get(Q)!)
        return { snapshot: snap(Q), receipt: { clientRequestId: b.clientRequestId, revision: this.rev.get(Q) } }
      }
      case 'donnaProjectSave': {
        if (this.failSaveOnce) { this.failSaveOnce = false; throw new HostErr(0, 'timeout') }
        const id = b.projectId as string
        assert.equal(b.expectedRevision, this.rev.get(id), 'gravação usa a revisão relida')
        this.rev.set(id, this.rev.get(id)! + 1)
        this.tasks.set(id, b.tasks as unknown[])
        return { snapshot: snap(id), receipt: { clientRequestId: b.clientRequestId, revision: this.rev.get(id) } }
      }
      case 'donnaTaskMetaSave': {
        const id = b.projectId as string
        if (b.expectedRevision !== this.rev.get(id)) throw new HostErr(409, 'projects_conflict: o Projeto mudou')
        this.rev.set(id, this.rev.get(id)! + 1)
        this.meta[b.taskId as string] = b.meta
        return { snapshot: snap(id), meta: this.meta, receipt: { clientRequestId: b.clientRequestId, revision: this.rev.get(id) } }
      }
      case 'donnaTaskStart': return this.startAnswer ?? { receipt: { clientRequestId: b.clientRequestId }, start: { state: 'started' } }
      case 'donnaTaskSchedule': return this.scheduleAnswer ?? { receipt: { clientRequestId: b.clientRequestId }, agenda: null }
      case 'donnaProjectEnsureDefault': {
        if (!this.rev.has(G)) { this.rev.set(G, 1); this.tasks.set(G, []); this.ensured++ }
        return { version: 1, hubId: 'h1', spaceId: 'p', snapshot: snap(G), created: this.ensured === 1 && !this.ensuredOnce++ }
      }
      case 'donnaTaskComments': return { version: 1, hubId: 'h1', spaceId: 'p', projectId: b.projectId, taskId: b.taskId, comments: this.comments, omitted: 0 }
      case 'donnaTaskComment': {
        const again = this.commentByRequest.get(b.clientRequestId)
        if (again) return { comment: again, receipt: { clientRequestId: b.clientRequestId } }
        const c = { id: `c${this.comments.length + 1}`, at: 9, author: 'owner', text: b.text, mentions: b.mentions, onlyNote: b.onlyNote, notify: { state: 'not_notified', botIds: [], reason: (b.mentions as unknown[]).length ? 'delivery_unavailable' : 'no_mention' } }
        this.comments.push(c)
        this.commentByRequest.set(b.clientRequestId, c)
        if (this.loseCommentAnswer) { this.loseCommentAnswer = false; throw new HostErr(0, 'timeout') }
        return { comment: c, receipt: { clientRequestId: b.clientRequestId } }
      }
    }
    throw new HostErr(404, 'unknown method')
  }
}

class Source {
  hello = { hostId: 'h1', rev: 1 }
  principalEpoch = 1
  hub = new Hub()
  bodies: Record<string, unknown>[] = []
  connectedClient: TowerLink = {
    call: async (method, body) => { this.bodies.push({ method, ...(body as object) }); return this.hub.call(method, body as Record<string, unknown>) },
    inSpace: () => ({ events: (_s: number, _c: string, _on: (e: TowerEvent) => void) => () => {} }),
  }
  subscribe() { return () => {} }
}
const tick = (ms = 0) => new Promise((r) => setTimeout(r, ms))
const ids = () => { let n = 0; return () => `00000000-0000-4000-8000-${String(++n).padStart(12, '0')}` }

async function opened(caps: string[] = ['manualProjects']) {
  const src = new Source()
  src.hub.caps = caps
  const s = new ProjectsSession(() => 42, ids())
  s.open(src, 'p')
  s.watchTasks(true)
  await tick(5)
  return { src, s }
}

test('sem pedir as tarefas, nada além da lista; pedindo, um donnaProject por Projeto ativo', async () => {
  const src = new Source()
  const s = new ProjectsSession(() => 42, ids())
  s.open(src, 'p')
  await tick(5)
  assert.deepEqual(src.hub.log, ['donnaProjects'])
  assert.equal(s.tasks, null)
  s.watchTasks(true)
  await tick(5)
  assert.deepEqual(src.hub.log.slice(1), ['donnaProjects', `donnaProject ${P}`, `donnaProject ${Q}`])
  // Função: o TS não desfaz o estreitamento (`tasks === null`) depois de watchTasks().
  const tasksOf = (x: ProjectsSession) => x.tasks
  assert.deepEqual(tasksOf(s)?.map((b) => b.project.id), [P, Q])
  assert.equal(s.canWrite, true)
  assert.equal(s.has('taskMeta'), false, 'sem anúncio, sem metadados')
  s.close()
})

test('com taskList: donnaTasks paginado traz os metadados', async () => {
  const { src, s } = await opened(['manualProjects', 'taskList', 'taskMeta'])
  assert.deepEqual(src.hub.log, ['donnaProjects', 'donnaTasks', 'donnaTasks'])
  assert.equal(s.metaOf(P, T1)?.priority, 'high')
  s.close()
})

test('mover da Lista relê o Projeto e não troca o Projeto aberto', async () => {
  const { src, s } = await opened()
  src.hub.rev.set(P, 9) // mudou em outro aparelho depois da leitura da Lista
  src.hub.log = []
  const r = await s.saveFresh(P, (snap) => withTask(draftOf(snap), { ...snap.tasks[0]!, state: 'in_progress' }))
  assert.equal(r.ok, true)
  assert.deepEqual(src.hub.log, [`donnaProject ${P}`, 'donnaProjectSave'])
  assert.equal(s.view.kind, 'none')
  assert.equal(s.tasks?.find((b) => b.project.id === P)?.tasks[0]?.state, 'in_progress')
  s.close()
})

test('criar com metadados: 2º passo em conflito vira sucesso parcial; Tentar de novo relê e manda só os metadados', async () => {
  const { src, s } = await opened(['manualProjects', 'taskMeta'])
  const novo = { ...task, id: '55555555-5555-4555-8555-555555555555', title: 'Thumb' }
  const meta = { priority: 'urgent' as const, dueAt: 100, phases: [], currentPhase: null }
  src.hub.fail.donnaTaskMetaSave = new HostErr(409, 'projects_conflict: o Projeto mudou')
  const r = await s.createTask(P, (snap) => withTask(draftOf(snap), novo), novo.id, meta)
  assert.equal(r.ok, false)
  assert.equal(s.write?.partial, 'created')
  assert.equal(s.write?.status, 'conflict')
  assert.ok((src.hub.tasks.get(P) as { id: string }[]).some((x) => x.id === novo.id), 'a tarefa ficou gravada')
  await tick(5)
  src.hub.log = []
  const again = await s.retryMeta()
  assert.equal(again.ok, true)
  assert.deepEqual(src.hub.log.filter((m) => m !== 'donnaProjects' && !m.startsWith('donnaProject ' + Q)), [`donnaProject ${P}`, 'donnaTaskMetaSave'])
  assert.deepEqual(src.hub.meta[novo.id], meta)
  assert.equal(s.write, null)
  s.close()
})

test('metadados sem anúncio: nem tenta (fail-closed) e a tarefa é criada', async () => {
  const { src, s } = await opened()
  const novo = { ...task, id: '66666666-6666-4666-8666-666666666666' }
  const r = await s.createTask(P, (snap) => withTask(draftOf(snap), novo), novo.id, { priority: 'low', dueAt: null, phases: [], currentPhase: null })
  assert.equal(r.ok, true)
  assert.ok(!src.hub.log.includes('donnaTaskMetaSave'))
  s.close()
})

test('comentários: só com taskComments; aviso vem do Hub; incerto reenvia o mesmo pedido', async () => {
  const sem = await opened()
  sem.s.openProject(P)
  sem.s.openComments({ projectId: P, taskId: T1 })
  await tick(5)
  assert.ok(!sem.src.hub.log.includes('donnaTaskComments'))
  assert.equal((await sem.s.sendComment('oi', [], false, { projectId: P, taskId: T1 })).ok, false)
  sem.s.close()

  const { src, s } = await opened(['manualProjects', 'taskComments'])
  s.openProject(P)
  s.openComments({ projectId: P, taskId: T1 })
  await tick(5)
  assert.equal(s.comments?.state, 'ready')
  const r = await s.sendComment('@Editor tira os bastidores', ['b1'], false, { projectId: P, taskId: T1 })
  assert.equal(r.ok, true)
  assert.equal(s.comments?.list[0]?.notify?.reason, 'delivery_unavailable', 'a tela mostra o que o Hub disse, não “avisado”')
  src.hub.fail.donnaTaskComment = new HostErr(0, 'timeout')
  await s.sendComment('só anotando', [], false, { projectId: P, taskId: T1 })
  assert.equal(s.write?.status, 'uncertain')
  const first = src.bodies.filter((b) => b.method === 'donnaTaskComment').at(-1)!.clientRequestId
  await s.retry()
  const second = src.bodies.filter((b) => b.method === 'donnaTaskComment').at(-1)!.clientRequestId
  assert.equal(second, first)
  s.close()
})

test('Projeto arquivado entre a Lista e a gravação: nada é enviado', async () => {
  const { src, s } = await opened()
  src.hub.call = ((orig) => function (this: Hub, m: string, b: Record<string, unknown>) {
    const r = orig.call(this, m, b) as Record<string, unknown>
    if (m === 'donnaProject') return { ...r, project: { ...(r.project as object), state: 'archived' } }
    return r
  })(src.hub.call)
  src.hub.log = []
  const r = await s.saveFresh(P, (snap) => withTask(draftOf(snap), { ...snap.tasks[0]!, state: 'done' }))
  assert.equal(r.ok, false)
  assert.ok(!src.hub.log.includes('donnaProjectSave'))
  assert.equal(s.write?.status, 'rejected')
  s.close()
})

test('rascunho escrito na tarefa A não é enviado depois de abrir a tarefa B (#722)', async () => {
  const { src, s } = await opened(['manualProjects', 'taskComments'])
  const B = '77777777-7777-4777-8777-777777777777'
  src.hub.tasks.set(P, [task, { ...task, id: B, title: 'Cortes' }])
  s.openProject(P)
  s.openComments({ projectId: P, taskId: T1 })
  await tick(5)
  s.openComments({ projectId: P, taskId: B })
  await tick(5)
  src.bodies.length = 0
  const r = await s.sendComment('texto escrito em A', [], false, { projectId: P, taskId: T1 })
  assert.deepEqual(r, { ok: false, status: 'blocked' })
  assert.ok(!src.bodies.some((b) => b.method === 'donnaTaskComment'), 'nada chega ao Hub')
  assert.equal(s.comments?.taskId, B)
  assert.equal(s.comments?.list.length, 0, 'B não ganha o comentário de A')
  assert.equal((await s.sendComment('agora em B', [], false, { projectId: P, taskId: B })).ok, true)
  assert.equal(src.bodies.filter((b) => b.method === 'donnaTaskComment').at(-1)!.taskId, B)
  s.close()
})

test('corrida: outro cliente muda o prazo entre a leitura e a edição; mudar só a prioridade não desfaz o prazo', async () => {
  const { src, s } = await opened(['manualProjects', 'taskMeta'])
  const base = { priority: 'high' as const, dueAt: 100, phases: [], currentPhase: null }
  src.hub.meta[T1] = base
  src.hub.meta[T1] = { ...base, dueAt: 200 } // gravado por outro aparelho depois que a folha abriu
  src.hub.rev.set(P, src.hub.rev.get(P)! + 1)
  const r = await s.saveMeta(P, T1, { ...base, priority: 'low' }, undefined, base)
  assert.equal(r.ok, true)
  assert.deepEqual(src.hub.meta[T1], { ...base, priority: 'low', dueAt: 200 }, 'prazo do outro cliente preservado')
  const sent = src.bodies.filter((b) => b.method === 'donnaTaskMetaSave').at(-1)!
  assert.equal(sent.expectedRevision, src.hub.rev.get(P)! - 1, 'revisão da MESMA leitura que trouxe o meta')
  s.close()
})

test('corrida no mesmo campo: nada é enviado; Tentar de novo manda o valor do dono sobre a versão vista', async () => {
  const { src, s } = await opened(['manualProjects', 'taskMeta'])
  const base = { priority: 'high' as const, dueAt: 100, phases: [], currentPhase: null }
  src.hub.meta[T1] = { ...base, dueAt: 200, priority: 'urgent' }
  src.hub.log = []
  const r = await s.saveMeta(P, T1, { ...base, dueAt: 300 }, undefined, base)
  assert.deepEqual(r, { ok: false, status: 'conflict' })
  assert.ok(!src.hub.log.includes('donnaTaskMetaSave'))
  assert.equal(s.write?.status, 'conflict')
  assert.equal(s.metaOf(P, T1)?.dueAt, 200, 'a tela passa a mostrar a versão de lá')
  assert.equal((await s.retryMeta()).ok, true)
  assert.deepEqual(src.hub.meta[T1], { ...base, priority: 'urgent', dueAt: 300 }, 'prioridade do outro fica; o prazo é o do dono')
  s.close()
})

test('snapshot sem meta: edição com base não grava às cegas', async () => {
  const { src, s } = await opened(['manualProjects', 'taskMeta'])
  src.hub.snapMeta = false
  const base = { priority: null, dueAt: null, phases: [], currentPhase: null }
  const r = await s.saveMeta(P, T1, { ...base, priority: 'low' }, undefined, base)
  assert.equal(r.ok, false)
  assert.ok(!src.hub.log.includes('donnaTaskMetaSave'))
  s.close()
})

test('TEC-722-13: resposta inválida de iniciar/agendar não prende a sessão em sending; vira incerto com o mesmo pedido', async () => {
  const { src, s } = await opened(['manualProjects', 'taskStart', 'taskSchedule'])
  src.hub.startAnswer = { receipt: { clientRequestId: 'qualquer' }, start: { state: 'estado_novo' } }
  const r = await s.startTask(P, T1)
  assert.equal(r.ok, false)
  assert.equal(s.write?.status, 'uncertain', 'nem sending nem blocked')
  const first = src.bodies.filter((b) => b.method === 'donnaTaskStart').at(-1)!.clientRequestId
  src.hub.startAnswer = null
  assert.equal((await s.retry()).ok, true)
  assert.equal(src.bodies.filter((b) => b.method === 'donnaTaskStart').at(-1)!.clientRequestId, first, 'Tentar de novo reenvia o MESMO pedido')
  assert.equal(s.write, null)
  src.hub.scheduleAnswer = { receipt: { clientRequestId: 'x' }, agenda: { state: 'scheduled' } }
  await s.scheduleTask(P, T1, 99)
  const writeOf = (x: ProjectsSession) => x.write
  assert.equal(writeOf(s)?.status, 'uncertain')
  s.dismissWrite()
  await tick(5)
  assert.equal(s.canWrite, true, 'descartar libera a área')
  s.close()
})

test('jornada sem projeto: com defaultProject o Hub garante o Geral (idempotente) e a tarefa é criada nele', async () => {
  const src = new Source()
  src.hub.empty = true
  src.hub.caps = ['manualProjects', 'defaultProject']
  const s = new ProjectsSession(() => 42, ids())
  s.open(src, 'p')
  s.watchTasks(true)
  await tick(5)
  assert.equal(s.list?.projects.length, 0)
  const g = await s.ensureDefaultProject()
  assert.deepEqual(g, { ok: true, projectId: G })
  assert.equal(s.write, null)
  const again = await s.ensureDefaultProject()
  assert.deepEqual(again, { ok: true, projectId: G }, 'repetir devolve o mesmo Geral')
  assert.equal(src.hub.ensured, 1, 'criado uma vez só')
  const novo = { ...task, id: '99999999-9999-4999-8999-999999999999', title: 'Ligar para o contador' }
  const r = await s.createTask(G, (snap) => withTask(draftOf(snap), novo), novo.id, null)
  assert.equal(r.ok, true)
  assert.ok((src.hub.tasks.get(G) as { id: string }[]).some((x) => x.id === novo.id))
  await tick(300)
  assert.equal(s.list?.defaultProjectId, G)
  s.close()
})

test('jornada sem projeto e sem defaultProject: nada é criado sozinho', async () => {
  const src = new Source()
  src.hub.empty = true
  const s = new ProjectsSession(() => 42, ids())
  s.open(src, 'p')
  await tick(5)
  assert.deepEqual(await s.ensureDefaultProject(), { ok: false, status: 'blocked' })
  assert.ok(!src.hub.log.includes('donnaProjectEnsureDefault'))
  s.close()
})

const flagged = (bodies: Record<string, unknown>[], method: string) => bodies.filter((b) => b.method === method).map((b) => b.historyAuthors === true)

test('TEC-722-16: historyAuthors em toda RPC que devolve snapshot, inclusive no reenvio; fora delas, nunca', async () => {
  const { src, s } = await opened(['manualProjects', 'taskList', 'taskMeta', 'taskComments', 'taskStart', 'defaultProject'])
  s.openProject(P)
  s.openComments({ projectId: P, taskId: T1 })
  await tick(5)
  await s.saveFresh(P, (snap) => withTask(draftOf(snap), { ...snap.tasks[0]!, title: 'Roteiro 2' }))
  await s.saveMeta(P, T1, { priority: 'low', dueAt: null, phases: [], currentPhase: null })
  await s.create({ name: 'Novo', objective: '', coordinatorBotId: null })
  await s.ensureDefaultProject()
  await s.sendComment('nota', [], false, { projectId: P, taskId: T1 })
  await s.startTask(P, T1)
  for (const m of ['donnaProject', 'donnaProjectSave', 'donnaTaskMetaSave', 'donnaProjectCreate', 'donnaProjectEnsureDefault']) {
    const f = flagged(src.bodies, m)
    assert.ok(f.length > 0 && f.every(Boolean), `${m} leva historyAuthors`)
  }
  for (const m of ['donnaProjects', 'donnaTasks', 'donnaTaskComments', 'donnaTaskComment', 'donnaTaskStart']) {
    assert.ok(flagged(src.bodies, m).every((x) => !x), `${m} sem historyAuthors`)
  }
  // Reenvio de pedido incerto: mesmo clientRequestId e a mesma flag.
  src.hub.failSaveOnce = true
  await s.saveFresh(P, (snap) => withTask(draftOf(snap), { ...snap.tasks[0]!, title: 'Roteiro 3' }))
  const writeOf = (x: ProjectsSession) => x.write
  assert.equal(writeOf(s)?.status, 'uncertain')
  await s.retry()
  const saves = src.bodies.filter((b) => b.method === 'donnaProjectSave').slice(-2)
  assert.equal(saves[0]!.clientRequestId, saves[1]!.clientRequestId)
  assert.ok(saves.every((b) => b.historyAuthors === true))
  s.close()
})

test('TEC-722-16: Hub antigo recusa o campo → a mesma chamada sai sem ele e a conexão segue sem a flag', async () => {
  const src = new Source()
  src.hub.old = true
  src.hub.caps = ['manualProjects', 'taskMeta']
  const s = new ProjectsSession(() => 42, ids())
  s.open(src, 'p')
  s.watchTasks(true)
  await tick(5)
  assert.deepEqual(s.tasks?.map((b) => b.project.id), [P, Q], 'leitura funciona no Hub antigo')
  const r = await s.saveFresh(P, (snap) => withTask(draftOf(snap), { ...snap.tasks[0]!, state: 'review' }))
  assert.equal(r.ok, true)
  assert.equal(src.hub.log.filter((m) => m === 'donnaProjectSave').length, 1, 'gravou uma vez só')
  const plain = src.bodies.filter((b) => !('historyAuthors' in b) && (b.method === 'donnaProject' || b.method === 'donnaProjectSave'))
  assert.ok(plain.length >= 3)
  const tries = src.bodies.filter((b) => 'historyAuthors' in b)
  assert.equal(tries.length, 1, 'a flag só é tentada uma vez por conexão')
  s.close()
})

test('TEC-722-16: só a recusa de parse do Hub antigo dispara o reenvio sem a flag', () => {
  assert.equal(legacyHistoryRefusal(new HostErr(400, 'invalid_field: solicitação de Projetos inválida.')), true, 'erro real da base')
  assert.equal(legacyHistoryRefusal(new HostErr(400, 'invalid_field: unknown field `historyAuthors`')), true)
  assert.equal(legacyHistoryRefusal(new HostErr(400, 'invalid_field: Projeto arquivado.')), false, 'outra recusa não é de parse')
  assert.equal(legacyHistoryRefusal(new HostErr(409, 'invalid_field: solicitação de Projetos inválida.')), false)
  assert.equal(legacyHistoryRefusal(new HostErr(0, 'timeout')), false, 'queda/tempo esgotado nunca reenvia sozinho')
})

test('TEC-722-16: Hub com outra recusa (não de parse) não recebe reenvio nem perde a flag', async () => {
  const { src, s } = await opened(['manualProjects'])
  const orig = src.hub.call.bind(src.hub)
  src.hub.call = (m: string, b: Record<string, unknown>) => {
    if (m === 'donnaProjectSave') throw new HostErr(400, 'invalid_field: Projeto arquivado.')
    return orig(m, b)
  }
  src.bodies.length = 0
  const r = await s.saveFresh(P, (snap) => withTask(draftOf(snap), { ...snap.tasks[0]!, state: 'review' }))
  assert.equal(r.ok, false)
  const saves = src.bodies.filter((b) => b.method === 'donnaProjectSave')
  assert.equal(saves.length, 1, 'um envio só')
  assert.equal(saves[0]!.historyAuthors, true)
  s.close()
})

test('TEC-722-17: Concluída declarada vem do fato do Hub na Lista padrão (donnaTasks, sem histórico) e no detalhe', async () => {
  const src = new Source()
  src.hub.caps = ['manualProjects', 'taskList']
  src.hub.tasks.set(P, [{ ...task, state: 'done', evidence: 'link do corte' }])
  const decl = { [T1]: { botId: 'b1', at: 7 } }
  const orig = src.hub.call.bind(src.hub)
  src.hub.call = (m: string, b: Record<string, unknown>) => {
    const r = orig(m, b) as Record<string, any>
    if (m === 'donnaTasks' && r.page?.project.id === P) return { ...r, page: { ...r.page, declarations: decl } }
    if (m === 'donnaProject' && b.projectId === P) return { ...r, declarations: decl }
    return r
  }
  const s = new ProjectsSession(() => 42, ids())
  s.open(src, 'p')
  s.watchTasks(true)
  await tick(5)
  const lista = s.tasks!.find((x) => x.project.id === P)!
  assert.equal(lista.history, null, 'Lista padrão sem histórico')
  assert.equal(declaredBy(lista.declarations, lista.tasks[0]!), 'b1')
  s.openProject(P)
  await tick(5)
  assert.equal(declaredBy(s.project!.declarations, s.project!.tasks[0]!), 'b1')
  const q = s.tasks!.find((x) => x.project.id === Q)!
  assert.deepEqual(q.declarations, {}, 'sem o campo: nada declarado')
  s.close()
})

test('resíduo 06: recusa de menção mostra a frase do Hub sem o código', async () => {
  const { src, s } = await opened(['manualProjects', 'taskComments'])
  s.openProject(P)
  s.openComments({ projectId: P, taskId: T1 })
  await tick(5)
  src.hub.fail.donnaTaskComment = new HostErr(400, 'invalid_field: menção sem @ no texto.')
  const r = await s.sendComment('oi', ['b1'], false, { projectId: P, taskId: T1 })
  assert.equal(r.ok, false)
  const writeOf = (x: ProjectsSession) => x.write
  assert.equal(writeOf(s)?.status, 'rejected')
  assert.equal(writeOf(s)?.detail, 'menção sem @ no texto.')
  s.close()
})

test('P2-2: comentário incerto confirmado pelo Tentar de novo da faixa — um comentário só, e o campo sabe que é o dele', async () => {
  const { src, s } = await opened(['manualProjects', 'taskComments'])
  s.openProject(P)
  s.openComments({ projectId: P, taskId: T1 })
  await tick(5)
  const target = { projectId: P, taskId: T1 }
  src.hub.loseCommentAnswer = true
  const r = await s.sendComment('@Editor revisa o roteiro', ['b1'], false, target)
  assert.equal(r.ok, false)
  const id = !r.ok ? r.requestId : undefined
  assert.equal(r.ok === false && r.status, 'uncertain')
  assert.ok(id, 'o incerto devolve o pedido que ficou para Tentar de novo')
  assert.equal(s.commentConfirmed(id), false)
  assert.equal(s.canWrite, false, 'com o incerto na faixa, nenhum envio novo sai do campo')
  // Tentar de novo da faixa da página (sem onDone do campo): mesmo pedido, o Hub devolve o mesmo comentário.
  assert.equal((await s.retry()).ok, true)
  assert.equal(s.commentConfirmed(id), true)
  const sent = src.bodies.filter((b) => b.method === 'donnaTaskComment')
  assert.equal(sent.length, 2)
  assert.equal(sent[1]!.clientRequestId, sent[0]!.clientRequestId, 'mesmo pedido')
  assert.equal(src.hub.comments.length, 1, 'um comentário só no Hub (um aviso só ao bot)')
  assert.equal(s.comments?.list.length, 1)
  // O campo (Composer) decide com isso: texto igual ao enviado some; rascunho novo fica.
  assert.equal(pendingDraft('@Editor revisa o roteiro', '@Editor revisa o roteiro', s.commentConfirmed(id), false), 'clear')
  assert.equal(pendingDraft('@Editor revisa o roteiro', 'outro rascunho', s.commentConfirmed(id), false), 'keep')

  // Descartar o incerto: nada confirmado, o texto fica no campo para o dono decidir.
  src.hub.loseCommentAnswer = true
  const r2 = await s.sendComment('segundo', [], false, target)
  const id2 = !r2.ok ? r2.requestId : undefined
  assert.ok(id2)
  s.dismissWrite()
  assert.equal(s.commentConfirmed(id2), false)
  assert.equal(pendingDraft('segundo', 'segundo', s.commentConfirmed(id2), s.write?.clientRequestId === id2), 'keep')
  s.close()
})

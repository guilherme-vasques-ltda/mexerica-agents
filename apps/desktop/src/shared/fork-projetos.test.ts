import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  botLabel, draftOf, draftProblem, editState, isCanonicalUuid, keepMine, projectFields, sameProjectFields, sameTaskContent, withTask, neighborState, parentOptions, parseProjectSnapshot, parseProjectsList, parseWriteAnswer, saveBody,
  type ProjectTask,
} from './fork-projetos.ts'

// D541: a resposta só vale da origem pedida; pai sem ciclo e até 3 níveis; tarefa gravada não some; referência a bot
// fora da lista visível vira indisponível sem expor ID ou nome.
const o = { hubId: 'h1', spaceId: 'p' }
const project = { id: 'p1', name: 'Vídeo X', objective: '', coordinatorBotId: 'oculto', state: 'active', revision: 3, createdAt: 1, updatedAt: 2 }
const task = (id: string, parentTaskId: string | null = null): ProjectTask => ({ id, parentTaskId, assigneeBotId: null, title: id, description: '', acceptance: '', evidence: '', state: 'backlog' })

test('resposta de outra origem ou fora do formato não é pintada', () => {
  const ok = { version: 1, ...o, projects: [project], bots: [], capabilities: ['manualProjects'] }
  assert.equal(parseProjectsList(ok, o).capable, true)
  assert.throws(() => parseProjectsList({ ...ok, hubId: 'h2' }, o), /projects_invalid_answer/)
  assert.throws(() => parseProjectsList({ ...ok, spaceId: 'outro' }, o), /projects_invalid_answer/)
  assert.throws(() => parseProjectsList({ ...ok, projects: [{ ...project, state: 'running' }] }, o), /projects_invalid_answer/)
  const snap = { version: 1, ...o, project, tasks: [{ ...task('t1'), state: 'in_progress' }], history: [{ id: 1, at: 5, author: 'owner', action: 'task', summary: 'Movida' }], historyOmitted: 0, bots: [] }
  const parsed = parseProjectSnapshot(snap, { ...o, projectId: 'p1' })
  assert.equal(parsed.history[0]!.taskId, null, 'taskId ausente é opcional')
  assert.throws(() => parseProjectSnapshot(snap, { ...o, projectId: 'p2' }), /project.id/)
  assert.throws(() => parseProjectSnapshot({ ...snap, history: [{ id: 1, at: 5, author: 'bot', action: 'x', summary: 'y' }] }, o), /history.author/)
  assert.throws(() => parseWriteAnswer({ snapshot: snap, receipt: { clientRequestId: 'outro', revision: 4 } }, o, 'meu'), /receipt/)
})

test('pais possíveis: sem si mesma, sem descendente e sem passar de 3 níveis', () => {
  const tasks = [task('a'), task('b', 'a'), task('c', 'b'), task('d')]
  assert.deepEqual(parentOptions(tasks, 'a').map((t) => t.id), [], 'a já ocupa 3 níveis; b e c são descendentes')
  assert.deepEqual(parentOptions(tasks, 'd').map((t) => t.id), ['a', 'b'], 'c já está no 3º nível')
  assert.deepEqual(parentOptions(tasks, 'b').map((t) => t.id), ['a', 'd'])
})

test('rascunho: ciclo, tarefa sumida, título vazio e nome vazio são barrados antes do Hub', () => {
  const base = [task('a'), task('b', 'a')]
  const draft = (tasks: ProjectTask[], name = 'X') => ({ name, objective: '', coordinatorBotId: null, state: 'active' as const, tasks })
  assert.equal(draftProblem(draft(base), base), null)
  assert.equal(draftProblem(draft([task('a', 'b'), task('b', 'a')]), base), 'badParent')
  assert.equal(draftProblem(draft([task('a')]), base), 'removedTask')
  assert.equal(draftProblem(draft([...base, { ...task('c'), title: ' ' }]), base), 'titleRequired')
  assert.equal(draftProblem(draft(base, ' '), base), 'nameRequired')
  assert.equal(draftProblem(draft([task('a'), task('b', 'a'), task('c', 'b'), task('d', 'c')])), 'badParent')
})

test('gravação leva revisão esperada e só os campos do contrato', () => {
  const body = saveBody({ ...o, project: { ...project, state: 'active' } }, { name: 'X', objective: '', coordinatorBotId: null, state: 'archived', tasks: [task('a')] }, 'req')
  assert.equal(body.expectedRevision, 3)
  assert.deepEqual(Object.keys(body).sort(), ['clientRequestId', 'coordinatorBotId', 'expectedRevision', 'hubId', 'name', 'objective', 'projectId', 'spaceId', 'state', 'tasks'])
  assert.deepEqual(Object.keys(body.tasks[0]!).sort(), ['acceptance', 'assigneeBotId', 'description', 'evidence', 'id', 'parentTaskId', 'state', 'title'])
})

test('bot fora da lista visível: indisponível, sem nome nem ID', () => {
  assert.deepEqual(botLabel([{ id: 'b1', name: 'Editor' }], 'oculto'), { kind: 'unavailable' })
  assert.deepEqual(botLabel([{ id: 'b1', name: 'Editor' }], 'b1'), { kind: 'bot', name: 'Editor' })
  assert.deepEqual(botLabel([], null), { kind: 'none' })
})

test('teclado e IDs', () => {
  assert.equal(neighborState('backlog', -1), null)
  assert.equal(neighborState('backlog', 1), 'in_progress')
  assert.equal(neighborState('cancelled', 1), null)
  assert.equal(isCanonicalUuid('11111111-1111-4111-8111-111111111111'), true)
  assert.equal(isCanonicalUuid('11111111-1111-4111-8111-11111111111A'), false)
})

test('edição sobre a base: conflito só quando a MESMA tarefa mudou lá e aqui; outra tarefa mudada não bloqueia', () => {
  const base = { ...task('t1'), title: 'Roteiro' }
  const mine = { ...base, title: 'Roteiro v2' }
  assert.equal(editState(base, mine, base, sameTaskContent), 'clean', 'ninguém mais mexeu')
  assert.equal(editState(base, mine, { ...base, evidence: 'link' }, sameTaskContent), 'conflict', 'mudou lá e aqui')
  assert.equal(editState(base, base, { ...base, evidence: 'link' }, sameTaskContent), 'follow', 'sem mudança aqui, acompanha')
  assert.equal(editState(base, mine, { ...mine }, sameTaskContent), 'clean', 'o atual já é o meu (pedido incerto que chegou)')
  assert.equal(editState(base, mine, undefined, sameTaskContent), 'gone')
  for (const k of ['parentTaskId', 'assigneeBotId', 'title', 'description', 'acceptance', 'evidence', 'state'] as const) {
    assert.equal(sameTaskContent(base, { ...base, [k]: k === 'state' ? 'done' : 'outro' }), false, k)
  }
  // Outra tarefa mudada em outro aparelho: a tarefa aberta segue igual à base, e o rascunho montado sobre o atual a mantém.
  const now = { version: 1, ...o, project, tasks: [base, { ...task('t2'), title: 'Mudou lá' }], history: [], historyOmitted: 0, bots: [] }
  const snapNow = parseProjectSnapshot(now, o)
  assert.equal(editState(base, mine, snapNow.tasks.find((x) => x.id === 't1'), sameTaskContent), 'clean')
  assert.equal(withTask(draftOf(snapNow), mine).tasks.find((x) => x.id === 't2')?.title, 'Mudou lá')
})

test('campos do Projeto: nome, objetivo, coordenador e estado contam; tarefas não', () => {
  const base = projectFields({ ...project, state: 'active' as const })
  assert.equal(sameProjectFields(base, { ...base }), true)
  for (const k of ['name', 'objective', 'coordinatorBotId', 'state'] as const) {
    assert.equal(sameProjectFields(base, { ...base, [k]: k === 'state' ? 'archived' : 'x' }), false, k)
  }
  assert.equal(editState(base, { ...base, name: 'Meu' }, { ...base, state: 'archived' }, sameProjectFields), 'conflict')
})

test('Manter a minha: fica só o campo que o dono mudou; o resto vem da versão atual', () => {
  const base = { ...task('t1'), title: 'Roteiro', acceptance: 'velho' }
  const mine = { ...base, description: 'meu texto' }
  const now = { ...base, acceptance: 'mudou lá', description: 'lá também' }
  assert.deepEqual(keepMine(base, mine, now), { ...now, description: 'meu texto' })
})

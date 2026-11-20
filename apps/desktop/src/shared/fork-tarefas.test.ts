import { test } from 'node:test'
import assert from 'node:assert/strict'
import { parseProject, parseProjectSnapshot, parseProjectsList, parseTask, type ProjectSummary, type ProjectTask } from './fork-projetos.ts'
import {
  EMPTY_META, boardColumns, declaredBy, mergeMeta, entriesOf, listGroups, mentionQuery, mentionsIn, metaBody, metaProblem, NO_FILTER, parseComments,
  followCurrentPhase, joinNames, parseCommentAnswer, parseSessions, parseTaskMeta, parseTasksPage, pendingDraft, phasesFromText, pickProject, progressOf, fromLocalInput, sheetMeta, syncSheetMeta, toLocalInput,
  type TaskBundle,
} from './fork-tarefas.ts'

// MX-142: regras puras da área Tarefas. Capacidade só com anúncio; resposta de outra origem ou fora do formato não
// pinta; status de aviso vem do Hub e estado desconhecido nunca vira “avisado”.
const o = { hubId: 'h1', spaceId: 'p' }
const P = '11111111-1111-4111-8111-111111111111'
const Q = '22222222-2222-4222-8222-222222222222'
const project = (id: string, name: string, state = 'active'): ProjectSummary => ({ id, name, objective: '', coordinatorBotId: null, state: state as 'active', revision: 3, createdAt: 1, updatedAt: 2 })
const task = (id: string, extra: Partial<ProjectTask> = {}): ProjectTask => ({ id, parentTaskId: null, assigneeBotId: null, title: id, description: '', acceptance: '', evidence: '', state: 'backlog', ...extra })
const bundle = (p: ProjectSummary, tasks: ProjectTask[], meta: TaskBundle['meta'] = {}): TaskBundle => ({ project: p, tasks, meta, history: null, declarations: {} })

test('atenção na ficha só com fato tipado da mesma tarefa, projeto, Space e Hub', () => {
  const where = { ...o, projectId: P, taskId: 't1' }
  const base = { version: 1, ...where, sessions: [] }
  assert.equal(parseSessions({ ...base, attention: 'owner' }, where).attention, 'owner')
  for (const attention of [undefined, null, true, 'coordinator', 'unknown']) {
    assert.equal(parseSessions({ ...base, attention }, where).attention, null)
  }
  for (const key of ['hubId', 'spaceId', 'projectId', 'taskId'] as const) {
    assert.throws(() => parseSessions({ ...base, [key]: 'outro', attention: 'owner' }, where), new RegExp(key))
  }
  assert.throws(() => parseSessions({ ...base, version: 2, attention: 'owner' }, where), /version/)
})

test('capacidades e projeto padrão: só com anúncio, e só se o Projeto está na lista', () => {
  const base = { version: 1, ...o, projects: [project(P, 'Geral')], bots: [] }
  const sem = parseProjectsList({ ...base, capabilities: ['manualProjects'], defaultProjectId: P }, o)
  assert.deepEqual(sem.capabilities, ['manualProjects'])
  assert.equal(sem.defaultProjectId, null, 'sem defaultProject anunciado, o campo é ignorado')
  const com = parseProjectsList({ ...base, capabilities: ['manualProjects', 'defaultProject', 'taskMeta'], defaultProjectId: P }, o)
  assert.equal(com.defaultProjectId, P)
  assert.equal(parseProjectsList({ ...base, capabilities: ['manualProjects', 'defaultProject'], defaultProjectId: Q }, o).defaultProjectId, null, 'padrão fora da lista não vale')
})

test('metadados: formato estrito, sem startAt inventado', () => {
  assert.deepEqual(parseTaskMeta({ priority: 'high', dueAt: 5, phases: ['A', 'B'], currentPhase: 1 }), { priority: 'high', dueAt: 5, phases: ['A', 'B'], currentPhase: 1 })
  assert.deepEqual(parseTaskMeta({}), EMPTY_META)
  assert.throws(() => parseTaskMeta({ priority: 'máxima' }), /meta.priority/)
  assert.throws(() => parseTaskMeta({ phases: ['A'], currentPhase: 1 }), /meta.currentPhase/)
  const s = { hubId: 'h1', spaceId: 'p', project: project(P, 'X') }
  const body = metaBody(s, 't1', { priority: 'low', dueAt: null, phases: ['A'], currentPhase: 0 }, 'req')
  assert.deepEqual(Object.keys(body.meta).sort(), ['currentPhase', 'dueAt', 'phases', 'priority'])
  assert.equal(body.expectedRevision, 3)
})

test('fases: uma por linha, só na tarefa principal, com limites', () => {
  assert.deepEqual(phasesFromText(' Pesquisa \n\nRoteiro\n', 3), { phases: ['Pesquisa', 'Roteiro'], currentPhase: null })
  assert.equal(metaProblem({ ...EMPTY_META, phases: ['A'] }, { parentTaskId: 'x' }), 'phasesOnlyRoot')
  assert.equal(metaProblem({ ...EMPTY_META, phases: Array.from({ length: 21 }, (_, i) => `F${i}`) }, { parentTaskId: null }), 'tooManyPhases')
  assert.equal(metaProblem({ ...EMPTY_META, dueAt: -1 }, { parentTaskId: null }), 'badDue')
  assert.equal(metaProblem({ ...EMPTY_META, phases: ['A'] }, { parentTaskId: null }), null)
})

test('donnaTasks: página da mesma origem, metadados por tarefa, próximo cursor', () => {
  const page = { version: 1, ...o, page: { project: project(P, 'X'), tasks: [task('t1')], meta: { t1: { priority: 'urgent', dueAt: null, phases: [], currentPhase: null } } }, next: Q }
  const r = parseTasksPage(page, o, { project: parseProject, task: parseTask })
  assert.equal(r.next, Q)
  assert.equal(r.bundle?.meta.t1?.priority, 'urgent')
  assert.equal(parseTasksPage({ version: 1, ...o, page: null, next: null }, o, { project: parseProject, task: parseTask }).bundle, null)
  assert.throws(() => parseTasksPage({ ...page, spaceId: 'outro' }, o, { project: parseProject, task: parseTask }), /spaceId/)
})

test('comentários: origem exata, autor e aviso honesto', () => {
  const where = { ...o, projectId: P, taskId: 't1' }
  const answer = {
    version: 1, ...where, omitted: 0, comments: [
      { id: 'c1', at: 1, author: 'owner', text: 'oi', mentions: [], onlyNote: false, notify: { state: 'not_notified', botIds: [], reason: 'no_mention' } },
      { id: 'c2', at: 2, author: 'bot:b1', text: 'feito', mentions: [], notify: null },
      { id: 'c3', at: 3, author: 'owner', text: '@Editor', mentions: ['b1'], notify: { state: 'entregue_em_marte', botIds: ['b1'] } },
    ],
  }
  const { comments } = parseComments(answer, where)
  assert.deepEqual(comments[1]!.author, { kind: 'bot', botId: 'b1' })
  assert.equal(comments[0]!.notify?.reason, 'no_mention')
  assert.equal(comments[2]!.notify?.state, 'uncertain', 'estado desconhecido nunca vira avisado')
  assert.throws(() => parseComments({ ...answer, taskId: 't2' }, where), /taskId/)
  assert.throws(() => parseComments({ ...answer, comments: [{ ...answer.comments[0], author: 'bot:' }] }, where), /author/)
  assert.throws(() => parseCommentAnswer({ comment: answer.comments[0], receipt: { clientRequestId: 'outro' } }, 'req'), /receipt/)
})

test('menções: nome inteiro do catálogo do Space, o mais longo primeiro', () => {
  const bots = [{ id: 'a', name: 'Ana' }, { id: 'ac', name: 'Ana Clara' }, { id: 'e', name: 'Editor' }]
  assert.deepEqual(mentionsIn('@Ana Clara e @editor, veja', bots), ['ac', 'e'])
  assert.deepEqual(mentionsIn('email@Ana.com @Anabela', bots), [], 'sem @ no meio de palavra nem prefixo de outro nome')
  assert.deepEqual(mentionQuery('oi @Ed', 6), { start: 3, query: 'Ed' })
  assert.equal(mentionQuery('a@b', 3), null)
  // Mesma regra do Hub (fork/mentions.rs): '_' antes do @ conta como palavra; nome com espaço; maiúsculas indiferentes.
  assert.deepEqual(mentionsIn('foo_@Ana', bots), [], "'_@' não é menção")
  assert.deepEqual(mentionsIn('(@ANA CLARA)', bots), ['ac'])
  assert.deepEqual(mentionsIn('@Ana_x', bots), [], 'fronteira final sem letra/dígito/_')
  assert.equal(mentionQuery('oi @Ed', -1), null, 'Esc fecha o menu até a próxima digitação')
})

test('lista: árvore por estado da principal, “Esperando você” como atalho, filtro achata', () => {
  const p = project(P, 'Canal')
  const q = project(Q, 'Loja')
  const entries = entriesOf([
    bundle(p, [task('root', { state: 'in_progress' }), task('kid', { parentTaskId: 'root', state: 'review', assigneeBotId: 'b1' }), task('neto', { parentTaskId: 'kid', state: 'done' })], { root: { ...EMPTY_META, priority: 'high' } }),
    bundle(q, [task('solta', { state: 'in_progress', title: 'Fotos novas' })]),
  ])
  assert.deepEqual(entries.map((e) => [e.task.id, e.depth]), [['root', 0], ['kid', 1], ['neto', 2], ['solta', 0]])
  const { waiting, groups } = listGroups(entries, NO_FILTER)
  assert.deepEqual(waiting.map((e) => e.task.id), ['kid'])
  const andamento = groups.find((g) => g.state === 'in_progress')!
  assert.deepEqual(andamento.rows.map((e) => e.task.id), ['root', 'kid', 'neto', 'solta'], 'prioridade alta primeiro, filhos logo abaixo')
  assert.equal(andamento.count, 2)
  const f = listGroups(entries, { ...NO_FILTER, botId: 'b1' })
  assert.deepEqual(f.groups.find((g) => g.state === 'review')!.rows.map((e) => [e.task.id, e.depth]), [['kid', 0]])
  assert.deepEqual(boardColumns(entries, { ...NO_FILTER, query: 'fotos' }).find((c) => c.state === 'in_progress')!.cards.map((e) => e.task.id), ['solta'])
  assert.deepEqual(progressOf([task('r'), task('a', { parentTaskId: 'r', state: 'done' }), task('b', { parentTaskId: 'r', state: 'cancelled' }), task('c', { parentTaskId: 'a' })], 'r'), { done: 1, total: 2 })
})

test('pai ausente vira raiz e ciclo do Hub não some nem repete', () => {
  const entries = entriesOf([bundle(project(P, 'X'), [task('a', { parentTaskId: 'sumiu' }), task('x', { parentTaskId: 'y' }), task('y', { parentTaskId: 'x' })])])
  assert.deepEqual(entries.map((e) => e.task.id).sort(), ['a', 'x', 'y'])
})

test('projeto da Nova tarefa: último usado → padrão do Hub → primeiro ativo', () => {
  const ps = [project(P, 'A'), project(Q, 'Geral'), project('33333333-3333-4333-8333-333333333333', 'Arq', 'archived')]
  assert.equal(pickProject(ps, Q, null), Q)
  assert.equal(pickProject(ps, '33333333-3333-4333-8333-333333333333', Q), Q, 'arquivado não vale como último')
  assert.equal(pickProject(ps, null, null), P)
  assert.equal(pickProject([], null, null), null)
})

test('data/hora local ida e volta', () => {
  const ms = new Date(2026, 10, 14, 16, 30).getTime()
  assert.equal(fromLocalInput(toLocalInput(ms)), ms)
  assert.equal(fromLocalInput(''), null)
  assert.equal(toLocalInput(null), '')
})

test('meta parcial: o campo do dono vale, o resto vem da versão relida; conflito só no mesmo campo', () => {
  const base = { priority: 'high' as const, dueAt: 100, phases: ['A', 'B'], currentPhase: 0 }
  const lá = { ...base, dueAt: 200 } // outro cliente mudou o prazo
  assert.deepEqual(mergeMeta(base, { ...base, priority: 'low' }, lá), { meta: { ...lá, priority: 'low' }, conflict: false })
  assert.equal(mergeMeta(base, { ...base, dueAt: 300 }, lá).conflict, true)
  assert.equal(mergeMeta(base, { ...base, dueAt: 200 }, lá).conflict, false, 'mesma mudança dos dois lados não é conflito')
  assert.deepEqual(mergeMeta(base, { ...base, phases: ['A', 'B', 'C'] }, { ...base, currentPhase: 1 }).conflict, true, 'fases e fase atual andam juntas')
})

test('TEC-722-17: Concluída declarada só pelo fato do Hub (declarations), nunca pelo histórico', () => {
  const decl = { t: { botId: 'b1', at: 5 } }
  assert.equal(declaredBy(decl, { id: 't', state: 'done' }), 'b1')
  assert.equal(declaredBy(decl, { id: 't', state: 'review' }), null, 'saiu de Concluída: não está declarada')
  assert.equal(declaredBy({}, { id: 't', state: 'done' }), null, 'sem fato, Concluída simples (não infere conferência humana nem declaração)')
  assert.equal(declaredBy(undefined, { id: 't', state: 'done' }), null, 'Hub sem o campo')
  // Lista padrão (donnaTasks, sem histórico): o fato vem na página.
  const page = { version: 1, ...o, page: { project: project(P, 'X'), tasks: [task('t', { state: 'done' })], meta: {}, declarations: decl }, next: null }
  const { bundle } = parseTasksPage(page, o, { project: parseProject, task: parseTask })
  assert.equal(bundle!.history, null)
  assert.equal(declaredBy(bundle!.declarations, bundle!.tasks[0]!), 'b1')
  // Detalhe (donnaProject): o fato vem no snapshot.
  const snap = parseProjectSnapshot({ version: 1, ...o, project: project(P, 'X'), tasks: [task('t', { state: 'done' })], history: [], historyOmitted: 0, bots: [], declarations: decl }, o)
  assert.equal(declaredBy(snap.declarations, snap.tasks[0]!), 'b1')
  assert.throws(() => parseProjectSnapshot({ version: 1, ...o, project: project(P, 'X'), tasks: [], history: [], bots: [], declarations: { t: { at: 1 } } }, o), /botId/)
})

test('TEC-722-14: agenda failed é terminal e nenhum idioma promete nova tentativa', async () => {
  const promete = /tentando de novo|retrying|reintentando|; nouvelle tentative$|正在重试|جارٍ إعادة المحاولة/
  for (const lang of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE']) {
    const { textos } = await import(`../renderer/lib/fork-i18n/${lang}/projetos.ts`) as { textos: Record<string, string> }
    assert.ok(!promete.test(textos['projetos.tarefas.agenda.failed']!), lang)
    assert.ok(!('projetos.tarefas.regraBots' in textos), `${lang}: regra antiga “bots não mudam tarefas” saiu`)
  }
})

test('TEC-722-15: várias menções — [answered, failed] nunca vira “ninguém avisado”', () => {
  const where = { hubId: 'h1', spaceId: 'p', projectId: P, taskId: 't1' }
  const c = (notify: unknown) => parseComments({ version: 1, ...where, omitted: 0, comments: [{ id: 'c', at: 1, author: 'owner', text: '@A @B', mentions: ['a', 'b'], notify }] }, where).comments[0]!.notify!
  const contraditorio = c({ state: 'not_notified', botIds: ['a', 'b'], reason: 'delivery_failed', recipients: [{ botId: 'a', state: 'answered' }, { botId: 'b', state: 'failed', reason: 'delivery_failed' }] })
  assert.equal(contraditorio.state, 'partial')
  assert.deepEqual(contraditorio.recipients.map((r) => r.state), ['answered', 'failed'])
  assert.equal(c({ state: 'partial', botIds: ['a'], recipients: [{ botId: 'a', state: 'nova_coisa' }] }).recipients[0]!.state, 'unknown')
  assert.equal(c({ state: 'not_notified', botIds: [], reason: 'no_mention' }).state, 'not_notified', 'sem destinatário que recebeu, continua ninguém')
  assert.deepEqual(c({ state: 'responded', botIds: ['a'] }).recipients, [], 'campo novo é opcional')
})

test('P2-1: fases digitadas na folha sobrevivem a uma releitura que muda outro metadado', () => {
  const base = { priority: 'normal' as const, dueAt: 100, phases: ['Pesquisa', 'Roteiro'], currentPhase: 0 }
  const meta = { ...base }
  const typed = 'Pesquisa\nRoteiro\nGravação'
  // Outro aparelho muda a prioridade enquanto o dono digita a 3ª fase (ainda só no texto, fora de `meta`).
  const now = { ...base, priority: 'urgent' as const }
  const r = syncSheetMeta(base, meta, typed, now)
  assert.equal(r.kind, 'rebase')
  assert.equal(r.phasesText, typed, 'o texto digitado fica')
  assert.equal(r.meta.priority, 'urgent', 'o campo que o dono não tocou segue a versão nova')
  // Depois do rebase (base = now), a folha fica estável e o envio leva as fases digitadas com a prioridade nova.
  assert.equal(syncSheetMeta(now, r.meta, r.phasesText, now).kind, 'clean')
  assert.deepEqual(sheetMeta(r.meta, r.phasesText), { priority: 'urgent', dueAt: 100, phases: ['Pesquisa', 'Roteiro', 'Gravação'], currentPhase: 0 })

  // Linha em branco do meio da digitação (Enter antes de escrever a fase) também fica.
  const mid = 'Pesquisa\nRoteiro\n'
  const r2 = syncSheetMeta(base, meta, mid, now)
  assert.equal(r2.kind === 'rebase' && r2.phasesText, mid)

  // O dono não mexeu nas fases e a versão nova trouxe outras: o texto segue a versão nova.
  const r3 = syncSheetMeta(base, { ...meta, priority: 'low' }, 'Pesquisa\nRoteiro', { ...base, phases: ['Pesquisa', 'Roteiro', 'Edição'] })
  assert.equal(r3.kind, 'rebase')
  assert.equal(r3.kind === 'rebase' && r3.phasesText, 'Pesquisa\nRoteiro\nEdição')
  assert.equal(r3.kind === 'rebase' && r3.meta.priority, 'low', 'e a prioridade do dono fica')

  // Bot avança a fase atual enquanto o dono reescreve as fases: conflito no mesmo grupo, nada é sobrescrito sozinho.
  const r4 = syncSheetMeta(base, meta, typed, { ...base, currentPhase: 1 })
  assert.equal(r4.kind, 'conflict')
  assert.equal(r4.kind === 'conflict' && r4.phasesText, typed, '“Manter a minha” guarda o texto')

  assert.equal(syncSheetMeta(base, meta, typed, base).kind, 'clean', 'sem mudança no Hub, nada a fazer')
})

test('P2-2: rascunho de comentário incerto só sai do campo com a confirmação DESSE pedido', () => {
  assert.equal(pendingDraft('oi @Editor', 'oi @Editor', false, true), 'wait', 'incerto ou reenviando: espera')
  assert.equal(pendingDraft('oi @Editor', '  oi @Editor ', true, false), 'clear', 'confirmado e o campo ainda tem o enviado')
  assert.equal(pendingDraft('oi @Editor', 'outra coisa', true, false), 'keep', 'confirmado, mas o dono escreveu outro rascunho')
  assert.equal(pendingDraft('oi @Editor', 'oi @Editor', false, false), 'keep', 'descartado ou recusado: o texto fica')
})

test('P3: fase atual segue o nome da fase, não a posição', () => {
  const antes = ['Pesquisa', 'Roteiro', 'Gravação']
  assert.equal(followCurrentPhase(antes, 1, ['Roteiro', 'Gravação']), 0, 'apagar uma fase anterior não passa a atual para a próxima')
  assert.equal(followCurrentPhase(antes, 1, ['Briefing', 'Pesquisa', 'Roteiro', 'Gravação']), 2, 'inserir antes também não')
  assert.equal(followCurrentPhase(antes, 1, ['Pesquisa', 'Roteiro final', 'Gravação']), 1, 'renomeada no lugar: mesma posição')
  assert.equal(followCurrentPhase(antes, 1, ['Pesquisa', 'Gravação']), null, 'a atual sumiu: sem fase atual, nada escolhido sozinho')
  // Nome repetido: a atual era o 2º “A”, no fim inalterado; o 1º “A” saiu e “X” entrou antes → continua o último “A”.
  assert.equal(followCurrentPhase(['A', 'B', 'A'], 2, ['B', 'A', 'X', 'A']), 3, 'linha no sufixo inalterado fica com ela')
  assert.equal(followCurrentPhase(['A', 'B'], 1, ['A', 'B', 'B']), 1, 'linha no prefixo inalterado fica; a cópia nova não a rouba')
  // Sufixo inalterado [A, Z]: a atual (o A) é a mesma linha, contada do fim (regra 1), mesmo com outro A no começo.
  assert.equal(followCurrentPhase(['K', 'A', 'Z'], 1, ['A', 'M', 'A', 'Z']), 2, 'linha no sufixo inalterado, mesmo com nome repetido antes')
  // Sem prefixo nem sufixo em comum (K≠A, W≠A): a atual está no trecho editado; dois A à distância 1 → empate → a mais cedo.
  assert.equal(followCurrentPhase(['K', 'A', 'W'], 1, ['A', 'M', 'A']), 0, 'empate genuíno no trecho editado: a mais cedo')
  // Sem prefixo nem sufixo em comum (K≠M, W≠Y): o mesmo nome achado dentro do trecho editado.
  assert.equal(followCurrentPhase(['K', 'A', 'W'], 1, ['M', 'N', 'A', 'Y']), 2, 'no trecho editado: o mesmo nome')
  assert.equal(followCurrentPhase(antes, null, ['X']), null)
})

test('P3: nomes juntados pelo idioma, cada um isolado (bidi)', () => {
  const nomes = ['Ana', 'Bia', 'Caio']
  assert.equal(joinNames('pt-BR', nomes), '\u2068Ana\u2069, \u2068Bia\u2069 e \u2068Caio\u2069')
  assert.equal(joinNames('en', ['Ana', 'Bia']), '\u2068Ana\u2069 and \u2068Bia\u2069')
  assert.ok(!joinNames('zh-CN', nomes).includes(', '), 'zh-CN não usa a vírgula latina')
  assert.ok(!joinNames('ar-AE', nomes).includes(', '), 'ar-AE não usa a vírgula latina')
  assert.equal(joinNames('pt-BR', ['Só']), '\u2068Só\u2069')
})

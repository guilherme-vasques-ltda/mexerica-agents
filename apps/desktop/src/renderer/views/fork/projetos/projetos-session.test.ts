import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { TowerEvent, TowerLink } from '../torre/torre-session.ts'
import { ProjectsSession } from './projetos-session.ts'

// D541: assina antes de ler, invalida só pelo aviso do mesmo Hub/space, só leitura sem link ou antes da leitura nova,
// 403 e outra época apagam tudo, falha incerta guarda o MESMO pedido e resposta antiga nunca ressuscita nada.
const P = '11111111-1111-4111-8111-111111111111'
const project = (rev: number) => ({ id: P, name: 'Vídeo X', objective: '', coordinatorBotId: null, state: 'active', revision: rev, createdAt: 1, updatedAt: rev })
const list = (hubId: string, capabilities = ['manualProjects'], spaceId = 'p') => ({ version: 1, hubId, spaceId, projects: [project(1)], bots: [{ id: 'b1', name: 'Editor' }], capabilities })
const snap = (hubId: string, rev: number, spaceId = 'p') => ({ version: 1, hubId, spaceId, project: project(rev), tasks: [], history: [], historyOmitted: 0, bots: [] })
class HostErr extends Error { status: number; constructor(status: number, m = 'x') { super(m); this.status = status } }

type Handler = (method: string, body: Record<string, unknown>) => Promise<unknown>

class Source {
  hello: { hostId: string; rev: number } | null = { hostId: 'h1', rev: 1 }
  principalEpoch = 1
  log: string[] = []
  emit: ((e: TowerEvent) => void) | null = null
  /** O ouvinte de cada space (o de A continua aqui depois da troca, para provar que não mexe em B). */
  emits = new Map<string, (e: TowerEvent) => void>()
  rev = 1
  handler: Handler | null = null
  connectedClient: TowerLink | null = this.link()
  private listeners = new Set<() => void>()
  link(): TowerLink {
    return {
      call: async (method, body) => {
        const b = body as Record<string, unknown>
        this.log.push(`${method} ${JSON.stringify(b)}`)
        if (this.handler) return this.handler(method, b)
        if (method === 'donnaProjects') return list(this.hello!.hostId)
        return snap(this.hello!.hostId, this.rev)
      },
      inSpace: (spaceId) => ({ events: (_since, _client, onEvent) => { this.log.push(`listen ${spaceId}`); this.emit = onEvent; this.emits.set(spaceId, onEvent); return () => {} } }),
    }
  }
  subscribe(l: () => void) { this.listeners.add(l); return () => { this.listeners.delete(l) } }
  notify() { for (const l of this.listeners) l() }
}
const tick = (ms = 0) => new Promise((r) => setTimeout(r, ms))
const ids = () => { let n = 0; return () => `00000000-0000-4000-8000-${String(++n).padStart(12, '0')}` }
const draft = { name: 'Vídeo X', objective: '', coordinatorBotId: null, state: 'active' as const, tasks: [] }

async function opened() {
  const src = new Source()
  const s = new ProjectsSession(() => 42, ids())
  s.open(src, 'p')
  await tick()
  s.openProject(P)
  await tick()
  return { src, s }
}

test('assina antes de ler; aviso de outro space/Hub não relê; seguidos viram uma leitura', async () => {
  const src = new Source()
  const s = new ProjectsSession(() => 42, ids())
  s.open(src, 'p')
  assert.deepEqual(src.log, ['listen p', 'donnaProjects {"hubId":"h1","spaceId":"p"}'])
  await tick()
  assert.equal(s.canWrite, true)
  src.log = []
  src.emit!({ type: 'donnaProjectsChanged', spaceId: 'outro' })
  src.emit!({ type: 'donnaProjectsChanged', spaceId: 'p', hubId: 'h9' } as TowerEvent)
  src.emit!({ type: 'donnaProjectsChanged', spaceId: 'p' })
  src.emit!({ type: 'donnaProjectsChanged', spaceId: 'p' })
  await tick(300)
  assert.equal(src.log.length, 1)
  s.close()
})

test('Hub sem o anúncio ou sem o método: indisponível, sem alternativa', async () => {
  const src = new Source()
  src.handler = async () => list('h1', [])
  const s = new ProjectsSession(() => 42, ids())
  s.open(src, 'p')
  await tick()
  assert.deepEqual(s.phase, { kind: 'error', reason: 'unavailable' })
  src.handler = async () => { throw new HostErr(404, 'unknown method') }
  s.reread()
  await tick()
  assert.deepEqual(s.phase, { kind: 'error', reason: 'unavailable' })
  assert.equal(s.canWrite, false)
  s.close()
})

test('sem link: fotografia antiga e só leitura; link novo continua só leitura até a leitura nova', async () => {
  const { src, s } = await opened()
  src.connectedClient = null
  src.notify()
  assert.equal(s.stale, true)
  assert.equal(s.project?.project.revision, 1)
  assert.equal(s.canWrite, false)
  let release!: () => void
  src.handler = (m) => new Promise((r) => { release = () => r(m === 'donnaProjects' ? list('h1') : snap('h1', 2)) })
  src.connectedClient = src.link()
  src.notify()
  assert.equal(s.canWrite, false, 'antes da leitura autorizada da conexão nova, nada muda')
  release()
  await tick()
  release()
  await tick()
  assert.equal(s.canWrite, true)
  assert.equal(s.project?.project.revision, 2)
  s.close()
})

test('falha incerta guarda o mesmo pedido; Tentar de novo reenvia chave e corpo iguais', async () => {
  const { src, s } = await opened()
  src.handler = async (m) => { if (m === 'donnaProjectSave') throw new HostErr(0); return m === 'donnaProjects' ? list('h1') : snap('h1', 1) }
  const r = await s.save(draft, s.project!)
  // O incerto nomeia o pedido guardado para Tentar de novo (o campo de comentário liga a confirmação a ele).
  assert.deepEqual(r, { ok: false, status: 'uncertain', requestId: s.write?.clientRequestId })
  assert.equal(s.write?.clientRequestId, '00000000-0000-4000-8000-000000000001')
  assert.equal(s.canWrite, false, 'incerto bloqueia outra gravação')
  const first = src.log.filter((l) => l.startsWith('donnaProjectSave')).at(-1)
  src.handler = async (m, b) => m === 'donnaProjectSave' ? { snapshot: snap('h1', 2), receipt: { clientRequestId: b.clientRequestId, revision: 2 } } : m === 'donnaProjects' ? list('h1') : snap('h1', 2)
  const again = await s.retry()
  assert.equal(again.ok, true)
  const second = src.log.filter((l) => l.startsWith('donnaProjectSave')).at(-1)
  assert.equal(second, first)
  assert.equal(s.write, null)
  assert.equal(s.project?.project.revision, 2, 'a tela só muda com a resposta do Hub')
  s.close()
})

test('conflito relê e não reenvia; recusa guarda a frase do Hub sem o código', async () => {
  const { src, s } = await opened()
  src.rev = 5
  src.handler = async (m) => { if (m === 'donnaProjectSave') throw new HostErr(409, 'projects_conflict: revisão antiga'); return m === 'donnaProjects' ? list('h1') : snap('h1', src.rev) }
  assert.deepEqual(await s.save(draft, s.project!), { ok: false, status: 'conflict' })
  await tick()
  assert.equal(s.project?.project.revision, 5)
  assert.equal(src.log.filter((l) => l.startsWith('donnaProjectSave')).length, 1)
  src.handler = async (m) => { if (m === 'donnaProjectSave') throw new HostErr(400, 'projects_limit: texto longo demais'); return snap('h1', 5) }
  await s.save(draft, s.project!)
  assert.equal(s.write?.status, 'rejected')
  assert.equal(s.write?.detail, 'texto longo demais')
  s.close()
})

test('403 apaga tudo; resposta antiga depois não ressuscita pedido nem dados', async () => {
  const { src, s } = await opened()
  let answer!: (v: unknown) => void
  src.handler = (m) => m === 'donnaProjectSave' ? new Promise((r) => { answer = r }) : Promise.reject(new HostErr(403))
  const pending = s.save(draft, s.project!)
  s.reread()
  await tick()
  assert.deepEqual(s.phase, { kind: 'error', reason: 'lostAccess' })
  assert.equal(s.list, null)
  assert.equal(s.write, null)
  answer({ snapshot: snap('h1', 9), receipt: { clientRequestId: '00000000-0000-4000-8000-000000000001', revision: 9 } })
  assert.deepEqual(await pending, { ok: false, status: 'blocked' })
  assert.equal(s.write, null)
  assert.equal(s.project, null)
  s.close()
})

test('outra época do MESMO Hub (hello da reconexão) não é troca de dono: fotografia fica antiga; pedido em voo vira incerto', async () => {
  const { src, s } = await opened()
  let fail!: (e: unknown) => void
  src.handler = (m) => m === 'donnaProjectSave' ? new Promise((_r, j) => { fail = j }) : new Promise(() => {})
  const pending = s.save(draft, s.project!)
  src.principalEpoch = 2
  src.connectedClient = src.link()
  src.notify()
  assert.equal(s.project?.project.revision, 1, 'a fotografia fica')
  assert.equal(s.list?.projects.length, 1)
  assert.equal(s.canWrite, false, 'só leitura até a leitura autorizada da época nova')
  fail(new HostErr(0))
  assert.deepEqual(await pending, { ok: false, status: 'uncertain', requestId: '00000000-0000-4000-8000-000000000001' })
  assert.equal(s.write?.status, 'uncertain', 'o mesmo pedido fica para Tentar de novo')
  assert.equal(s.project?.project.revision, 1, 'erro da época anterior não pinta')
  s.close()
})

test('outra fonte (outro BotStore) ou outro Hub apagam tudo', async () => {
  const { s } = await opened()
  s.open(new Source(), 'p')
  assert.equal(s.project, null)
  assert.equal(s.list, null)
  const other = await opened()
  other.src.hello = { hostId: 'h2', rev: 1 }
  other.src.notify()
  assert.deepEqual(other.s.phase, { kind: 'moved' })
  assert.equal(other.s.project, null)
  s.close(); other.s.close()
})

test('mesma autoridade com link trocado: a resposta não pinta e o pedido fica para Tentar de novo', async () => {
  const { src, s } = await opened()
  let answer!: (v: unknown) => void
  src.handler = (m) => m === 'donnaProjectSave' ? new Promise((r) => { answer = r }) : new Promise(() => {})
  const pending = s.save(draft, s.project!)
  src.connectedClient = src.link()
  src.notify()
  answer({ snapshot: snap('h1', 3), receipt: { clientRequestId: '00000000-0000-4000-8000-000000000001', revision: 3 } })
  assert.deepEqual(await pending, { ok: false, status: 'uncertain', requestId: '00000000-0000-4000-8000-000000000001' })
  assert.equal(s.project?.project.revision, 1)
  assert.equal(s.write?.status, 'uncertain')
  s.close()
  assert.equal(s.write, null, 'fechar abandona o pedido')
})

// D543: cada Space tem os seus Projetos; trocar de Space apaga tudo e nada do anterior volta a pintar.
const E = '22222222-2222-4222-8222-222222222222'
// Função (não `s.list` direto): depois de `assert.equal(s.list, null)` o TypeScript estreitaria o tipo para null.
const projectIds = (s: ProjectsSession) => s.list?.projects.map((p) => p.id)
const botIds = (s: ProjectsSession) => s.list?.bots.map((b) => b.id)
const estudio = { version: 1, hubId: 'h1', spaceId: 'estudio', projects: [{ ...project(1), id: E, name: 'Clipe' }], bots: [{ id: 'bot-estudio', name: 'Diretora' }], capabilities: ['manualProjects'] }

test('Space A → B: projeção, Projeto aberto e pedido incerto de A somem; todas as chamadas levam Hub + Space B', async () => {
  const { src, s } = await opened()
  src.handler = async (m) => { if (m === 'donnaProjectSave') throw new HostErr(0); return m === 'donnaProjects' ? list('h1') : snap('h1', 1) }
  await s.save(draft, s.project!)
  assert.equal(s.write?.status, 'uncertain')
  src.handler = async (m) => m === 'donnaProjects' ? estudio : Promise.reject(new HostErr(400))
  src.log = []
  s.close()
  s.open(src, 'estudio')
  assert.equal(s.space, 'estudio')
  assert.equal(s.list, null, 'nada de A no primeiro quadro de B')
  assert.equal(s.project, null)
  assert.deepEqual(s.view, { kind: 'none' })
  assert.equal(s.write, null, 'o pedido incerto de A não segue para B')
  assert.deepEqual(await s.retry(), { ok: false, status: 'blocked' })
  await tick()
  assert.deepEqual(projectIds(s), [E])
  assert.deepEqual(botIds(s), ['bot-estudio'], 'só o catálogo do próprio Space')
  assert.deepEqual(src.log, ['listen estudio', 'donnaProjects {"hubId":"h1","spaceId":"estudio"}'])
  const r = await s.create({ name: 'Trailer', objective: '', coordinatorBotId: 'bot-estudio' })
  assert.equal(r.ok, false)
  const sent = src.log.find((l) => l.startsWith('donnaProjectCreate'))!
  assert.match(sent, /"hubId":"h1","spaceId":"estudio"/)
  s.close()
})

test('resposta tardia de A não repovoa A nem aparece em B', async () => {
  const src = new Source()
  const s = new ProjectsSession(() => 42, ids())
  let lateA!: (v: unknown) => void
  src.handler = (m, b) => b.spaceId === 'p' ? new Promise((r) => { lateA = r }) : Promise.resolve(m === 'donnaProjects' ? estudio : snap('h1', 1, 'estudio'))
  s.open(src, 'p')
  s.close()
  s.open(src, 'estudio')
  await tick()
  assert.deepEqual(projectIds(s), [E], 'a leitura de A em voo não bloqueia B')
  lateA(list('h1'))
  await tick()
  assert.equal(s.space, 'estudio')
  assert.deepEqual(projectIds(s), [E])
  s.close()
})

test('resposta com outro Space é recusada e nada é pintado', async () => {
  const src = new Source()
  src.handler = async () => list('h1', ['manualProjects'], 'p')
  const s = new ProjectsSession(() => 42, ids())
  s.open(src, 'estudio')
  await tick()
  assert.deepEqual(s.phase, { kind: 'error', reason: 'invalid' })
  assert.equal(s.list, null)
  assert.equal(s.canWrite, false)
  s.close()
})

test('eventos de A não releem B; o aviso de B relê só B', async () => {
  const src = new Source()
  src.handler = async (_m, b) => b.spaceId === 'estudio' ? estudio : list('h1')
  const s = new ProjectsSession(() => 42, ids())
  s.open(src, 'p')
  await tick()
  const fromA = src.emits.get('p')!
  s.close()
  s.open(src, 'estudio')
  await tick()
  src.log = []
  fromA({ type: 'donnaProjectsChanged', spaceId: 'p' })
  fromA({ type: 'resync' } as TowerEvent)
  src.emit!({ type: 'donnaProjectsChanged', spaceId: 'p' })
  await tick(300)
  assert.deepEqual(src.log, [], 'nem o ouvinte antigo nem o aviso de A tocam em B')
  src.emit!({ type: 'donnaProjectsChanged', spaceId: 'estudio' })
  await tick(300)
  assert.deepEqual(src.log, ['donnaProjects {"hubId":"h1","spaceId":"estudio"}'])
  s.close()
})

// Contrato do Hub: `not_allowed` em HTTP 400 (autorização original encerrada, inclusive na entrada dispatch) é dúvida
// de autoridade, não revogação: a fotografia da MESMA origem fica, antiga, e nada grava até uma leitura autorizada.
const ambiguous = () => new HostErr(400, 'not_allowed: autorização original encerrada')

test('queda simples de link: fotografia fica, antiga; gravar é recusado sem chamar o Hub', async () => {
  const { src, s } = await opened()
  src.connectedClient = null
  src.notify()
  assert.equal(s.project?.project.revision, 1)
  assert.equal(s.stale, true)
  assert.equal(s.canWrite, false)
  src.log = []
  assert.deepEqual(await s.save(draft, s.project!), { ok: false, status: 'blocked' })
  assert.equal(src.log.length, 0)
  s.close()
})

test('400 not_allowed na leitura: preserva a fotografia da mesma origem, só leitura até a leitura autorizada', async () => {
  const { src, s } = await opened()
  src.handler = async () => { throw ambiguous() }
  s.reread()
  await tick()
  assert.deepEqual(s.phase, { kind: 'ready' })
  assert.equal(s.list?.projects.length, 1, 'lista não é apagada')
  assert.equal(s.project?.project.revision, 1, 'Projeto não é apagado')
  assert.equal(s.authority, 'pending')
  assert.equal(s.stale, true, 'marcado como antigo')
  assert.equal(s.canWrite, false)
  src.log = []
  assert.deepEqual(await s.save(draft, s.project!), { ok: false, status: 'blocked' })
  assert.equal(src.log.length, 0, 'nada é enviado com a autoridade em dúvida')
  // Recupera só com uma leitura completa e autorizada (lista e Projeto) desta origem.
  src.rev = 2
  src.handler = null
  s.reread()
  await tick()
  assert.equal(s.authority, 'ok')
  assert.equal(s.stale, false)
  assert.equal(s.canWrite, true)
  assert.equal(s.project?.project.revision, 2)
  s.close()
})

test('400 not_allowed na leitura do Projeto (lista ok): mesmo tratamento, sem liberar gravação', async () => {
  const { src, s } = await opened()
  src.handler = async (m) => { if (m === 'donnaProject') throw ambiguous(); return list('h1') }
  s.reread()
  await tick()
  assert.equal(s.project?.project.revision, 1)
  assert.equal(s.canWrite, false)
  assert.equal(s.stale, true)
  s.close()
})

test('400 not_allowed na gravação: sem retry nem pedido guardado; fotografia fica; relê e só a leitura autorizada libera', async () => {
  const { src, s } = await opened()
  let reads = 0
  src.handler = async (m) => {
    if (m === 'donnaProjectSave') throw ambiguous()
    reads++
    if (reads <= 1) throw ambiguous()
    return m === 'donnaProjects' ? list('h1') : snap('h1', 1)
  }
  assert.deepEqual(await s.save(draft, s.project!), { ok: false, status: 'blocked' })
  assert.equal(s.write, null, 'nada fica para Tentar de novo')
  assert.equal(s.project?.project.revision, 1)
  assert.equal(s.canWrite, false)
  assert.deepEqual(await s.retry(), { ok: false, status: 'blocked' })
  await tick()
  assert.equal(src.log.filter((l) => l.startsWith('donnaProjectSave')).length, 1, 'um envio só, nenhum reenvio')
  assert.equal(s.canWrite, false, 'releitura que bate de novo no not_allowed não libera')
  s.reread()
  await tick()
  assert.equal(s.canWrite, true, 'leitura autorizada libera')
  s.close()
})

test('403 explícito segue limpando, sem levar o texto do Hub para a tela', async () => {
  const { src, s } = await opened()
  src.handler = async () => { throw new HostErr(403, 'forbidden: original authorization ended') }
  assert.deepEqual(await s.save(draft, s.project!), { ok: false, status: 'rejected' })
  assert.deepEqual(s.phase, { kind: 'error', reason: 'lostAccess' })
  assert.equal(s.project, null)
  assert.equal(s.write, null)
  assert.equal(s.authority, 'ok')
  s.close()
})

test('rascunho montado sobre fotografia já trocada (releitura ainda não desenhada): nada é enviado', async () => {
  const { src, s } = await opened()
  const drawn = s.project!
  src.rev = 2
  s.reread()
  await tick()
  assert.notEqual(s.project, drawn)
  src.log = []
  assert.deepEqual(await s.save(draft, drawn), { ok: false, status: 'outdated' })
  assert.equal(src.log.length, 0, 'conteúdo velho não vai com a revisão nova')
  assert.equal(s.write, null)
  const r = await s.save(draft, s.project!)
  assert.equal(src.log.find((l) => l.startsWith('donnaProjectSave'))?.includes('"expectedRevision":2'), true)
  assert.equal(r.ok, false, 'o Source de teste não grava; o que importa é a revisão enviada')
  s.close()
})

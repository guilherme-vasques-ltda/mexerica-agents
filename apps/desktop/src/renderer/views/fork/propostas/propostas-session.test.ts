import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { TowerEvent, TowerLink } from '../torre/torre-session.ts'
import { PropostasSession } from './propostas-session.ts'

const proposal = (id: string, version = 1, state: 'pending' | 'approved' | 'rejected' = 'pending') => ({
  id,
  version,
  state,
  botId: 'berga',
  projectId: 'proj-1',
  projectName: 'Canal',
  title: 'Plano inicial',
  createdAt: 1000,
  decidedAt: state === 'pending' ? null : 2000,
  items: [
    {
      id: 'i1',
      title: 'Roteiro',
      description: 'Escrever roteiro',
      acceptance: 'Roteiro revisado',
      assigneeBotId: 'b1',
      included: true,
      taskId: state === 'approved' ? 't1' : null,
      taskState: state === 'approved' ? 'in_progress' : null,
      startState: state === 'approved' ? 'started' : null,
    },
  ],
})

const snap = (hubId: string, spaceId = 'p', list = [proposal('prop-1')]) => ({
  version: 1,
  hubId,
  spaceId,
  limits: { perBot: 1, total: 2 },
  proposals: list,
})

class HostErr extends Error {
  status: number
  constructor(status: number, message = 'erro') {
    super(message)
    this.status = status
  }
}

type Handler = (method: string, body: Record<string, unknown>) => Promise<unknown>

class Source {
  hello: { hostId: string; rev: number } | null = { hostId: 'h1', rev: 1 }
  principalEpoch = 1
  log: string[] = []
  emits = new Map<string, (e: TowerEvent) => void>()
  ends = new Map<string, (err: unknown) => void>()
  handler: Handler | null = null
  connectedClient: TowerLink | null = this.link()
  private listeners = new Set<() => void>()

  link(): TowerLink {
    return {
      call: async (method, body) => {
        const b = (body ?? {}) as Record<string, unknown>
        this.log.push(`${method} ${JSON.stringify(b)}`)
        if (this.handler) return this.handler(method, b)
        return snap(this.hello!.hostId, String(b.spaceId ?? 'p'))
      },
      inSpace: (spaceId) => ({
        events: (_since, _client, onEvent, onEnd) => {
          this.log.push(`listen ${spaceId}`)
          this.emits.set(spaceId, onEvent)
          this.ends.set(spaceId, onEnd)
          return () => {}
        },
      }),
    }
  }

  subscribe(l: () => void) {
    this.listeners.add(l)
    return () => {
      this.listeners.delete(l)
    }
  }

  notify() {
    for (const l of this.listeners) l()
  }
}

const tick = (ms = 0) => new Promise((r) => setTimeout(r, ms))
const flushMicrotasks = async () => {
  for (let i = 0; i < 12; i++) await Promise.resolve()
}
const ids = () => {
  let n = 0
  return () => `00000000-0000-4000-8000-${String(++n).padStart(12, '0')}`
}

test('assina o stream antes de ler; donnaProjectsChanged de outro Space/Hub não relê e seguidos coalescem', async () => {
  const src = new Source()
  const s = new PropostasSession(() => 42, ids())
  s.open(src, 'p')
  assert.deepEqual(src.log, ['listen p', 'donnaTaskProposals {"hubId":"h1","spaceId":"p"}'])
  await tick()
  assert.equal(s.phase.kind, 'ready')
  assert.equal(s.canDecide, true)

  src.log = []
  const emit = src.emits.get('p')!
  emit({ type: 'donnaProjectsChanged', spaceId: 'outro' })
  emit({ type: 'donnaProjectsChanged', spaceId: 'p', hubId: 'h9' } as TowerEvent)
  emit({ type: 'donnaProjectsChanged', spaceId: 'p' })
  emit({ type: 'donnaProjectsChanged', spaceId: 'p' })
  await tick(300)
  assert.equal(src.log.length, 1)
  s.close()
})

test('Hub antigo recusa o método: estado unavailable silencioso e sem dados', async () => {
  const src = new Source()
  src.handler = async () => {
    throw new HostErr(404, 'unknown method')
  }
  const s = new PropostasSession(() => 42, ids())
  s.open(src, 'p')
  await tick()
  assert.deepEqual(s.phase, { kind: 'unavailable' })
  assert.equal(s.snapshot, null)
  assert.equal(s.canDecide, false)
  s.close()
})

test('resposta de outro Space ou troca de Space/Hub nunca pinta na origem atual', async () => {
  const src = new Source()
  let resolveA!: (v: unknown) => void
  src.handler = (_method, body) => {
    if (body.spaceId === 'space-a') return new Promise((r) => { resolveA = r })
    return Promise.resolve(snap('h1', 'space-b', [proposal('prop-b')]))
  }
  const s = new PropostasSession(() => 42, ids())
  s.open(src, 'space-a')
  s.open(src, 'space-b')
  await tick()
  assert.equal(s.snapshot?.proposals[0]?.id, 'prop-b')

  // Resposta atrasada do space-a não pode sobrescrever space-b.
  resolveA(snap('h1', 'space-a', [proposal('prop-a')]))
  await tick()
  assert.equal(s.snapshot?.spaceId, 'space-b')
  assert.equal(s.snapshot?.proposals[0]?.id, 'prop-b')

  // Resposta declarando outro spaceId para a origem aberta falha estritamente.
  src.handler = async () => snap('h1', 'space-errado')
  s.reread()
  await tick()
  assert.equal(s.snapshot, null)
  assert.deepEqual(s.phase, { kind: 'error', reason: 'invalid' })
  s.close()
})

test('uma decisão por vez; falha incerta guarda e retry reenvia exatamente o mesmo pedido', async () => {
  const src = new Source()
  const s = new PropostasSession(() => 42, ids())
  s.open(src, 'p')
  await tick()

  let release!: (v: unknown) => void
  src.handler = (method) => {
    if (method === 'donnaTaskProposalDecide') return new Promise((r) => { release = r })
    return Promise.resolve(snap('h1', 'p'))
  }

  const first = s.decide({ id: 'prop-1', version: 1 }, 'approve', ['i2'])
  assert.equal(s.write?.status, 'sending')
  assert.equal(s.canDecide, false)

  const blocked = await s.decide({ id: 'prop-1', version: 1 }, 'reject')
  assert.deepEqual(blocked, { ok: false, status: 'blocked' })

  release({ proposal: proposal('prop-1', 2, 'approved') })
  const done = await first
  assert.equal(done.ok, true)
  assert.equal(s.snapshot?.proposals[0]?.state, 'approved')

  // Falha incerta (status 0) guarda o mesmo pedido e retry reenvia com o mesmo clientRequestId.
  const s2 = new PropostasSession(() => 42, ids())
  const src2 = new Source()
  s2.open(src2, 'p')
  await tick()

  src2.handler = async (method) => {
    if (method === 'donnaTaskProposalDecide') throw new HostErr(0, 'network lost')
    return snap('h1', 'p')
  }
  const failRes = await s2.decide({ id: 'prop-1', version: 1 }, 'approve', ['i1'])
  assert.equal(failRes.ok, false)
  if (!failRes.ok) assert.equal(failRes.status, 'uncertain')
  const savedId = s2.write?.clientRequestId
  assert.equal(savedId, '00000000-0000-4000-8000-000000000001')
  const sentFirst = src2.log.filter((l) => l.startsWith('donnaTaskProposalDecide')).at(-1)

  src2.handler = async (method) => {
    if (method === 'donnaTaskProposalDecide') return { proposal: proposal('prop-1', 2, 'approved') }
    return snap('h1', 'p', [proposal('prop-1', 2, 'approved')])
  }
  const retried = await s2.retry()
  assert.equal(retried.ok, true)
  const sentRetry = src2.log.filter((l) => l.startsWith('donnaTaskProposalDecide')).at(-1)
  assert.equal(sentRetry, sentFirst)
  assert.equal(s2.write, null)

  s.close()
  s2.close()
})

test('conflito e decidido relêem sem reenviar sozinhos; proposal_invalid vira rejected', async () => {
  const src = new Source()
  const s = new PropostasSession(() => 42, ids())
  s.open(src, 'p')
  await tick()

  src.log = []
  src.handler = async (method) => {
    if (method === 'donnaTaskProposalDecide') throw new HostErr(409, 'proposal_conflict: esta proposta mudou')
    return snap('h1', 'p', [proposal('prop-1', 2, 'pending')])
  }
  const c = await s.decide({ id: 'prop-1', version: 1 }, 'approve')
  assert.deepEqual(c, { ok: false, status: 'conflict' })
  assert.equal(s.write?.status, 'conflict')
  assert.equal(s.write?.detail, 'esta proposta mudou')
  await tick()
  assert.equal(s.snapshot?.proposals[0]?.version, 2)
  assert.equal(src.log.filter((l) => l.startsWith('donnaTaskProposalDecide')).length, 1)

  src.log = []
  src.handler = async (method) => {
    if (method === 'donnaTaskProposalDecide') throw new HostErr(409, 'proposal_decided: já foi decidida')
    return snap('h1', 'p', [proposal('prop-1', 3, 'approved')])
  }
  const d = await s.decide({ id: 'prop-1', version: 2 }, 'reject')
  assert.deepEqual(d, { ok: false, status: 'decided' })
  assert.equal(s.write?.status, 'decided')
  await tick()
  assert.equal(s.snapshot?.proposals[0]?.state, 'approved')
  assert.equal(src.log.filter((l) => l.startsWith('donnaTaskProposalDecide')).length, 1)

  src.handler = async (method) => {
    if (method === 'donnaTaskProposalDecide') throw new HostErr(400, 'proposal_invalid: escolha pelo menos uma tarefa')
    return snap('h1', 'p', [proposal('prop-1', 3, 'pending')])
  }
  const inv = await s.decide({ id: 'prop-1', version: 3 }, 'approve')
  assert.deepEqual(inv, { ok: false, status: 'rejected' })
  assert.equal(s.write?.detail, 'escolha pelo menos uma tarefa')
  s.close()
})

test('troca de época durante envio vira incerto e bloqueia decidir até leitura autorizada', async () => {
  const src = new Source()
  const s = new PropostasSession(() => 42, ids())
  s.open(src, 'p')
  await tick()

  let releaseDecide!: (v: unknown) => void
  src.handler = (method) => {
    if (method === 'donnaTaskProposalDecide') return new Promise((r) => { releaseDecide = r })
    return Promise.resolve(snap('h1', 'p'))
  }
  const pending = s.decide({ id: 'prop-1', version: 1 }, 'approve')
  src.principalEpoch = 2
  src.connectedClient = src.link()
  src.notify()
  releaseDecide({ proposal: proposal('prop-1', 2, 'approved') })
  const res = await pending
  assert.equal(res.ok, false)
  if (!res.ok) assert.equal(res.status, 'uncertain')
  assert.equal(s.write?.status, 'uncertain')
  s.close()
})

test('falha na primeira leitura expõe retryRead e faz retry limitado sem afetar Hub antigo', async (t) => {
  const src = new Source()
  let fail = true
  src.handler = async () => {
    if (fail) throw new HostErr(504, 'timeout')
    return snap('h1', 'p')
  }
  const s = new PropostasSession(() => 42, ids())
  s.open(src, 'p')
  await tick()
  assert.deepEqual(s.phase, { kind: 'error', reason: 'failed' })
  assert.equal(s.snapshot, null)
  assert.equal(s.canDecide, false)

  fail = false
  s.retryRead()
  await tick()
  assert.equal(s.phase.kind, 'ready')
  assert.equal((s.snapshot as { proposals: unknown[] } | null)?.proposals.length, 1)
  assert.equal(s.canDecide, true)
  s.close()

  // Retry automático é limitado quando a primeira leitura falha continuamente.
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const srcAuto = new Source()
  let calls = 0
  srcAuto.handler = async () => {
    calls++
    throw new HostErr(504, 'timeout')
  }
  const sAuto = new PropostasSession(() => 42, ids())
  sAuto.open(srcAuto, 'p')
  await flushMicrotasks()
  assert.equal(calls, 1)
  t.mock.timers.tick(2_000)
  await flushMicrotasks()
  assert.equal(calls, 2)
  t.mock.timers.tick(4_000)
  await flushMicrotasks()
  assert.equal(calls, 3)
  t.mock.timers.tick(20_000)
  await flushMicrotasks()
  assert.equal(calls, 3)
  sAuto.close()
})

test('leitura atrasada não regride decisão e recibo não sobrescreve taskState/startState mais novos', async () => {
  const src = new Source()
  const s = new PropostasSession(() => 42, ids())
  s.open(src, 'p')
  await tick()

  let resolveOldRead!: (v: unknown) => void
  let readCount = 0
  src.handler = (method) => {
    if (method === 'donnaTaskProposals') {
      readCount++
      if (readCount === 1) return new Promise((r) => { resolveOldRead = r })
      return Promise.resolve(snap('h1', 'p', [proposal('prop-1', 2, 'approved')]))
    }
    return Promise.resolve({
      proposal: {
        ...proposal('prop-1', 2, 'approved'),
        items: [{ ...proposal('prop-1', 2, 'approved').items[0]!, taskState: 'backlog', startState: 'scheduled' }],
      },
    })
  }

  // Leitura da lista iniciada ANTES da decisão confirmar.
  s.reread()
  const decided = await s.decide({ id: 'prop-1', version: 1 }, 'approve')
  assert.equal(decided.ok, true)
  assert.equal(s.snapshot?.proposals[0]?.state, 'approved')

  // Resposta atrasada da leitura anterior (ainda pending) não pode regredir o estado.
  resolveOldRead(snap('h1', 'p', [proposal('prop-1', 1, 'pending')]))
  await tick()
  assert.equal(s.snapshot?.proposals[0]?.state, 'approved')

  // Reconcilia com uma releitura da lista após confirmar.
  await tick(300)
  assert.equal(readCount, 2)
  assert.equal(s.snapshot?.proposals[0]?.items[0]?.taskState, 'in_progress')

  // Se o snapshot já viu versão igual/mais nova com taskState/startState mais atual, o recibo atrasado não sobrescreve.
  const s2 = new PropostasSession(() => 42, ids())
  const src2 = new Source()
  s2.open(src2, 'p')
  await tick()

  let resolveSlowDecide!: (v: unknown) => void
  src2.handler = (method) => {
    if (method === 'donnaTaskProposalDecide') return new Promise((r) => { resolveSlowDecide = r })
    return Promise.resolve(snap('h1', 'p', [
      {
        ...proposal('prop-1', 2, 'approved'),
        items: [{ ...proposal('prop-1', 2, 'approved').items[0]!, taskState: 'done', startState: 'started' }],
      },
    ]))
  }
  const pendingWrite = s2.decide({ id: 'prop-1', version: 1 }, 'approve')
  // Antes do recibo chegar, uma releitura por evento já trouxe a versão 2 com taskState 'done'.
  s2.reread()
  await tick()
  assert.equal(s2.snapshot?.proposals[0]?.items[0]?.taskState, 'done')

  // Agora o recibo da decisão (mesma versão 2, mas ainda 'backlog'/'scheduled') chega.
  resolveSlowDecide({
    proposal: {
      ...proposal('prop-1', 2, 'approved'),
      items: [{ ...proposal('prop-1', 2, 'approved').items[0]!, taskState: 'backlog', startState: 'scheduled' }],
    },
  })
  const res2 = await pendingWrite
  assert.equal(res2.ok, true)
  assert.equal(s2.snapshot?.proposals[0]?.items[0]?.taskState, 'done')
  assert.equal(s2.snapshot?.proposals[0]?.items[0]?.startState, 'started')

  s.close()
  s2.close()
})

test('queda de stream limpa verificação e só libera ações após leitura bem-sucedida ao reconectar', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const src = new Source()
  let failRead = false
  src.handler = async () => {
    if (failRead) throw new HostErr(500, 'temp')
    return snap('h1', 'p')
  }
  const s = new PropostasSession(() => 42, ids())
  s.open(src, 'p')
  await flushMicrotasks()
  assert.equal(s.stale, false)
  assert.equal(s.canDecide, true)

  // Stream cai: perde verificação imediatamente mesmo mantendo snapshot em cache.
  src.ends.get('p')!(new Error('stream dropped'))
  assert.equal(s.events, 'paused')
  assert.equal(s.stale, true)
  assert.equal(s.canDecide, false)
  assert.ok(s.snapshot !== null)

  // Timer de reconexão dispara, mas a leitura após reconectar falha: continua bloqueado.
  failRead = true
  t.mock.timers.tick(2_000)
  await flushMicrotasks()
  assert.equal(s.events, 'live')
  assert.equal(s.rereadFailed, true)
  assert.equal(s.stale, true)
  assert.equal(s.canDecide, false)

  // Leitura bem-sucedida após reconectar libera ações novamente.
  failRead = false
  s.reread()
  await flushMicrotasks()
  assert.equal(s.rereadFailed, false)
  assert.equal(s.stale, false)
  assert.equal(s.canDecide, true)
  s.close()
})

test('403 atrasado de conexão substituída preserva snapshot e pedido incerto; 403 atual limpa acesso', async () => {
  const src = new Source()
  const s = new PropostasSession(() => 42, ids())
  s.open(src, 'p')
  await tick()

  let rejectOld!: (e: unknown) => void
  src.handler = (method) => {
    if (method === 'donnaTaskProposalDecide') return new Promise((_r, rej) => { rejectOld = rej })
    return Promise.resolve(snap('h1', 'p'))
  }
  const inFlight = s.decide({ id: 'prop-1', version: 1 }, 'approve')
  const savedRequestId = s.write?.clientRequestId

  // Substitui conexão (nova época/client).
  src.principalEpoch = 2
  src.connectedClient = src.link()
  src.notify()
  await tick()

  // 403 atrasado da conexão antiga não pode limpar snapshot nem o pedido salvo.
  rejectOld(new HostErr(403, 'forbidden'))
  const delayedRes = await inFlight
  assert.equal(delayedRes.ok, false)
  if (!delayedRes.ok) assert.equal(delayedRes.status, 'uncertain')
  assert.ok(s.snapshot !== null)
  assert.equal(s.phase.kind, 'ready')
  assert.equal(s.write?.status, 'uncertain')
  assert.equal(s.write?.clientRequestId, savedRequestId)

  // Na conexão atual, 403 limpa acesso, snapshot e pedido salvo.
  src.handler = async (method) => {
    if (method === 'donnaTaskProposalDecide') throw new HostErr(403, 'forbidden')
    return snap('h1', 'p')
  }
  const currentRes = await s.retry()
  assert.deepEqual(currentRes, { ok: false, status: 'rejected' })
  assert.equal(s.snapshot, null)
  assert.equal(s.write, null)
  assert.deepEqual(s.phase, { kind: 'error', reason: 'lostAccess' })
  s.close()
})

test('close() preserva pedido incerto para mesma autoridade e escopo; limpa só em dispensa ou troca de escopo/autoridade', async () => {
  const src = new Source()
  const s = new PropostasSession(() => 42, ids())
  s.open(src, 'p')
  await tick()

  src.handler = async (method) => {
    if (method === 'donnaTaskProposalDecide') throw new HostErr(0, 'network lost')
    return snap(src.hello!.hostId, 'p')
  }
  await s.decide({ id: 'prop-1', version: 1 }, 'approve', ['i2'])
  assert.equal(s.write?.status, 'uncertain')
  const savedId = s.write!.clientRequestId
  const savedBody = structuredClone(s.write!.body)

  // Sai da aba e volta para a mesma autoridade (h1) e mesmo escopo (p).
  s.close()
  assert.equal(s.write?.status, 'uncertain')
  assert.equal(s.write?.clientRequestId, savedId)

  s.open(src, 'p')
  await tick()
  assert.equal(s.write?.status, 'uncertain')
  assert.equal(s.write?.clientRequestId, savedId)
  assert.deepEqual(s.write?.body, savedBody)

  src.handler = async (method) => {
    if (method === 'donnaTaskProposalDecide') return { proposal: proposal('prop-1', 2, 'approved') }
    return snap('h1', 'p', [proposal('prop-1', 2, 'approved')])
  }
  const retried = await s.retry()
  assert.equal(retried.ok, true)
  const sentRetry = src.log.filter((l) => l.startsWith('donnaTaskProposalDecide')).at(-1)
  assert.equal(sentRetry, `donnaTaskProposalDecide ${JSON.stringify(savedBody)}`)

  // Trocar de Space descarta pedido incerto.
  src.handler = async (method) => {
    if (method === 'donnaTaskProposalDecide') throw new HostErr(0, 'network lost')
    return snap(src.hello!.hostId, 'p')
  }
  await s.decide({ id: 'prop-1', version: 2 }, 'reject')
  assert.equal(s.write?.status, 'uncertain')
  s.close()
  src.handler = async (_method, body) => snap(src.hello!.hostId, String(body.spaceId ?? 'p'))
  s.open(src, 'outro-space')
  await tick()
  assert.equal(s.write, null)

  // Trocar de Hub descarta pedido incerto.
  s.open(src, 'p')
  await tick()
  src.handler = async (method) => {
    if (method === 'donnaTaskProposalDecide') throw new HostErr(0, 'network lost')
    return snap(src.hello!.hostId, 'p')
  }
  await s.decide({ id: 'prop-1', version: 1 }, 'reject')
  assert.equal((s.write as { status: string } | null)?.status, 'uncertain')
  s.close()
  src.hello = { hostId: 'h2', rev: 1 }
  src.handler = async () => snap('h2', 'p')
  s.open(src, 'p')
  await tick()
  assert.equal(s.write, null)

  // Dispensa explícita limpa.
  src.handler = async (method) => {
    if (method === 'donnaTaskProposalDecide') throw new HostErr(0, 'network lost')
    return snap('h2', 'p')
  }
  await s.decide({ id: 'prop-1', version: 1 }, 'reject')
  assert.equal((s.write as { status: string } | null)?.status, 'uncertain')
  s.dismissWrite()
  assert.equal(s.write, null)
  s.close()
})

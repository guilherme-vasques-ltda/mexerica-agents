import { test } from 'node:test'
import assert from 'node:assert/strict'
import { TowerSession, type TowerEvent, type TowerLink } from './torre-session.ts'

// VG-02: assina antes de ler, relê no aviso do Hub, guarda a última fotografia como antiga sem link e esquece outro Hub.
const snap = (hubId: string, n: number) => ({ version: 1, hubId, snapshotId: `s${n}`, collectedAt: n, spaceId: 'p', bots: [] })

class Source {
  hello: { hostId: string; rev: number } | null = { hostId: 'h1', rev: 1 }
  principalEpoch = 1
  log: string[] = []
  emit: ((e: TowerEvent) => void) | null = null
  reads = 0
  connectedClient: TowerLink | null = this.link()
  private listeners = new Set<() => void>()
  link(): TowerLink {
    return {
      call: async (method, body) => {
        this.log.push(`call ${method} ${JSON.stringify(body)}`)
        return snap(this.hello!.hostId, ++this.reads)
      },
      inSpace: (spaceId) => ({ events: (_since, _client, onEvent) => { this.log.push(`listen ${spaceId}`); this.emit = onEvent; return () => {} } }),
    }
  }
  subscribe(l: () => void) { this.listeners.add(l); return () => { this.listeners.delete(l) } }
  notify() { for (const l of this.listeners) l() }
}
const tick = (ms = 0) => new Promise((r) => setTimeout(r, ms))

test('badge fetches the scoped queue, rejects foreign hints, and clears a previous principal', async () => {
  const src = new Source()
  src.connectedClient = { ...src.link(), call: async (method, body) => {
    assert.equal(method, 'donnaAttention')
    assert.deepEqual(body, { spaceId: 'p', includeResolvedToday: false })
    return { version: 1, hubId: 'h1', spaceId: 'p', collectedAt: ++src.reads, items: [], resolvedToday: [], coverageGaps: [] }
  } }
  const session = new TowerSession(Date.now, 'badge')
  session.open(src, 'p')
  try {
    await tick()
    assert.equal(session.currentAttention(src, 'p')?.collectedAt, 1)
    src.emit!({ type: 'donnaAttentionChanged', spaceId: 'p', hubId: 'other' })
    await tick(275)
    assert.equal(src.reads, 1)
    src.emit!({ type: 'donnaAttentionChanged', spaceId: 'p', hubId: 'h1' })
    await tick(275)
    assert.equal(session.currentAttention(src, 'p')?.collectedAt, 2)
    src.principalEpoch++
    assert.equal(session.currentAttention(src, 'p'), null)
  } finally {
    session.close()
  }
})

test('assina antes de ler, relê no aviso, fica antiga sem link e esquece outro Hub', async () => {
  const src = new Source()
  const s = new TowerSession(() => 42)
  s.open(src, 'p')
  assert.deepEqual(src.log, ['listen p', 'call donnaControlTower {"hubId":"h1","spaceId":"p"}'])
  await tick()
  assert.equal(s.snapshot?.snapshotId, 's1')
  src.emit!({ type: 'donnaControlTowerChanged', spaceId: 'outro' })
  src.emit!({ type: 'donnaControlTowerChanged', spaceId: 'p' })
  src.emit!({ type: 'entry' })
  await tick(300)
  assert.equal(src.reads, 2, 'avisos seguidos viram uma leitura; outro space não relê')
  src.connectedClient = null
  src.notify()
  assert.equal(s.link, 'offline')
  assert.equal(s.stale, true)
  assert.equal(s.offlineSince, 42)
  assert.equal(s.snapshot?.snapshotId, 's2', 'a última fotografia continua, marcada como antiga')
  src.hello = { hostId: 'h2', rev: 1 }
  src.connectedClient = src.link()
  src.notify()
  assert.equal(s.snapshot, null)
  assert.equal(s.phase.kind, 'moved')
  s.close()
})

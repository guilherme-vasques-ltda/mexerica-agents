import assert from 'node:assert/strict'
import test from 'node:test'
import { createWorkspaceSession, isWorkspaceAccessLost } from './workspace-session.ts'
import type { WorkspaceContext } from './workspace-session.ts'
import type { Workspace } from './workspaces.ts'

const personal: Workspace = { id: 'personal:user-1', kind: 'personal' }
const team: Workspace = { id: 'org:org-1', kind: 'team', organizationId: 'org-1', name: 'Equipe' }
const context = (workspace: Workspace, epoch = 7): WorkspaceContext => ({
  schema: 1,
  hubId: 'hub-local',
  kind: workspace.kind,
  workspaceId: workspace.id,
  userId: 'user-1',
  role: workspace.kind === 'personal' ? 'owner' : 'member',
  workspaceEpoch: epoch,
  capabilities: [],
})

async function openConfirmed(workspace = personal) {
  const session = createWorkspaceSession(async () => {})
  const lease = await session.open(workspace, 'user-1', () => {})
  assert.ok(lease)
  assert.equal(lease.acceptContext(context(workspace)), true)
  return { session, lease }
}

test('ignora eventos e respostas da sessão antiga após trocar de workspace', async () => {
  const session = createWorkspaceSession(async () => {})
  const old = await session.open(personal, 'user-1', () => {})
  assert.ok(old)
  assert.equal(old.acceptContext(context(personal)), true)
  const next = await session.open(team, 'user-1', () => {})
  assert.ok(next)
  assert.equal(old.acceptEvent({ type: 'update', workspaceId: personal.id, workspaceEpoch: 7 }), false)
  assert.equal(old.acceptsResponse(), false)
  assert.equal(next.acceptContext(context(team)), true)
  assert.equal(next.acceptEvent({ type: 'update', workspaceId: personal.id, workspaceEpoch: 7 }), false)
  await session.close()
})

test('rejeita contexto de epoch inválido ou identidade/Hub-workspace incompatível', async () => {
  const session = createWorkspaceSession(async () => {})
  const lease = await session.open(team, 'user-1', () => {})
  assert.ok(lease)
  assert.equal(lease.acceptContext({ ...context(team), workspaceEpoch: -1 }), false)
  assert.equal(lease.acceptContext({ ...context(team), workspaceId: personal.id }), false)
  assert.equal(lease.acceptContext({ ...context(team), userId: 'user-2' }), false)
  assert.equal(lease.acceptContext(context(team)), true)
  await session.close()
})

test('role_changed ou revogação com epoch posterior revoga a lease; epoch igual não', async () => {
  for (const type of ['role_changed', 'workspace_revoked']) {
    const { session, lease } = await openConfirmed()
    assert.equal(lease.acceptEvent({ type, workspaceId: personal.id, workspaceEpoch: 7 }), false)
    assert.equal(lease.acceptsResponse(), true)
    assert.equal(lease.acceptEvent({ type, workspaceId: personal.id, workspaceEpoch: 8 }), false)
    assert.equal(lease.acceptsResponse(), false)
    await session.close()
  }
})

test('mismatch de workspace ou epoch não aceita evento', async () => {
  const { session, lease } = await openConfirmed()
  assert.equal(lease.acceptEvent({ type: 'update', workspaceId: personal.id, workspaceEpoch: 8 }), false)
  assert.equal(lease.acceptEvent({ type: 'update', workspaceId: 'org:other', workspaceEpoch: 7 }), false)
  assert.equal(lease.acceptEvent({ type: 'update', workspaceId: personal.id, workspaceEpoch: 7 }), true)
  await session.close()
})

test('close aborta lease, executa disposers e espera a limpeza antes de abrir outra', async () => {
  const barriers: (() => void)[] = []
  let cleared = 0
  const session = createWorkspaceSession(async () => {
    cleared++
    await new Promise<void>((resolve) => { barriers.push(resolve) })
  })
  const lease = await session.open(personal, 'user-1', () => {})
  assert.ok(lease)
  lease.acceptContext(context(personal))
  let disposed = 0
  lease.onDispose(() => { disposed++ })

  const closing = session.close()
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(lease.signal.aborted, true)
  assert.equal(disposed, 1)
  assert.equal(cleared, 1)
  let opened = false
  const opening = session.open(team, 'user-1', () => {}).then((next) => { opened = true; return next })
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(opened, false)
  barriers[0]()
  await closing
  const next = await opening
  assert.ok(next)
  const finalClose = session.close()
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(barriers.length, 2)
  barriers[1]()
  await finalClose
})

test('cleanup failure não bloqueia limpeza posterior; dispose remove callbacks apenas uma vez', async () => {
  let calls = 0
  const session = createWorkspaceSession(async () => {
    calls++
    if (calls === 1) throw new Error('cleanup failed')
  })
  const lease = await session.open(personal, 'user-1', () => {})
  assert.ok(lease)
  let disposed = 0
  lease.onDispose(() => { disposed++ })
  await assert.rejects(session.close(), /cleanup failed/)
  assert.equal(disposed, 1)
  await session.close()
  assert.equal(disposed, 1)
  assert.equal(calls, 2)
})

test('classifica terminalmente workspace_epoch_mismatch', () => {
  assert.equal(isWorkspaceAccessLost(409, 'workspace_epoch_mismatch'), true)
  assert.equal(isWorkspaceAccessLost(409, 'other'), false)
  assert.equal(isWorkspaceAccessLost(401, 'device_revoked'), true)
  assert.equal(isWorkspaceAccessLost(403, 'role_insufficient'), false)
})

import assert from 'node:assert/strict'
import test from 'node:test'
import { createRemoteController, type RemoteHub } from './controller.ts'
import type { RemoteViewState } from './view.ts'
import { createWorkspaceSession, type WorkspaceContext } from './workspace-session.ts'
import type { HubWorkspaceGrant } from './workspaces.ts'

const personal: HubWorkspaceGrant = { id: 'personal:u1', kind: 'personal' }
const team: HubWorkspaceGrant = { id: 'org:t1', kind: 'team', organizationId: 't1' }
const orgs = [{ id: 't1', name: 'Equipe fictícia' }]
const context = (workspace: HubWorkspaceGrant): WorkspaceContext => ({
  schema: 1, userId: 'u1', hubId: 'h1', workspaceId: workspace.id,
  kind: workspace.kind, role: workspace.kind === 'team' ? 'member' : 'owner',
  workspaceEpoch: 2, capabilities: [],
})

test('o perfil sem transporte não autoriza nem se anuncia conectado', async () => {
  const states: RemoteViewState[] = []
  const c = createRemoteController((state) => states.push(state))
  await c.setIdentity('u1', orgs, [personal, team])
  assert.deepEqual(states.at(-1), { status: 'unavailable', workspaces: [], activeId: null, signedIn: true })
})

test('permite selecionar de novo após falha de confirmação sem reaproveitar a sessão', async () => {
  const states: RemoteViewState[] = []
  const signals: AbortSignal[] = []
  const c = createRemoteController((state) => states.push(state), {
    confirm: async (workspace, { signal }) => {
      signals.push(signal)
      if (signals.length === 1) throw new Error('confirmation rejected')
      return context(workspace)
    },
  }, createWorkspaceSession(async () => {}))
  await c.setIdentity('u1', orgs, [personal, team])
  assert.equal(states.at(-1)?.status, 'unavailable')
  assert.equal(states.at(-1)?.activeId, null)
  assert.deepEqual(states.at(-1)?.workspaces.map(({ id }) => id), [personal.id, team.id])
  assert.equal(signals[0].aborted, true)
  await c.select(personal.id)
  assert.equal(states.at(-1)?.status, 'active')
  assert.equal(signals[1].aborted, false)
})

test('estado offline aborta o stream e retry manual substitui a tentativa agendada', async () => {
  type Callbacks = Parameters<RemoteHub['confirm']>[1]
  const attempts: Callbacks[] = []
  const states: RemoteViewState[] = []
  const cleared: string[] = []
  const c = createRemoteController((state) => states.push(state), {
    confirm: async (workspace, callbacks) => {
      attempts.push(callbacks)
      return context(workspace)
    },
  }, createWorkspaceSession(async (id) => { cleared.push(id) }))
  await c.setIdentity('u1', orgs, [personal, team])
  assert.equal(states.at(-1)?.activeId, personal.id)
  assert.equal(states.at(-1)?.status, 'active')
  attempts[0].accessLost(403, 'role_insufficient')
  assert.equal(attempts[0].acceptsResponse(), true)
  attempts[0].offline('transport')
  assert.equal(states.at(-1)?.status, 'hub_offline')
  assert.equal(attempts[0].signal.aborted, true)
  assert.equal(attempts.length, 1)
  await c.retry()
  assert.deepEqual(cleared, [personal.id])
  assert.equal(attempts.length, 2)
  assert.equal(attempts[0].acceptEvent({ type: 'update', workspaceId: personal.id, workspaceEpoch: 2 }), false)
  assert.equal(attempts[1].acceptsResponse(), true)
  attempts[1].accessLost(401, 'device_revoked')
  assert.equal(attempts[1].signal.aborted, true)
  assert.equal(states.at(-1)?.status, 'device_revoked')
  assert.deepEqual(states.at(-1)?.workspaces.map(({ id }) => id), [team.id])
  await c.setIdentity(null, [], [])
})

test('limita o backoff e cancela uma tentativa agendada na troca de conta', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const attempts: Parameters<RemoteHub['confirm']>[1][] = []
  let status: RemoteViewState['status'] | undefined
  let down = false
  const controller = createRemoteController((state) => { status = state.status }, {
    confirm: async (workspace, callbacks) => {
      attempts.push(callbacks)
      if (down) callbacks.offline('transport')
      return context(workspace)
    },
  }, createWorkspaceSession(async () => {}))
  const tick = async (ms: number) => {
    t.mock.timers.tick(ms)
    await new Promise<void>((resolve) => setImmediate(resolve))
  }
  await controller.setIdentity('u1', [], [personal])
  down = true
  attempts[0].offline('transport')
  for (const [i, delay] of [1_000, 2_000, 4_000, 8_000, 16_000, 30_000].entries()) {
    await tick(delay - 1)
    assert.equal(attempts.length, i + 1)
    await tick(1)
    assert.equal(attempts.length, i + 2)
    assert.equal(status, 'hub_offline')
  }
  await tick(120_000)
  assert.equal(attempts.length, 7)
  down = false
  await controller.retry()
  assert.equal(status, 'active')
  attempts.at(-1)!.offline('transport')
  await controller.setIdentity(null, [], [])
  await tick(120_000)
  assert.equal(attempts.length, 8)
  assert.equal(status, 'signed_out')
})

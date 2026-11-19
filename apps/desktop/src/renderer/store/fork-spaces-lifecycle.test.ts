import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Space, SpacesSnapshot } from '../../shared/fork-spaces.ts'
import type { HostClient } from '../client/host-client.ts'
import type { BotStore } from './bot-store.ts'
import { activateSpacePrincipal, deleteSpace, readSpaceBotEditor, type SpacesOrigin } from './fork-spaces.ts'
import { spaceCanDelete, spaceDeleteReady, spaceLifecycleOk, spaceBotLabel } from '../lib/fork-spaces-rail.ts'

// D543 E1 (4.6): origem que mudou não envia; Hub antigo bloqueia; Pessoal não exclui;
// confirmação só com o nome exato; depois de excluir a lista já não tem o Space.

const PERSONAL = 'pessoal'
const space = (id: string, principalBotId: string | null = null): Space => ({
  id, kind: 'space', name: id === 'oficina' ? 'Oficina' : 'Estúdio', position: 1, revision: 1,
  principalBotId, tile: null, appearanceConfigured: true,
  appearance: { wallpaper: 'none', blur: 0, transparency: 0, veilLight: 0, veilDark: 0 }, owner: null,
})
const personal: Space = { ...space(PERSONAL), kind: 'personal', name: 'Pessoal', position: 0 }
const snapshot = (spaces: Space[], revision = 3): SpacesSnapshot => ({ version: 1, personalSpaceId: PERSONAL, revision, spaces })

const fixture = (opts: { lifecycle?: number; spaces?: Space[]; revision?: number } = {}) => {
  const calls: { method: string; body: Record<string, unknown> }[] = []
  const state = { spaces: opts.spaces ?? [personal, space('estudio', 'bot-berga'), space('oficina')], revision: opts.revision ?? 3 }
  const store = {
    hello: { hostId: 'hub-a', spacesContract: 1, ...(opts.lifecycle === undefined ? {} : { spaceLifecycleContract: opts.lifecycle }) },
    connectedClient: null as unknown,
    principalEpoch: 4,
    get spaces() { return state.spaces ? snapshot(state.spaces, state.revision) : null },
    acceptSpaces(answer: SpacesSnapshot) { state.spaces = answer.spaces; state.revision = answer.revision; return true },
  }
  const client = {
    call: async (method: string, body: unknown) => {
      calls.push({ method, body: body as Record<string, unknown> })
      return { hubId: 'hub-a', ...snapshot(state.spaces ?? [], (body as { expectedRevision?: number }).expectedRevision ?? 0) }
    },
  }
  store.connectedClient = client
  const origin: SpacesOrigin = { store: store as unknown as BotStore, hostId: 'hub-a', client: client as unknown as HostClient, epoch: 4, snapshot: snapshot(state.spaces ?? [], opts.revision ?? 3) }
  return { origin, store, state, calls }
}

test('D543 E1: origem que mudou não envia nada — host, link, geração e revisão', async () => {
  for (const change of ['host', 'link', 'epoch', 'revision'] as const) {
    const { origin, store, state, calls } = fixture({ lifecycle: 1 })
    if (change === 'host') store.hello.hostId = 'hub-b'
    if (change === 'link') store.connectedClient = { call: async () => ({}) }
    if (change === 'epoch') store.principalEpoch++
    if (change === 'revision') state.revision = 9 // um evento trouxe a lista nova
    await assert.rejects(deleteSpace(origin, 'oficina', 'Oficina'))
    await assert.rejects(activateSpacePrincipal(origin, 'oficina', { clientNonce: 'n', backend: 'codex', engineAccountId: 'ac' }))
    assert.deepEqual(calls, [], change)
  }
})

test('D543 E1: Hub antigo bloqueia as duas ações; Pessoal nunca exclui; confirmação exige o nome exato', () => {
  assert.equal(spaceLifecycleOk(null), false)
  assert.equal(spaceLifecycleOk({}), false)
  assert.equal(spaceLifecycleOk({ spaceLifecycleContract: 0 }), false)
  assert.equal(spaceLifecycleOk({ spaceLifecycleContract: 1 }), true)
  assert.equal(spaceCanDelete(personal), false)
  assert.equal(spaceCanDelete(space('x')), true)
  assert.equal(spaceDeleteReady('Oficina', 'Oficina', true), true)
  assert.equal(spaceDeleteReady('Oficina', ' Oficina ', true), false, 'o nome deve ser exato')
  assert.equal(spaceDeleteReady('Oficina', 'oficina', true), false, 'maiúsculas contam')
  assert.equal(spaceDeleteReady('Oficina', 'Oficin', true), false)
  assert.equal(spaceDeleteReady('Oficina', 'Oficina', false), false, 'Hub antigo nunca habilita')
})

test('D543 E1: excluir devolve a lista sem o Space — a vista fecha ao voltar ao Pessoal', async () => {
  const { origin, store, state, calls } = fixture({ lifecycle: 1 })
  const client = origin.client as unknown as { call: (m: string, b: unknown) => Promise<unknown> }
  void client
  // o transporte de mentira remove o Space como o Hub faz
  const real = (origin.client as unknown as { call: (m: string, b: unknown) => Promise<unknown> }).call
  ;(origin.client as unknown as { call: (m: string, b: unknown) => Promise<unknown> }).call = async (m, b) => {
    calls.push({ method: m, body: b as Record<string, unknown> })
    state.spaces = state.spaces.filter((s) => s.id !== 'oficina')
    return { hubId: 'hub-a', ...snapshot(state.spaces, 4) }
  }
  void real
  const result = await deleteSpace(origin, 'oficina', 'Oficina')
  assert.ok(!('busy' in result))
  assert.equal(calls.length, 1)
  assert.equal(calls[0]!.method, 'donnaSpaceDelete')
  assert.equal((calls[0]!.body as { confirmName: string }).confirmName, 'Oficina')
  assert.equal(state.spaces.some((s) => s.id === 'oficina'), false, 'o Space sumiu da lista aceita')
  assert.equal(store.spaces?.spaces.some((s) => s.id === 'oficina'), false)
})

test('D543 E1: ocupado volta só com os nomes e a lista não muda', async () => {
  const { origin, state, calls } = fixture({ lifecycle: 1 })
  ;(origin.client as unknown as { call: (m: string, b: unknown) => Promise<unknown> }).call = async (m, b) => {
    calls.push({ method: m, body: b as Record<string, unknown> })
    return { hubId: 'hub-a', busy: [{ botId: 'b1', name: 'Revisor' }] }
  }
  const result = await deleteSpace(origin, 'oficina', 'Oficina')
  assert.ok('busy' in result)
  assert.deepEqual(result.busy, [{ botId: 'b1', name: 'Revisor' }])
  assert.equal(state.spaces.length, 3, 'nada foi aplicado')
})

test('D543 E1: ativação e aparência passam pela origem; leitura do editor é escopada ao Space', async () => {
  const { origin, calls } = fixture({ lifecycle: 1 })
  await activateSpacePrincipal(origin, 'oficina', { clientNonce: 'nonce-1', backend: 'codex', engineAccountId: 'ac_1', appearance: { v: 2, estilo: 'simples', aparencia: { cor: 'roxo', olhos: 'ponto', roupa: 'nenhuma', acessorio: 'nenhum', objeto: 'nenhum' } } })
  assert.equal(calls[0]!.method, 'donnaSpacePrincipal')
  assert.equal(calls[0]!.body.spaceId, 'oficina')
  assert.equal(calls[0]!.body.hubId, 'hub-a')
  const editor = await readSpaceBotEditor(origin, 'estudio', 'bot-berga')
  void editor
  assert.equal(calls[1]!.method, 'donnaBotEditor')
  assert.equal(calls[1]!.body.botId, 'bot-berga')
  assert.equal(calls[1]!.body.spaceId, 'estudio')
})

test('D543 E1: resposta perdida repete corpo imutável após revisão nova', async () => {
  const { origin, state, calls } = fixture({ lifecycle: 1 })
  const { principalIntent } = await import('./fork-spaces.ts')
  const intent = principalIntent(origin, 'oficina')
  ;(origin.client as unknown as { call: (m: string, b: unknown) => Promise<unknown> }).call = async (m, b) => {
    calls.push({ method: m, body: structuredClone(b) as Record<string, unknown> })
    if (calls.length === 1) throw Object.assign(new Error('resposta perdida'), { status: 0 })
    if (m === 'donnaSpaces') { state.revision = 9; return { hubId: 'hub-a', ...snapshot(state.spaces, 9) } }
    return { hubId: 'hub-a', ...snapshot(state.spaces, 9) }
  }
  await assert.rejects(activateSpacePrincipal(origin, 'oficina', { clientNonce: 'original', backend: 'codex', engineAccountId: 'ac' }, intent))
  assert.equal(intent.fixed, true)
  await activateSpacePrincipal(origin, 'oficina', { clientNonce: 'outro', backend: 'claude', engineAccountId: 'outra-conta' }, intent)
  assert.deepEqual(calls[2]!.body, calls[0]!.body)
  assert.equal(calls[2]!.body.expectedRevision, 3)
})

test('D543 E1: ativação em Hub antigo não envia RPC', async () => {
  const { origin, calls } = fixture({ lifecycle: 0 })
  await assert.rejects(activateSpacePrincipal(origin, 'oficina', { clientNonce: 'n', backend: 'codex', engineAccountId: 'ac' }))
  assert.deepEqual(calls, [])
})

test('D543 E1: exclusão em Hub antigo recusa antes do transporte', async () => {
  for (const lifecycle of [0, 2]) {
    const { origin, calls } = fixture({ lifecycle })
    await assert.rejects(deleteSpace(origin, 'oficina', 'Oficina'))
    assert.deepEqual(calls, [])
  }
})


test('D543 E1: listas mistas identificam o principal do Space e preservam o Pessoal', () => {
  const list = snapshot([personal, space('estudio', 'berga-estudio'), space('oficina', 'berga-oficina')])
  assert.equal(spaceBotLabel({ id: 'berga-estudio', name: 'Berga' }, list), 'Berga · Estúdio')
  assert.equal(spaceBotLabel({ id: 'berga-oficina', name: 'Berga' }, list), 'Berga · Oficina')
  assert.equal(spaceBotLabel({ id: 'pessoal', name: 'Berga' }, list), 'Berga')
})

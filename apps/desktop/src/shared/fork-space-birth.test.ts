import test from 'node:test'
import assert from 'node:assert/strict'
import { birthAccounts, birthDefault, SpaceBirthIntent } from './fork-space-birth.ts'
import type { ManagedAccount } from './fork-engine-accounts.ts'
import type { SpaceBirth, SpacesSnapshot } from './fork-spaces.ts'
const account = (id: string, origin: 'legacy' | 'managed' = 'legacy', state: ManagedAccount['state'] = 'connected', backend: ManagedAccount['backend'] = 'codex'): ManagedAccount => ({ id, label: id, origin, state, backend, brand: 'OpenAI', billing: 'subscription', models: [], revision: 1, botIds: [] })
const draft: Omit<SpaceBirth, 'expectedRevision' | 'clientNonce'> = { name: 'Trabalho', tile: { color: 'cobre', style: 'filled', symbol: { kind: 'icon', value: 'briefcase' } }, principal: { name: 'Berga', backend: 'codex', engineAccountId: 'local' } }
const response = { hubId: 'hub-a', version: 1, personalSpaceId: 'personal', revision: 2, spaces: [] } as SpacesSnapshot & { hubId: string }

test('conta padrão é única, conectada e do motor escolhido; ambiguidade não escolhe', () => {
  const list = [account('local'), account('managed', 'managed'), account('off', 'legacy', 'disconnected'), account('claude', 'legacy', 'connected', 'claude')]
  assert.equal(birthDefault(list, 'codex'), 'local')
  assert.deepEqual(birthAccounts(list, 'codex').map(a => a.id), ['local', 'managed'])
  assert.equal(birthDefault([...list, account('another')], 'codex'), '')
  assert.equal(birthDefault([account('managed', 'managed')], 'codex'), '')
})

test('resposta perdida: retry conserva nonce, payload, conta, Hub e revisão originais', async () => {
  const calls: Record<string, unknown>[] = []
  const client = { async call(method: string, body: unknown) {
    assert.equal(method, 'donnaSpaceBirth'); calls.push(structuredClone(body) as Record<string, unknown>)
    if (calls.length === 1) throw Error('timeout')
    return response
  } }
  const source = { hello: { hostId: 'hub-a', spaceBirthContract: 1 }, connectedClient: client, principalEpoch: 4 }
  const intent = new SpaceBirthIntent(source, 'hub-a', client, 4, 'personal', 1, 'intention-a')
  const input = structuredClone(draft)
  await assert.rejects(intent.send(input), /timeout/)
  input.principal.name = 'mudança posterior'
  await intent.send({ ...draft, name: 'outro Space' })
  assert.deepEqual(calls[0], calls[1])
  assert.equal((calls[0]!.principal as { name: string }).name, 'Berga')
  assert.equal(calls[0]!.clientNonce, 'intention-a')
  assert.equal(calls[0]!.hubId, 'hub-a')
  assert.equal(calls[0]!.spaceId, 'personal')
})

test('não envia em Hub antigo ou após troca/reconexão; resposta tardia não é aceita', async () => {
  let calls = 0
  let finish!: (v: unknown) => void
  const client = { call: async () => { calls++; return new Promise(r => { finish = r }) } }
  const source = { hello: { hostId: 'hub-a', spaceBirthContract: 0 }, connectedClient: client, principalEpoch: 1 }
  const intent = new SpaceBirthIntent(source, 'hub-a', client, 1, 'personal', 1, 'n')
  await assert.rejects(intent.send(draft), /space_birth_unsupported/)
  assert.equal(calls, 0)
  source.hello.spaceBirthContract = 1
  const pending = intent.send(draft)
  source.principalEpoch++
  finish(response)
  await assert.rejects(pending, /space_birth_context_changed/)
  await assert.rejects(intent.send(draft), /space_birth_context_changed/)
  assert.equal(calls, 1)
})

test('resposta de outro Hub é recusada e duplo clique não manda outra criação', async () => {
  let calls = 0
  let finish!: (v: unknown) => void
  const client = { call: async () => { calls++; return new Promise(r => { finish = r }) } }
  const source = { hello: { hostId: 'hub-a', spaceBirthContract: 1 }, connectedClient: client, principalEpoch: 1 }
  const intent = new SpaceBirthIntent(source, 'hub-a', client, 1, 'personal', 1, 'n')
  const pending = intent.send(draft)
  await assert.rejects(intent.send(draft), /space_birth_inflight/)
  assert.equal(calls, 1)
  finish({ ...response, hubId: 'hub-b' })
  await assert.rejects(pending, /cross_hub_response/)
})


test('lista alterada antes do envio bloqueia, mas não altera a revisão de um retry', async () => {
  let calls = 0
  const client = { async call() { calls++; throw Error('timeout') } }
  const source = { hello: { hostId: 'hub-a', spaceBirthContract: 1 }, connectedClient: client, principalEpoch: 1, spaces: { revision: 2 } }
  const intent = new SpaceBirthIntent(source, 'hub-a', client, 1, 'personal', 1, 'n')
  await assert.rejects(intent.send(draft), /space_birth_revision_changed/)
  assert.equal(calls, 0)
  source.spaces.revision = 1
  await assert.rejects(intent.send(draft), /timeout/)
  source.spaces.revision = 2
  await assert.rejects(intent.send(draft), /timeout/)
  assert.equal(calls, 2)
})

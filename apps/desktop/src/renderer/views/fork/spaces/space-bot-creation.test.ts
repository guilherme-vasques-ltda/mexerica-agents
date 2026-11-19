import { test } from 'node:test'
import assert from 'node:assert/strict'
import { SpaceBotCreation } from '../../../store/fork-space-bot-creation.ts'
import type { SpaceSource } from '../../../store/fork-space-context.ts'
import type { SpacesSnapshot } from '../../../../shared/fork-spaces.ts'

const fixture = () => {
  const calls: unknown[] = []
  const source = { hello: { hostId: 'hub-A', rev: 1, spacesContract: 1 }, principalEpoch: 1,
    spaces: { spaces: [{ id: 'trabalho', kind: 'space' }] } as SpacesSnapshot,
    connectedClient: { call: async (method: string, body: unknown) => { calls.push([method, body]); return { bot: { id: 'novo', spaceId: 'trabalho' } } }, inSpace: () => ({ events: () => () => {} }) },
    subscribe: () => () => {},
  } satisfies SpaceSource
  return { source, calls }
}
test('D543: + contextual fixa Hub/Space e não aceita corpo do Pessoal', async () => {
  const { source, calls } = fixture()
  const creation = new SpaceBotCreation(source, 'trabalho')
  await assert.rejects(creation.create({ name: 'Inválido' }), /cross_space_reference/)
  await creation.create({ name: 'Certo', spaceId: 'trabalho' })
  assert.deepEqual(calls, [['createBot', { name: 'Certo', hubId: 'hub-A', spaceId: 'trabalho' }]])
})
test('D543: folha aberta não reaproveita Hub, link ou geração novos', async () => {
  for (const change of ['host', 'link', 'epoch', 'space'] as const) {
    const { source, calls } = fixture()
    const creation = new SpaceBotCreation(source, 'trabalho')
    if (change === 'host') source.hello.hostId = 'hub-B'
    if (change === 'link') source.connectedClient = { ...source.connectedClient }
    if (change === 'epoch') source.principalEpoch++
    if (change === 'space') source.spaces.spaces = []
    await assert.rejects(creation.create({ spaceId: 'trabalho' }), /stale_space_context/)
    assert.deepEqual(calls, [])
  }
})

test('D543: ACK depois da mudança de origem conserva recibo e retry não duplica bot', async () => {
  for (const change of ['host', 'link', 'epoch', 'space'] as const) {
    const { source, calls } = fixture()
    type Reply = { bot: { id: string; spaceId: string; name?: string } }
    let finish!: (v: Reply) => void
    source.connectedClient.call = async (method, body) => { calls.push([method, body]); return await new Promise<Reply>(r => { finish = r }) }
    const creation = new SpaceBotCreation(source, 'trabalho')
    const request = creation.create({ name: 'Criado', spaceId: 'trabalho' })
    const concurrent = creation.create({ name: 'Criado', spaceId: 'trabalho' })
    if (change === 'host') source.hello.hostId = 'hub-B'
    if (change === 'link') source.connectedClient = { ...source.connectedClient }
    if (change === 'epoch') source.principalEpoch++
    if (change === 'space') source.spaces.spaces = []
    finish({ bot: { id: 'novo', name: 'Criado', spaceId: 'trabalho' } })
    assert.equal((await request).id, 'novo', 'ACK válido não vira erro de origem')
    assert.equal((await concurrent).id, 'novo')
    assert.equal(creation.isCurrent(), false)
    assert.equal((await creation.create({ spaceId: 'trabalho' })).id, 'novo')
    assert.equal(calls.length, 1)
  }
})

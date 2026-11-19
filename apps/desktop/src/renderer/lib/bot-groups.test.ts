import assert from 'node:assert/strict'
import test from 'node:test'
import { groupOf, groupRoster, inGroup, renameIntent, UNGROUPED } from './bot-groups.ts'
import { BotGroupsModel } from '../store/bot-groups.ts'

const snapshot = { rev: 2, groups: [{ id: 'g', name: 'Pesquisa', bots: ['b', 'a'] }] }
test('grupos seguem a ordem do roster e fora de grupo é derivado', () => {
  const items = ['a', 'c', 'b'].map((id) => ({ bot: { id } }))
  assert.deepEqual(groupRoster(items, snapshot).map((g) => g.items.map((i) => i.bot.id)), [['c'], ['a', 'b']])
  assert.equal(groupOf(snapshot, 'c'), null)
  assert.equal(inGroup(snapshot, 'c', UNGROUPED), true)
})

test('modelo rejeita eventos de outro Space e revisões antigas', () => {
  const model = new BotGroupsModel(async () => { throw new Error('sem chamada') }, () => 'pessoal')
  model.receive({ type: 'hello', spaceId: 'pessoal', botGroups: snapshot })
  model.receive({ type: 'botGroups', spaceId: 'trabalho', snapshot: { rev: 9, groups: [] } })
  model.receive({ type: 'botGroups', spaceId: 'pessoal', snapshot: { rev: 1, groups: [] } })
  assert.deepEqual(model.snapshot, snapshot)
  model.receive({ type: 'hello', spaceId: 'pessoal' })
  assert.equal(model.snapshot, null)
})

test('nome gerado tenta sufixo ao receber name_taken', async () => {
  const names: string[] = []
  const model = new BotGroupsModel(async () => ({ call: async <T>(_method: string, body?: unknown): Promise<T> => {
    const name = (body as { name: string }).name
    names.push(name)
    if (names.length === 1) throw Object.assign(new Error('duplicado'), { code: 'name_taken' })
    return { snapshot, created: 'g' } as T
  } }), () => 'pessoal')
  const result = await model.addGenerated('Novo grupo')
  assert.deepEqual(names, ['Novo grupo', 'Novo grupo (2)'])
  assert.equal(result.created, 'g')
  assert.equal(model.error, null)
})

test('duas criações concorrentes usam nomes distintos após recusa do Hub', async () => {
  const names = new Set<string>()
  const model = new BotGroupsModel(async () => ({ call: async <T>(_method: string, body?: unknown): Promise<T> => {
    const name = (body as { name: string }).name
    if (names.has(name)) throw Object.assign(new Error('duplicado'), { code: 'name_taken' })
    names.add(name)
    return { snapshot: { rev: names.size, groups: [] }, created: name } as T
  } }), () => 'pessoal')
  const result = await Promise.all([model.addGenerated('Grupo'), model.addGenerated('Grupo')])
  assert.deepEqual(result.map((r) => r.created), ['Grupo', 'Grupo (2)'])
})

test('uma operação aguardando conexão não muda de Hub', async () => {
  let resolve!: (v: { call<T>(): Promise<T> }) => void
  let origin = 'um', called = false
  const waiting = new Promise<{ call<T>(): Promise<T> }>((r) => { resolve = r })
  const model = new BotGroupsModel(() => waiting, () => 'pessoal', () => origin)
  const operation = model.edit({ op: 'remove', id: 'g' })
  origin = 'dois'
  resolve({ call: async <T>() => { called = true; return {} as T } })
  await assert.rejects(operation, /stale_space_link/)
  assert.equal(called, false)
  assert.equal(model.error, null)
})

test('novo Hub descarta revisão anterior; Hub antigo desativa grupos', () => {
  const model = new BotGroupsModel(async () => { throw new Error('sem chamada') }, () => 'pessoal')
  model.receive({ type: 'hello', hostId: 'um', spaceId: 'pessoal', botGroups: snapshot })
  assert.equal(model.matchesOrigin('um', 'pessoal'), true)
  assert.equal(model.matchesOrigin('dois', 'pessoal'), false)
  model.receive({ type: 'hello', hostId: 'dois', spaceId: 'pessoal', botGroups: { rev: 1, groups: [] } })
  assert.equal(model.snapshot?.rev, 1)
  model.receive({ type: 'hello', hostId: 'dois', spaceId: 'pessoal' })
  assert.equal(model.snapshot, null)
})


test('hello com contador global menor aceita estado restaurado e impede respostas antigas', async () => {
  let finish!: (value: { snapshot: typeof snapshot }) => void
  const pending = new Promise<{ snapshot: typeof snapshot }>((resolve) => { finish = resolve })
  const model = new BotGroupsModel(async () => ({ call: <T>() => pending as Promise<T> }), () => 'pessoal')
  model.receive({ type: 'hello', hostId: 'h', spaceId: 'pessoal', rev: 100, botGroups: { ...snapshot, rev: 90 } })
  const edit = model.edit({ op: 'remove', id: 'g' })
  await new Promise<void>((resolve) => setImmediate(resolve))
  model.receive({ type: 'hello', hostId: 'h', spaceId: 'pessoal', rev: 10, botGroups: { ...snapshot, rev: 8 } })
  finish({ snapshot: { ...snapshot, rev: 91 } })
  await assert.rejects(edit, /stale_space_link/)
  assert.equal(model.snapshot?.rev, 8)
  model.receive({ type: 'botGroups', spaceId: 'pessoal', snapshot: { ...snapshot, rev: 7 } })
  assert.equal(model.snapshot?.rev, 8)
})

test('retry automático não publica name_taken, mas erro de nome digitado é mostrado', async () => {
  let done!: (value: unknown) => void
  let attempt = 0
  const model = new BotGroupsModel(async () => ({ call: async <T>() => {
    attempt++
    if (attempt === 1 || attempt === 3) throw Object.assign(new Error('duplicado'), { code: 'name_taken' })
    return await new Promise<T>((resolve) => { done = resolve as (value: unknown) => void })
  } }), () => 'pessoal')
  const creating = model.addGenerated('Grupo')
  await new Promise<void>((resolve) => setImmediate(resolve))
  assert.equal(model.error, null)
  done({ snapshot, created: 'g' })
  await creating
  await assert.rejects(model.edit({ op: 'rename', id: 'g', name: 'Grupo' }))
  const failure = model.error as { code?: string } | null
  assert.equal(failure?.code, 'name_taken')
})

test('sair do nome sem alteração não produz intenção de escrita', () => {
  assert.equal(renameIntent(snapshot, 'g', 'Pesquisa'), null)
  assert.equal(renameIntent(snapshot, 'ausente', 'Outro'), null)
  assert.deepEqual(renameIntent(snapshot, 'g', 'Outro'), { op: 'rename', id: 'g', name: 'Outro' })
})

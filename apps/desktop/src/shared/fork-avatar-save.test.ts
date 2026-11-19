import assert from 'node:assert/strict'
import test from 'node:test'
import { DEFAULT_CONFIG, confirmAvatar, normalizedAvatar, type MexeriquinhaConfig } from './fork-avatar-config.ts'
import { AvatarSaveQueue } from './fork-avatar-save.ts'
import { avatarEditorFor, retireAvatarEditors, type AvatarEditorStore } from '../renderer/fork/avatars/editor-store.ts'
import { normalizeBot } from './models.ts'

test('fila por bot mantém a última intenção ao fechar e reabrir durante save lento', async () => {
  let finishFirst!: () => void
  const slow = new Promise<void>((resolve) => { finishFirst = resolve })
  const writes: string[] = []
  const listeners = new Set<() => void>()
  const store: AvatarEditorStore = {
    bots: new Map([['a', normalizeBot({ id: 'a' })]]), connection: { kind: 'online' },
    subscribe: (listener) => { listeners.add(listener); return () => { listeners.delete(listener) } },
    saveAvatar: async (_, config) => {
      writes.push(normalizedAvatar(config).color)
      if (writes.length === 1) await slow
    },
  }
  const queue = avatarEditorFor(store, 'a', DEFAULT_CONFIG)
  queue.change({ ...DEFAULT_CONFIG, color: '#111111' })
  const first = queue.flush(true)
  queue.change({ ...DEFAULT_CONFIG, color: '#222222' })
  await queue.flush(true) // desmontagem: a fila segue sendo a mesma, com a primeira em voo
  const reopened = avatarEditorFor(store, 'a', DEFAULT_CONFIG)
  assert.equal(reopened, queue)
  reopened.receive(DEFAULT_CONFIG) // montagem com cópia salva ainda antiga
  assert.equal(normalizedAvatar(reopened.getSnapshot().value).color, '#222222')
  reopened.change({ ...DEFAULT_CONFIG, color: '#333333' })
  finishFirst()
  await first
  await reopened.flush(true)
  assert.deepEqual(writes, ['#111111', '#333333'])
  assert.equal(normalizedAvatar(queue.getSnapshot().value).color, '#333333')
  assert.equal(queue.getSnapshot().dirty, false)
  store.bots.delete('a')
  for (const listener of listeners) listener()
  queue.change({ ...DEFAULT_CONFIG, color: '#444444' })
  await queue.flush(true)
  assert.deepEqual(writes, ['#111111', '#333333'], 'remoção do bot descarta a fila')
  retireAvatarEditors(store)
})

test('resposta de Hub antigo mantém falha e pendência em vez de indicar Avatar salvo', async () => {
  const queue = new AvatarSaveQueue(DEFAULT_CONFIG, async (config) => confirmAvatar(undefined, config), true)
  queue.change({ ...DEFAULT_CONFIG, color: '#123456' })
  await queue.flush(true)
  assert.match(queue.getSnapshot().error!, /Atualize o Hub/)
  assert.equal(queue.getSnapshot().dirty, true)
  assert.equal(queue.getSnapshot().saving, false)
  queue.dispose()
})


test('fila v2 conserva intenção nested durante save lento e confirma profundamente', async () => {
  const initial = {v:2,estilo:'simples',aparencia:{cor:'laranja',olhos:'ponto',roupa:'nenhuma',acessorio:'nenhum',objeto:'nenhum'}} as const
  let finish!:()=>void
  const slow = new Promise<void>((ok)=>{finish=ok})
  const writes: unknown[]=[]
  const queue = new AvatarSaveQueue(initial, async (config)=>{
    writes.push(structuredClone(config))
    if (writes.length===1) await slow
    confirmAvatar(JSON.parse(JSON.stringify(config)),config)
  },true)
  const firstChoice=structuredClone(initial)
  queue.change(firstChoice)
  const first=queue.flush(true)
  const latest: MexeriquinhaConfig={...initial,aparencia:{...initial.aparencia,cor:'roxo'}}
  queue.change(latest)
  latest.aparencia.cor='laranja' // mutação posterior do consumidor não altera a intenção capturada
  queue.receive(initial)
  assert.equal((queue.getSnapshot().value as typeof initial).aparencia.cor,'roxo')
  finish(); await first; await queue.flush(true)
  assert.equal((writes[1] as typeof initial).aparencia.cor,'roxo')
  assert.equal(queue.getSnapshot().dirty,false)
  queue.dispose()
})

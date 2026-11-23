import assert from 'node:assert/strict'
import test from 'node:test'
import { clearWorkspaceStorage, workspaceStoragePrefix } from './storage.ts'

test('limpa caches do workspace sem tocar em outro workspace ou nas preferências', async () => {
  const prefix = workspaceStoragePrefix('org:a')
  const other = workspaceStoragePrefix('org:a:b')
  const keys = [`${prefix}tower`, `${other}draft`, 'appearanceTheme', `${prefix}approval`]
  const sessionKeys = [`${prefix}pending`, `${other}pending`]
  const stores = (data: string[]) => ({
    get length() { return data.length },
    key: (i: number) => data[i] ?? null,
    removeItem: (key: string) => { data.splice(data.indexOf(key), 1) },
  })
  const caches: string[] = []
  const databases: string[] = []
  const indexedDB = {
    databases: async () => [{ name: `${prefix}mirror` }, { name: `${other}mirror` }, {}],
    deleteDatabase: (name: string) => {
      databases.push(name)
      const request = {} as IDBOpenDBRequest
      queueMicrotask(() => request.onsuccess?.call(request, new Event('success')))
      return request
    },
  }
  await clearWorkspaceStorage('org:a', {
    localStorage: stores(keys),
    sessionStorage: stores(sessionKeys),
    caches: {
      keys: async () => [`${prefix}pwa`, `${other}pwa`, 'shell'],
      delete: async (name) => { caches.push(name); return true },
    },
    indexedDB,
  })
  assert.deepEqual(keys, [`${other}draft`, 'appearanceTheme'])
  assert.deepEqual(sessionKeys, [`${other}pending`])
  assert.deepEqual(caches, [`${prefix}pwa`])
  assert.deepEqual(databases, [`${prefix}mirror`])
})

test('falha fechada se o navegador bloquear a exclusão de IndexedDB', async () => {
  await assert.rejects(clearWorkspaceStorage('org:a', {
    indexedDB: {
      databases: async () => [{ name: `${workspaceStoragePrefix('org:a')}mirror` }],
      deleteDatabase: () => {
        const request = {} as IDBOpenDBRequest
        queueMicrotask(() => request.onblocked?.call(request, new Event('blocked') as IDBVersionChangeEvent))
        return request
      },
    },
  }), /workspace_storage_unavailable/)
})

test('não libera a troca se o navegador não puder enumerar o IndexedDB', async () => {
  const keysRemoved: string[] = []
  const key = `${workspaceStoragePrefix('org:a')}draft`
  await assert.rejects(clearWorkspaceStorage('org:a', {
    indexedDB: { deleteDatabase: () => { assert.fail('não tenta adivinhar nomes') } },
    localStorage: { length: 1, key: () => key, removeItem: (name) => { keysRemoved.push(name) } },
  }), /workspace_storage_unavailable/)
  assert.deepEqual(keysRemoved, [key])
})

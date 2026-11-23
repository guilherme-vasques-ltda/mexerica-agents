export function workspaceStoragePrefix(workspaceId: string): string {
  return `mx-remote-workspace:${encodeURIComponent(workspaceId)}:`
}

interface WorkspaceStorage {
  localStorage?: Pick<Storage, 'length' | 'key' | 'removeItem'>
  sessionStorage?: Pick<Storage, 'length' | 'key' | 'removeItem'>
  caches?: Pick<CacheStorage, 'keys' | 'delete'>
  indexedDB?: Pick<IDBFactory, 'databases' | 'deleteDatabase'>
}

export async function clearWorkspaceStorage(workspaceId: string, browser: WorkspaceStorage = globalThis): Promise<void> {
  const prefix = workspaceStoragePrefix(workspaceId)
  const tasks = await Promise.allSettled([
    ...(['localStorage', 'sessionStorage'] as const).map(async (name) => {
      const storage = browser[name]
      if (!storage) return
      for (let i = storage.length - 1; i >= 0; i--) {
        const key = storage.key(i)
        if (key?.startsWith(prefix)) storage.removeItem(key)
      }
    }),
    (async () => {
      const caches = browser.caches
      if (!caches) return
      for (const name of await caches.keys()) {
        if (name.startsWith(prefix)) await caches.delete(name)
      }
    })(),
    (async () => {
      const databases = browser.indexedDB
      if (!databases) return
      const names = (await databases.databases()).flatMap(({ name }) => name?.startsWith(prefix) ? [name] : [])
      const deletions = await Promise.allSettled(names.map((name) =>
        new Promise<void>((resolve, reject) => {
          const request = databases.deleteDatabase(name)
          request.onsuccess = () => resolve()
          request.onerror = request.onblocked = () => reject(new Error('workspace_storage_unavailable'))
        })))
      if (deletions.some((deletion) => deletion.status === 'rejected')) throw new Error('workspace_storage_unavailable')
    })(),
  ])
  if (tasks.some((task) => task.status === 'rejected')) throw new Error('workspace_storage_unavailable')
}

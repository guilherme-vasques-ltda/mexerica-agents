import type { AsyncSigner } from '../../shared/relay-protocol/protocol-internal.ts'

function openKeys(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open('mx-remote-device', 1)
    request.onupgradeneeded = () => request.result.createObjectStore('keys')
    let blocked = false
    request.onerror = () => reject(new Error('remote_key_storage'))
    request.onblocked = () => {
      blocked = true
      reject(new Error('remote_key_storage'))
    }
    request.onsuccess = () => {
      if (blocked) request.result.close()
      else resolve(request.result)
    }
  })
}

type RecordKey = { privateKey: CryptoKey; publicKey: CryptoKey; state: 'new' | 'paired' | 'revoked' }

export async function remoteDevice(partition: string, create: boolean) {
  const db = await openKeys()
  async function update<T>(mode: IDBTransactionMode, run: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
    return new Promise((resolve, reject) => {
      const tx = db.transaction('keys', mode)
      const request = run(tx.objectStore('keys'))
      tx.oncomplete = () => resolve(request.result)
      tx.onabort = () => reject(new Error('remote_key_storage'))
    })
  }
  try {
    let record = await update<RecordKey | undefined>('readonly', (store) => store.get(partition))
    if (!record && create) {
      const pair = await crypto.subtle.generateKey('Ed25519', false, ['sign', 'verify'])
      try {
        await update('readwrite', (store) => store.add({ ...pair, state: 'new' }, partition))
      } catch {
        if (!await update('readonly', (store) => store.get(partition))) throw new Error('remote_key_storage')
      }
      record = await update<RecordKey | undefined>('readonly', (store) => store.get(partition))
    }
    if (!record) return null
    if (record.privateKey.extractable || record.privateKey.type !== 'private' || record.privateKey.algorithm.name !== 'Ed25519') {
      throw new Error('remote_key_invalid')
    }
    const { privateKey, publicKey, state } = record
    const signer: AsyncSigner = {
      publicKey: new Uint8Array(await crypto.subtle.exportKey('raw', publicKey)),
      async sign(input) {
        return new Uint8Array(await crypto.subtle.sign('Ed25519', privateKey, Uint8Array.from(input)))
      },
    }
    return { signer, state }
  } finally {
    db.close()
  }
}

export async function markDevice(partition: string, state: RecordKey['state']): Promise<void> {
  const db = await openKeys()
  try {
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction('keys', 'readwrite')
      const store = tx.objectStore('keys')
      const request = store.get(partition)
      request.onsuccess = () => {
        const current: RecordKey | undefined = request.result
        if (current && current.state !== 'revoked') store.put({ ...current, state }, partition)
        else if (current?.state === 'revoked' && state === 'revoked') return
        else tx.abort()
      }
      tx.oncomplete = () => resolve()
      tx.onabort = () => reject(new Error('remote_key_storage'))
    })
  } finally {
    db.close()
  }
}

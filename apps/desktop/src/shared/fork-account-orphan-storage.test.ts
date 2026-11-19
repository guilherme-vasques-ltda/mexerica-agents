import assert from 'node:assert/strict'
import * as crypto from 'node:crypto'
import * as fs from 'node:fs/promises'
import * as path from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'
import { loadMainModule } from './test-support/main-module.ts'

test('troca de guard preserva o registro anterior quando publicação falha e rejeita configuração divergente', async () => {
  const dir = await fs.mkdtemp(path.join(tmpdir(), 'd171-account-orphan-'))
  let failGuardRename = false
  const io = { ...fs, rename: async (source: string, target: string) => {
    if (target.endsWith('/account.guard') && failGuardRename) throw new Error('fixture guard failure')
    await fs.rename(source, target)
  } }
  const { AccountStorage } = loadMainModule<typeof import('../main/fork-account/storage.ts')>('fork-account/storage', {
    'node:crypto': crypto, 'node:path': path, 'node:fs': { promises: io },
  }, { Buffer })
  try {
    const original = new AccountStorage(dir, 'config-a')
    const old = Buffer.from('sealed-pending-fixture')
    await original.quarantine(old, () => {})
    failGuardRename = true
    await assert.rejects(original.retain(Buffer.from('sealed-orphan-fixture'), 'active', () => {}))
    const before = await new AccountStorage(dir, 'config-a').load()
    assert.equal(before.kind, 'revocation_unknown')
    assert.deepEqual('sealed' in before ? before.sealed : null, old)
    failGuardRename = false
    await original.retain(Buffer.from('sealed-orphan-fixture'), 'active', () => {})
    const after = await new AccountStorage(dir, 'config-a').load()
    assert.equal(after.kind, 'active')
    assert.deepEqual('sealed' in after ? after.sealed : null, Buffer.from('sealed-orphan-fixture'))
    assert.equal((await new AccountStorage(dir, 'config-b').load()).kind, 'revocation_unknown')
  } finally { await fs.rm(dir, { recursive: true, force: true }) }
})

test('reabertura valida guard e digest após publicação incerta antes ou depois de rename', async () => {
  for (const failAfter of [false, true]) {
    const dir = await fs.mkdtemp(path.join(tmpdir(), 'd171-account-fence-'))
    let armed = false; let renamed = false
    const io = { ...fs,
      rename: async (source: string, target: string) => {
        if (armed && target.endsWith('/account.guard') && !failAfter) { armed = false; throw new Error('fixture before rename') }
        await fs.rename(source, target)
        if (armed && target.endsWith('/account.guard')) renamed = true
      },
      open: async (name: string, flags: string, mode?: number) => {
        const h = await fs.open(name, flags, mode)
        return { writeFile: (data: Buffer | string) => h.writeFile(data), close: () => h.close(), sync: async () => {
          if (armed && renamed && name === dir) { armed = false; throw new Error('fixture after rename') }
          await h.sync()
        } }
      },
    }
    const { AccountStorage } = loadMainModule<typeof import('../main/fork-account/storage.ts')>('fork-account/storage', {
      'node:crypto': crypto, 'node:path': path, 'node:fs': { promises: io },
    }, { Buffer })
    try {
      const storage = new AccountStorage(dir, 'config-a')
      await storage.save(Buffer.from('sealed-old-fixture'), () => {})
      armed = true
      await assert.rejects(storage.retain(Buffer.from('sealed-new-fixture'), 'active', () => {}))
      await storage.fence()
      const reopened = new AccountStorage(dir, 'config-a')
      const fenced = await reopened.load()
      assert.equal(fenced.kind, 'revocation_unknown')
      assert.equal(fenced.kind === 'revocation_unknown' && fenced.recoverable, true)
      assert.deepEqual('sealed' in fenced ? fenced.sealed : null, Buffer.from(failAfter ? 'sealed-new-fixture' : 'sealed-old-fixture'))
      await assert.rejects(reopened.resumeFence(() => { throw new Error('stale epoch') }))
      await reopened.resumeFence(() => {})
      assert.equal((await reopened.load()).kind, 'active')
    } finally { await fs.rm(dir, { recursive: true, force: true }) }
  }
})

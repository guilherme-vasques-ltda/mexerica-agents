import assert from 'node:assert/strict'
import test from 'node:test'
import * as crypto from 'node:crypto'
import * as path from 'node:path'
import { promises as fs } from 'node:fs'
import { tmpdir } from 'node:os'
import { loadMainModule } from './test-support/main-module.ts'

test('falha ao remover mantém lápide durável, informa erro e impede restauração após reabrir', async () => {
  const files = new Map<string, Buffer>()
  let refuseRemove = false
  const fakeFs = {
    mkdir: async () => {},
    readdir: async (dir: string) => [...files.keys()].filter(name => name.startsWith(`${dir}/`)).map(name => name.slice(dir.length + 1)),
    readFile: async (name: string, encoding?: string) => {
      const bytes = files.get(name)
      if (!bytes) throw new Error('absent')
      return encoding ? bytes.toString('utf8') : bytes
    },
    open: async (name: string, flags: string) => ({
      writeFile: async (data: string | Buffer) => { if (flags === 'wx') files.set(name, Buffer.from(data)) },
      sync: async () => {}, close: async () => {},
    }),
    rename: async (from: string, to: string) => { const data = files.get(from); if (!data) throw new Error('absent'); files.set(to, data); files.delete(from) },
    rm: async (name: string) => { if (refuseRemove && name.endsWith('/account.bin')) throw new Error('denied'); files.delete(name) },
  }
  const mod = loadMainModule<typeof import('../main/fork-account/storage.ts')>('fork-account/storage', {
    'node:crypto': crypto, 'node:fs': { promises: fakeFs }, 'node:path': path,
  }, { Buffer })
  const first = new mod.AccountStorage('/fixture', 'deployment-fixture')
  await first.save(Buffer.from('encrypted fixture'), () => {})
  assert.equal((await first.load()).kind, 'active')
  refuseRemove = true
  await assert.rejects(first.clear(), /denied/)
  const reopened = new mod.AccountStorage('/fixture', 'deployment-fixture')
  assert.equal((await reopened.load()).kind, 'empty')
  assert.equal((await new mod.AccountStorage('/fixture', 'other-deployment').load()).kind, 'empty')
  refuseRemove = false; await reopened.clear(); assert.equal(files.size, 0)
})

test('guard inválido impede carregar o vínculo cifrado', async () => {
  const mod = loadMainModule<typeof import('../main/fork-account/storage.ts')>('fork-account/storage', {
    'node:crypto': crypto, 'node:fs': { promises: { readFile: async () => '{invalid' } }, 'node:path': path,
  }, { Buffer })
  assert.equal((await new mod.AccountStorage('/fixture', 'fixture').load()).kind, 'revocation_unknown')
})

test('intenção de saída sobrevive reabertura e não pode voltar o guard a ativo', async () => {
  const dir = await fs.mkdtemp(path.join(tmpdir(), 'donna-account-exit-'))
  try {
    const mod = loadMainModule<typeof import('../main/fork-account/storage.ts')>('fork-account/storage', {
      'node:crypto': crypto, 'node:fs': { promises: fs }, 'node:path': path,
    }, { Buffer })
    const first = new mod.AccountStorage(dir, 'deployment-fixture')
    const sealed = Buffer.from('encrypted fixture')
    await first.save(sealed, () => {})
    await first.fence(true)
    // A later generic storage fence cannot erase the already committed exit intent.
    await first.fence()
    const reopened = new mod.AccountStorage(dir, 'deployment-fixture')
    assert.deepEqual({ ...(await reopened.load()) }, { kind: 'revocation_unknown', sealed, recoverable: true, exitRequested: true })
    const guard = await fs.readFile(path.join(dir, 'account.guard'))
    await assert.rejects(reopened.resumeFence(() => {}), /not recoverable/)
    assert.deepEqual(await fs.readFile(path.join(dir, 'account.guard')), guard)
    assert.deepEqual(await fs.readFile(path.join(dir, 'account.bin')), sealed)
  } finally { await fs.rm(dir, { recursive: true, force: true }) }
})

import assert from 'node:assert/strict'
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { officialAccount, officialAccountPlist, officialConfigHash } from './fork-official-account.ts'

const fields = {
  clerkPublishableKey: `pk_live_${Buffer.from('clerk.example.invalid$').toString('base64')}`,
  clerkFrontendOrigin: 'https://clerk.example.invalid',
  clerkProxyOrigin: 'https://auth.example.invalid',
  clerkTokenOrigin: 'https://app.example.invalid',
}
const plist = (values: Record<string, string>) => `<plist version="1.0"><dict>${Object.entries(values).map(([key, value]) => `<key>${key}</key><string>${value}</string>`).join('')}</dict></plist>`

test('official configuration contains only public account inputs and produces a canonical pin', () => {
  const config = officialAccountPlist(plist(fields))
  assert.deepEqual(config, { environment: 'main', ...fields })
  assert.equal(officialConfigHash(config!), officialConfigHash(officialAccount({ ...fields, environment: 'main' })!))
  assert.notEqual(officialConfigHash(config!), officialConfigHash(officialAccountPlist(plist({ ...fields, clerkProxyOrigin: 'https://other.example.invalid' }))!))
})

test('official configuration rejects absent, extra, duplicate and invalid input', () => {
  assert.equal(officialAccount(null), null)
  for (const invalid of [
    plist({}), plist({ ...fields, deploymentId: 'unexpected' }),
    plist(fields).replace('</dict>', '<key>clerkTokenOrigin</key><string>https://app.example.invalid</string></dict>'),
    plist({ ...fields, clerkPublishableKey: fields.clerkPublishableKey.replace('pk_live_', 'pk_test_') }),
    plist({ ...fields, clerkFrontendOrigin: 'https://other.example.invalid' }),
    plist({ ...fields, clerkProxyOrigin: 'https://auth.example.invalid/path' }),
    plist({ ...fields, clerkTokenOrigin: 'https://user@auth.example.invalid' }),
  ]) assert.equal(officialAccountPlist(invalid), null)
})

test('official packaging rejects mismatched compilation and missing native staging', async t => {
  const dir = await mkdtemp(join(tmpdir(), 'desktop-official-'))
  t.after(() => rm(dir, { recursive: true, force: true }))
  const builder = createRequire(import.meta.url)('../../tools/fork-official-builder.cjs')
  const context = { packager: { projectDir: dir }, electronPlatformName: 'linux' }
  const config = officialAccountPlist(plist(fields))!
  const stage = join(dir, 'build/official')
  const hash = join(stage, 'out/main/official-config.sha256')
  await mkdir(join(stage, 'out/main'), { recursive: true })
  await assert.rejects(builder.beforePack(context), /configuration does not match/)
  await writeFile(join(stage, 'account-config.json'), JSON.stringify(config))
  await writeFile(hash, '0'.repeat(64))
  await assert.rejects(builder.beforePack(context), /configuration does not match/)
  await writeFile(hash, officialConfigHash(config))
  await assert.rejects(builder.beforePack(context), /build\/native\/donnabot-host/)
  await mkdir(join(dir, 'build/native'))
  await writeFile(join(dir, 'build/native/donnabot-host'), 'fixture', { mode: 0o700 })
  await builder.beforePack(context)
})

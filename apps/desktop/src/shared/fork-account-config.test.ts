import assert from 'node:assert/strict'
import test from 'node:test'
import { mkdtempSync, mkdirSync, copyFileSync, writeFileSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { execFileSync } from 'node:child_process'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'

const host = 'identity.example.test'
const config = {
  environment: 'main',
  deploymentEnv: 'main',
  clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`,
  clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`,
  clerkIssuer: 'https://issuer.example.test',
  cloudURL: 'https://cloud.example.test',
  deploymentId: 'fixture',
  manifestHash: 'a'.repeat(64),
}

test('configuração exige pins explícitos e recusa payload não canônico ou outra origem', () => {
  assert.equal(accountConfig(config).kind, 'configured')
  const proxied = accountConfig(config)
  assert.equal(proxied.kind === 'configured' && proxied.proxyOrigin, 'https://auth.example.invalid')
  assert.equal(accountConfig({ ...config, clerkProxyOrigin: undefined }).kind, 'unavailable')
  assert.deepEqual(accountConfig({ ...config, clerkIssuer: null }), { kind: 'unavailable', reason: 'missing' })
  assert.deepEqual(accountConfig({ ...config, clerkTokenOrigin: undefined }), { kind: 'unavailable', reason: 'missing' })
  const production = accountConfig({ ...config, clerkTokenOrigin: 'https://app.example.invalid' })
  assert.equal(production.kind === 'configured' && production.tokenOrigin, 'https://app.example.invalid')
  for (const invalid of [
    { ...config, clerkProxyOrigin: 'https://different.example.test' },
    { ...config, clerkProxyOrigin: 'http://auth.example.test' },
    { ...config, clerkProxyOrigin: 'https://auth.example.test/path' },
    { ...config, clerkTokenOrigin: 'http://app.example.invalid' },
    { ...config, clerkTokenOrigin: 'https://user@app.example.invalid' },
    { ...config, clerkTokenOrigin: 'https://app.example.invalid/path' },
    { ...config, clerkTokenOrigin: 'https://app.example.invalid?token=x' },
    { ...config, clerkTokenOrigin: 'https://app.example.invalid:8443' },
    { ...config, clerkPublishableKey: `${config.clerkPublishableKey}!` },
    { ...config, clerkFrontendOrigin: 'https://other.example.test' },
    { ...config, cloudURL: 'https://user@cloud.example.test' },
    { ...config, cloudURL: 'https://cloud.example.test/path' },
  ]) assert.equal(accountConfig(invalid).kind, 'unavailable')
})


test('gerador transporta a origem explícita de cada ambiente; ausência continua fechada', () => {
  const root = mkdtempSync(join(tmpdir(), 'donna-account-config-'))
  try {
    const desktop = join(root, 'apps/desktop'), configs = join(desktop, 'config')
    mkdirSync(join(desktop, 'tools'), { recursive: true }); mkdirSync(join(desktop, 'resources')); mkdirSync(configs, { recursive: true })
    const script = join(desktop, 'tools/account-config.mjs')
    copyFileSync(new URL('../../tools/account-config.mjs', import.meta.url), script)
    for (const environment of ['dev', 'main']) {
      const values = { ...config, clerkTokenOrigin: environment === 'main' ? 'https://app.example.invalid' : 'https://dev.example.test', deploymentEnv: environment }
      const plist = (input: Record<string, unknown>) => '<plist><dict>' + Object.entries(input).map(([k, v]) => `<key>${k}</key>\n<string>${v}</string>`).join('\n') + '</dict></plist>'
      writeFileSync(join(configs, `${environment}.plist`), plist(values))
      execFileSync(process.execPath, [script, environment])
      const generated = JSON.parse(readFileSync(join(desktop, 'resources/account-config.json'), 'utf8'))
      assert.equal(generated.clerkTokenOrigin, values.clerkTokenOrigin)
      assert.equal(accountConfig(generated).kind, 'configured')
      const { clerkTokenOrigin: _removed, ...without } = values
      writeFileSync(join(configs, `${environment}.plist`), plist(without))
      execFileSync(process.execPath, [script, environment])
      assert.equal(accountConfig(JSON.parse(readFileSync(join(desktop, 'resources/account-config.json'), 'utf8'))).kind, 'unavailable')
    }
  } finally { rmSync(root, { recursive: true, force: true }) }
})

test('retorno OAuth usa proxy quando aponta à FAPI fixada', () => {
  const pinned = accountConfig(config)
  assert.equal(accountBrowserURL(`https://${host}/oauth?state=a`, pinned), 'https://auth.example.invalid/oauth?state=a')
  assert.equal(accountBrowserURL('https://provider.example.test/oauth', pinned), 'https://provider.example.test/oauth')
  assert.throws(() => accountBrowserURL('http://provider.example.test/', pinned))
})

import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import * as protocol from './personal-protocol.ts'
import type { Principal } from './principal.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'

const vector = JSON.parse(readFileSync(new URL('../../../../../docs/reference/fixtures/remote-relay-vectors.json', import.meta.url), 'utf8')).personalBootstrap

test('PL1 C1: só constante do build admite manifesto; época/contexto divergentes e M2 recusam', () => {
  const c = vector.context
  const p = { deployment: { env: c[2], origin: c[3], issuer: c[4], deploymentId: c[5], manifestHash: c[6] }, current: () => true } as Principal
  function status(manifest: unknown, principal = p) {
    // Dependência privada do teste substitui o build; não existe setter no runtime.
    const module = loadMainModule<typeof import('./personal-authority.ts')>('fork-cloud/personal-authority', {
      '../environment': { buildConfig: { environment: 'dev', deploymentManifest: manifest } },
      './personal-protocol': protocol,
    }, { Buffer })
    return module.personalBootstrapStatus(principal)
  }
  assert.equal(status(null).reason, 'updateRequired')
  assert.equal(status(vector.manifest).reason, 'helperRequired')
  assert.equal(status(vector.manifest).available, false)
  assert.equal(status(vector.manifest, { ...p, current: () => false }).reason, 'updateRequired')
  assert.equal(status(vector.manifest, { ...p, deployment: { ...p.deployment, manifestHash: '0'.repeat(64) } }).reason, 'updateRequired')
  assert.equal(status({ ...vector.manifest, schema: 2, schemaVersion: 6 }).reason, 'updateRequired')
  assert.equal(status(null, { ...p, manifest: vector.manifest } as Principal).reason, 'updateRequired')
})

test('PL1 C1: serviço real projeta indisponibilidade sem iniciar rede ou cerimônia', async () => {
  let current = true, sends = 0
  const observation = { profile: 'personal-linux-v1', wire: 1, available: false, reason: 'updateRequired' } as const
  const principal = { seal: { epoch: 1, userId: 'fixture' }, namespace: 'fixture', current: () => current, bootstrapAvailable: false, personalBootstrapStatus: observation }
  const account = { sessionEpoch: 1, on() {}, off() {}, verifiedDeviceBinding: () => null }
  const { ForkCloudService } = loadMainModule<typeof import('./service.ts')>('fork-cloud/service', {
    'node:crypto': {}, electron: {}, './personal-runtime': {}, './principal': { capturePrincipal: () => current ? principal : null },
    './account-api': {}, './device-keys': {}, './crypto': {}, './enroll': {}, './connection': {}, './pins': {},
  }, { Buffer, Object, process })
  const service = new ForkCloudService(account as never, { registerDevice: async () => { sends++; return { kind: 'unavailable' } } } as never, { captureLocal: () => null })
  assert.deepEqual((await service.invoke({ kind: 'state' })).personalBootstrap, observation)
  assert.equal(sends, 0)
  current = false
  assert.equal((await service.invoke({ kind: 'state' })).personalBootstrap, undefined)
  service.dispose()
})

import { test } from 'node:test'
import assert from 'node:assert/strict'
import { admitsCloudBuild } from './fork-cloud-build.ts'
import { emptyMainRelease, hash, productionRelease, release } from './fork-cloud-build.fixture.ts'
import { forkFeatures } from './fork-features.ts'

test('only a complete matching Dev release makes the cloud module available', () => {
  const { input, manifest } = release('dev', 'https://clerk.example.test', 'https://cloud.example.test', 'https://clerk.example.test')
  assert.equal(forkFeatures.cloudAccess, false) // An ordinary Node/unconfigured build stays closed.
  assert.equal(admitsCloudBuild(input), true)
  assert.equal(admitsCloudBuild({}), false)
  assert.equal(admitsCloudBuild({ ...input, environment: 'main', deploymentEnv: 'main' }), false)
  assert.equal(admitsCloudBuild({ ...input, deploymentManifest: null }), false)
  assert.equal(admitsCloudBuild({ ...input, manifestHash: '0'.repeat(64) }), false)
  assert.equal(admitsCloudBuild({ ...input, deploymentManifest: { ...manifest, bootstrap: null } }), false)
})

test('the main variant opens the account only with a qualified production config; the empty repo plist stays closed', () => {
  const { input, manifest, key } = productionRelease()
  assert.equal(admitsCloudBuild(input), true)

  assert.equal(admitsCloudBuild(emptyMainRelease), false)
  assert.equal(admitsCloudBuild(null), false)

  const devManifest = { ...manifest, env: 'dev' }
  const dev = release('dev', 'https://clerk.example.test', 'https://cloud.example.test', 'https://clerk.example.test')
  for (const [label, refused] of Object.entries({
    'main with a development key': { ...input, clerkPublishableKey: key('pk_test_') },
    'Dev with a production key': { ...dev.input, clerkPublishableKey: dev.key('pk_live_') },
    'M from another environment': { ...input, deploymentManifest: devManifest, manifestHash: hash(devManifest) },
    'different deployment env': { ...input, deploymentEnv: 'dev' },
    'empty field': { ...input, cloudURL: '' },
    'divergent hash': { ...input, manifestHash: '0'.repeat(64) },
    'unknown environment': { ...input, environment: 'prod', deploymentEnv: 'prod' },
  })) assert.equal(admitsCloudBuild(refused), false, label)
})

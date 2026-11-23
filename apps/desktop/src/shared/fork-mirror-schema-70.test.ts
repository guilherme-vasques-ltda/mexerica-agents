import assert from 'node:assert/strict'
import test from 'node:test'
import { isCurrentMirrorSchema, MIRROR_SCHEMA, mirrorCacheStamp } from './fork-mirror-cache.ts'

test('MIRROR_SCHEMA 73: o stamp novo vale e o cache anterior é descartado', () => {
  assert.equal(MIRROR_SCHEMA, 73)
  assert.equal(mirrorCacheStamp('1.0', '1.0'), `donnabot-mirror-v${MIRROR_SCHEMA}/1.0/1.0`)
  assert.equal(isCurrentMirrorSchema(`donnabot-mirror-v${MIRROR_SCHEMA - 1}/1.0/1.0`), false, 'cache do schema anterior não é tratado como atual')
  assert.equal(isCurrentMirrorSchema(mirrorCacheStamp('1.0', '1.0')), true)
})

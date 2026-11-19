import assert from 'node:assert/strict'
import { test } from 'node:test'
import { maySendSetupFailure } from './analytics.ts'

test('falhas de instalação nunca podem ser enviadas', () => {
  assert.equal(maySendSetupFailure(true, true), false)
  assert.equal(maySendSetupFailure(undefined, false), false)
  assert.equal(maySendSetupFailure(false, false), false)
  assert.equal(maySendSetupFailure(false, true), false)
  assert.equal(maySendSetupFailure(undefined, true), false, 'an older app never kept the host’s setting')
})

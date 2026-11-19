import assert from 'node:assert/strict'
import test from 'node:test'
import { accountCallback } from '../main/fork-account/callback.ts'

test('callback pertence à tentativa capturada e não aceita valores duplicados', () => {
  const expected = 'app.fixture://callback?attempt=fixture-a'
  assert.equal(accountCallback(`${expected}&rotating_token_nonce=fixture-nonce`, expected).searchParams.get('rotating_token_nonce'), 'fixture-nonce')
  for (const actual of ['app.fixture://callback?attempt=fixture-b', 'app.fixture://other?attempt=fixture-a', 'app.fixture://callback?attempt=fixture-a&attempt=fixture-a', 'app.fixture://callback?attempt=fixture-a&rotating_token_nonce=a&rotating_token_nonce=b', 'app.fixture://callback?attempt=fixture-a#fragment']) {
    assert.throws(() => accountCallback(actual, expected))
  }
})

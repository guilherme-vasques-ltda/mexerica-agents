import assert from 'node:assert/strict'
import { test } from 'node:test'
import { checkVersions, isBelow, MIN_HOST, parseVersion } from './compat.ts'

test('versions parse like the host and the iPhone app', () => {
  assert.deepEqual(parseVersion('v2.6'), [2, 6, 0])
  assert.deepEqual(parseVersion('2.6.3-beta+1'), [2, 6, 3])
  assert.equal(parseVersion('2.x'), null)
  assert.equal(isBelow('2.6.3', '2.7.0'), true)
  assert.equal(isBelow('2.10.0', '2.9.9'), false)
  assert.equal(isBelow('garbage', '9.9.9'), false)
})

test('the side that must update', () => {
  assert.deepEqual(checkVersions('2.6.0', '2.7.0', '2.7.0'), { kind: 'updateApp', minimum: '2.7.0' })
  assert.deepEqual(checkVersions('2.20.1', '2.20.0', null), { kind: 'updateHost', version: '2.20.0', minimum: '2.20.1' })
  assert.equal(checkVersions('2.20.1', '2.20.1', '2.3.0'), null)
})

test('the Mexerica line (0.x) comes after the inherited one (2.x)', () => {
  assert.equal(isBelow('0.1.0', '2.20.1'), false)
  assert.equal(isBelow('2.20.3', '0.1.0'), true)
  assert.equal(isBelow('0.1.0', '0.2.0'), true)
  assert.equal(isBelow('2.19.0', '2.20.1'), true)
  const minApp = '2.20.3' // host/src/compat.rs MIN_APP
  assert.equal(MIN_HOST, '2.20.1')
  assert.equal(checkVersions('0.1.0', '0.1.0', minApp), null)
  assert.equal(checkVersions('0.1.0', '2.20.3', minApp), null)
  assert.equal(checkVersions('2.20.3', '0.1.0', minApp), null)
  assert.deepEqual(checkVersions('2.19.0', '0.1.0', minApp), { kind: 'updateApp', minimum: '2.20.3' })
  assert.deepEqual(checkVersions('0.1.0', '2.18.0', minApp), { kind: 'updateHost', version: '2.18.0', minimum: '2.20.1' })
  assert.equal(checkVersions('dev', '0.1.0', minApp), null)
})

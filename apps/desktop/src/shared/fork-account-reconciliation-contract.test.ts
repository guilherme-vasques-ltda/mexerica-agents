import assert from 'node:assert/strict'
import test from 'node:test'
import { findDeviceForKey, noDeviceForKey, terminalSession } from '../main/fork-account/reconciliation.ts'

test('término exige sessão exata e estado terminal explícito', () => {
  assert.equal(terminalSession({ response: { id: 's-a', status: 'expired' } }, 's-a'), true)
  for (const item of [{}, { response: { id: 's-b', status: 'removed' } }, { response: { id: 's-a', status: 'active' } }, { response: { sessions: [] } }]) assert.equal(terminalSession(item, 's-a'), false)
})

test('recuperação usa uma única chave exata, sem escolher nome ou primeiro', () => {
  const device = { deviceId: 'dev_a', deviceKey: 'key', revoked: false }
  assert.deepEqual(findDeviceForKey({ devices: [device] }, 'key'), { id: 'dev_a', revoked: false })
  assert.equal(findDeviceForKey({ devices: [] }, 'key'), null)
  assert.equal(findDeviceForKey({ devices: [device, { ...device, deviceId: 'dev_b' }] }, 'key'), null)
  assert.equal(noDeviceForKey({ devices: [device, { ...device, deviceId: 'dev_b' }] }, 'key'), false)
  assert.equal(noDeviceForKey({ devices: [] }, 'key'), true)
})

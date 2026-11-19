import assert from 'node:assert/strict'
import { test } from 'node:test'
import { sandboxEnvironments } from './dev-sandbox.ts'
import { envAlias } from './env-alias.ts'

test('#915 npm run dev: a porta entregue ao app é a do host do sandbox, mesmo com DONNABOT_PORT velha no shell', () => {
  const base = { DONNABOT_PORT: '1', PATH: '/bin' }
  const { hostArgs, hostEnv, appEnv } = sandboxEnvironments(base, { home: '/h/.mexerica-dev-x', port: 19431, cloud: false, cdp: 29431 })
  assert.deepEqual(hostArgs, ['serve', '--port', '19431'])
  assert.equal(envAlias(appEnv, 'MEXERICA_PORT', 'DONNABOT_PORT', () => {}), String(hostArgs[2]))
  assert.equal(appEnv.MEXERICA_HOME, '/h/.mexerica-dev-x')
  assert.equal(appEnv.CODYNC_REMOTE_DEBUG, '29431')
  assert.equal(hostEnv.CODYNC_CLOUD, 'off')
  assert.equal(sandboxEnvironments({}, { home: '/h', port: 19430, cloud: true, cdp: null }).hostEnv.CODYNC_CLOUD, undefined)
})

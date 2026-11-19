import assert from 'node:assert/strict'
import { test } from 'node:test'
import { isCodyncPort } from './environment.ts'
import { sandboxLabel, sandboxName, sandboxPorts } from './dev-sandbox.ts'

test('sandbox ports skip the ones other sandboxes hold and never reach Codync, Codync Dev or Donnabot', () => {
  const ports = sandboxPorts([19430, 19432])
  assert.deepEqual(ports.slice(0, 2), [19431, 19433])
  assert.equal(ports.length, 68)
  assert.ok(ports.every((port) => port >= 19430 && port <= 19499 && !isCodyncPort(port)))
})

test('a sandbox is named after its worktree folder', () => {
  assert.equal(sandboxName('feat details/panel'), 'feat-details-panel')
  assert.equal(sandboxLabel('Donnabot Dev', '/Users/usuario/.mexerica-dev-feat-x'), 'Donnabot Dev (feat-x)')
  assert.equal(sandboxLabel('Donnabot Dev', 'C:\\Users\\me\\box\\'), 'Donnabot Dev (box)')
})

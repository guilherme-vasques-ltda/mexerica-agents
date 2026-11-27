import assert from 'node:assert/strict'
import test from 'node:test'
import { ABAS_DA_OPEN, abaDaOpen } from './fork-open-abas.ts'

test('Open: só as abas que rodam em texto no Hub isolado', () => {
  assert.deepEqual([...ABAS_DA_OPEN], ['identity', 'engine', 'memory', 'notifications', 'export'])
  for (const aba of ['behaviour', 'tools', 'whatsapp', 'voice']) assert.equal(abaDaOpen(aba), false, aba)
})

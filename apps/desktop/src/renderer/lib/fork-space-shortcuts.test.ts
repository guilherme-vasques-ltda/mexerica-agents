import assert from 'node:assert/strict'
import test from 'node:test'
import { spaceShortcut } from './fork-space-shortcuts.ts'
const base = { key: '', metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, repeat: false, isComposing: false, defaultPrevented: false }
const ids = ['pessoal', 'a', 'b', 'c']
test('E2: atalhos da janela não dependem da barra lateral e preservam a ordem dos Spaces', () => {
  assert.deepEqual(spaceShortcut({ ...base, key: '2', metaKey: true }, true, ids, null), { id: 'a' })
  assert.deepEqual(spaceShortcut({ ...base, key: 'K', metaKey: true, shiftKey: true }, true, ids, 'a'), { open: true })
  assert.deepEqual(spaceShortcut({ ...base, key: 'Tab', ctrlKey: true }, true, ids, 'a'), { id: 'b' })
  assert.deepEqual(spaceShortcut({ ...base, key: 'Tab', ctrlKey: true, shiftKey: true }, true, ids, null), { id: 'c' })
  for (const extra of [{ key: '9' }, { isComposing: true }, { defaultPrevented: true }, { repeat: true }, { ctrlKey: true }]) {
    assert.equal(spaceShortcut({ ...base, key: '2', metaKey: true, ...extra }, true, ids, null), null)
  }
})

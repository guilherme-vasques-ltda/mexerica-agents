import assert from 'node:assert/strict'
import test from 'node:test'
import { semDesinstalados } from './desinstalados.ts'

test('favorito de motor desinstalado sai; os demais ficam', () => {
  const favoritos = ['claude/opus', 'codex/gpt', 'outro/x']
  assert.deepEqual(semDesinstalados(favoritos, [{ id: 'claude', installed: true }, { id: 'codex', installed: false }]), ['claude/opus', 'outro/x'])
})

test('sem confirmação de desinstalação, a lista não muda', () => {
  const favoritos = ['claude/opus']
  assert.equal(semDesinstalados(favoritos, []), favoritos)
  assert.equal(semDesinstalados(favoritos, [{ id: 'claude' }]), favoritos)
})

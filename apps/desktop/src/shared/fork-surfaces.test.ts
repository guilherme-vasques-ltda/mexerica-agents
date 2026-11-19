import assert from 'node:assert/strict'
import { test } from 'node:test'
import { accountConfirmed, canPresentSurfaces } from './fork-surfaces.ts'

const confirmed = { ready: true, busy: false, user: { id: 'u' } }
const pending = [
  { ready: false, busy: false, user: null },
  { ready: true, busy: false, user: null },
  { ready: true, busy: true, user: { id: 'u' } },
  { ready: false, busy: false, user: { id: 'u' } },
]

test('sem sistema de login, ilha e bandeja aparecem mesmo sem sessão; o pareamento continua exigindo conta', () => {
  for (const account of pending) {
    assert.equal(canPresentSurfaces(account, false), true)
    assert.equal(accountConfirmed(account), false)
  }
  assert.equal(canPresentSurfaces(confirmed, false), true)
})

test('com login, ilha, bandeja e pareamento só aparecem com a conta confirmada', () => {
  for (const account of pending) {
    assert.equal(canPresentSurfaces(account, true), false)
    assert.equal(accountConfirmed(account), false)
  }
  assert.equal(canPresentSurfaces(confirmed, true), true)
  assert.equal(accountConfirmed(confirmed), true)
})

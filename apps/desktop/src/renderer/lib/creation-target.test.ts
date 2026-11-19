import { test } from 'node:test'
import assert from 'node:assert/strict'
import { canCreateOn, requireCreationStore, resolveCreationStore } from './creation-target.ts'

// Fatia 1 (D454): a mensagem segue o idioma do <html>; o texto conferido aqui é o pt-BR.
(globalThis as { document?: unknown }).document ??= { documentElement: { lang: 'pt-BR' } }

const store = (id: string) => ({ computer: { id }, connectedClient: {} as object | null })

test('C2: alvo explícito prevalece sobre seleção e nunca cai em outro Hub', () => {
  const alfa = store('alfa'), beta = store('beta')
  assert.equal(resolveCreationStore([alfa, beta], [alfa], 'beta'), beta)
  beta.connectedClient = null
  assert.equal(resolveCreationStore([alfa, beta], [alfa], 'beta'), undefined)
  assert.equal(resolveCreationStore([alfa], [alfa], 'ausente'), undefined)
  assert.equal(resolveCreationStore([alfa], [alfa], ''), undefined)
})

test('criação global mantém seleção, local e primeiro disponível; nenhum se todos offline', () => {
  const alfa = store('alfa'), beta = store('beta')
  assert.equal(resolveCreationStore([alfa, beta], [beta, alfa]), beta)
  beta.connectedClient = null
  assert.equal(resolveCreationStore([alfa, beta], [beta, alfa]), alfa)
  assert.equal(resolveCreationStore([alfa, beta], []), alfa)
  alfa.connectedClient = null
  assert.equal(resolveCreationStore([alfa, beta], [beta, alfa]), undefined)
})

test('disponibilidade usa o link utilizável, inclusive queda em tolerância ou mismatch', () => {
  const beta = store('beta')
  assert.ok(canCreateOn(beta))
  beta.connectedClient = null
  assert.equal(canCreateOn(beta), false)
  assert.equal(canCreateOn(null), false)
})

test('confirmação relê o alvo e recusa queda, remoção ou substituição sem migrar a criação', () => {
  const beta = store('beta')
  const check = () => requireCreationStore(beta, beta)
  assert.doesNotThrow(check)
  beta.connectedClient = null
  assert.throws(check, /Hub escolhido está indisponível/)
  assert.throws(() => requireCreationStore(null, beta), /Hub escolhido/)
  assert.throws(() => requireCreationStore(store('beta'), beta), /Hub escolhido/)
  beta.connectedClient = {}
  assert.doesNotThrow(check)
})

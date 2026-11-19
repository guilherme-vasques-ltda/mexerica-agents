import assert from 'node:assert/strict'
import { test } from 'node:test'
import { readSuggestions, suggestionOutcome } from './sugestoes.ts'

test('D389 Sugestões: computador antigo é null; item fora do contrato é omitido, não inventado', () => {
  assert.equal(readSuggestions(undefined), null)
  const rows = readSuggestions([
    { id: 's1', action: 'remember', text: 'Mora em Curitiba', file: null, shared: true, origin: null, nature: 'said', createdAt: 7 },
    { id: 's2', action: 'note', text: null, file: 'casa.md', shared: false, origin: null, nature: 'inferred', createdAt: 8 },
    { id: 's3', action: 'grant', text: 'x' },
    { action: 'remember', text: 'sem id' },
  ])
  assert.deepEqual(rows?.map((r) => [r.id, r.action, r.text, r.file, r.shared]), [['s1', 'remember', 'Mora em Curitiba', null, true], ['s2', 'note', null, 'casa.md', false]])
})

test('Sono 2b1c: a confirmação do atalho literal aparece para o dono aprovar', () => {
  const rows = readSuggestions([{ id: 's4', action: 'confirm', text: 'Mora em Curitiba', file: null, shared: false, origin: null, nature: 'said', createdAt: 9 }])
  assert.deepEqual(rows?.map((r) => [r.id, r.action, r.text, r.nature]), [['s4', 'confirm', 'Mora em Curitiba', 'said']])
})

test('D389 resultado de aprovar/descartar vem do Hub; memória alterada vira aviso, não sobrescrita', () => {
  assert.equal(suggestionOutcome(true, { status: 'approved' }), 'approved')
  assert.equal(suggestionOutcome(true, { status: 'noChange' }), 'noChange')
  assert.equal(suggestionOutcome(false, { status: 'discarded' }), 'discarded')
  assert.equal(suggestionOutcome(true, { status: 'pending' }), 'error')
  assert.equal(suggestionOutcome(true, null, new Error('memory_suggestion_stale: A memória mudou')), 'stale')
  assert.equal(suggestionOutcome(false, null, new Error('timeout')), 'error')
})

test('Sono 2b1d-2: a confirmação pela mensagem do dono (reclassify) aparece com a natureza confirmed', () => {
  const rows = readSuggestions([{ id: 's5', action: 'reclassify', text: 'Prefere café sem açúcar', file: null, shared: false, origin: null, nature: 'confirmed', createdAt: 10 }])
  assert.deepEqual(rows?.map((r) => [r.id, r.action, r.text, r.nature]), [['s5', 'reclassify', 'Prefere café sem açúcar', 'confirmed']])
})

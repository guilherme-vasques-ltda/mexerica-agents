import assert from 'node:assert/strict'
import { test } from 'node:test'
import { nonSavedReason, readNonSaved, type MemoryNonSaved, type NonSavedReason } from './non-saved.ts'

const record = (reason: NonSavedReason = 'duplicate'): MemoryNonSaved => ({
  id: `audit-${reason}`, jobId: 'job', reason, at: 1793718000000, messageIds: ['human'], responseIds: [],
  status: reason === 'technicalDeferred' || reason === 'noSelection' ? 'deferred' : 'notSaved',
  availability: reason === 'technicalDeferred' ? 'transient' : ['full', 'expired', 'noSelection', 'ineligible'].includes(reason) ? reason as 'full' | 'expired' | 'noSelection' | 'ineligible' : null,
})

test('D259 aceita só as oito disposições do Hub, conserva ordem e adiamento como ocorrência passada', () => {
  const source = (Object.keys(nonSavedReason) as NonSavedReason[]).map(record)
  const result = readNonSaved(source)
  assert.equal(result.state, 'ready')
  if (result.state !== 'ready') return
  assert.deepEqual(result.rows.map((row) => row.reason), source.map((row) => row.reason))
  assert.deepEqual(result.rows.filter((row) => row.status === 'deferred').map((row) => row.reason), ['technicalDeferred', 'noSelection'])
  assert.equal(result.rows.length, 8)
})

test('D259 campo ausente é indisponível; vazio confirmado difere de resposta inválida/desconhecida', () => {
  assert.deepEqual(readNonSaved(undefined), { state: 'unavailable' })
  assert.deepEqual(readNonSaved([]), { state: 'ready', rows: [] })
  for (const bad of [null, {}, 'timeout', Array.from({ length: 51 }, () => record())]) {
    assert.deepEqual(readNonSaved(bad), { state: 'unknown' })
  }
})

test('D259 enum futuro, motivo ausente e combinação incompatível são omitidos sem inferir causa', () => {
  const bad = [
    { ...record(), reason: 'future' }, { ...record(), reason: undefined },
    { ...record(), status: 'pending' }, { ...record(), availability: 'future' },
    { ...record('technicalDeferred'), availability: null },
    { ...record('expired'), status: 'deferred' },
    { automatic: { status: 'noChange', availability: 'full' } },
  ]
  assert.deepEqual(readNonSaved(bad), { state: 'ready', rows: [] })
  assert.deepEqual(readNonSaved([...bad, record()]), readNonSaved([record()]))
})

test('D259 projeta somente rótulos/data/identificador interno; não transporta corpo, hash ou refs', () => {
  const raw = { ...record(), text: 'SEGREDO', reasonDetail: 'trecho externo', hash: 'hash-candidato', targetBot: 'outro', targetMemoryId: 'alvo', responseIds: [] }
  assert.deepEqual(readNonSaved([raw]), { state: 'ready', rows: [{ id: raw.id, reason: 'duplicate', status: 'notSaved', at: raw.at }] })
  assert.equal(JSON.stringify(readNonSaved([raw])).includes('SEGREDO'), false)
  assert.deepEqual(readNonSaved([{ ...record(), messageIds: [], responseIds: [] }]), readNonSaved([record()]))
})

test('D259 data legada zero é preservada sem inventar; metadados malformados e IDs repetidos são omitidos', () => {
  const zero = { ...record(), at: 0 }
  assert.deepEqual(readNonSaved([zero]), { state: 'ready', rows: [{ id: zero.id, reason: 'duplicate', status: 'notSaved', at: 0 }] })
  for (const bad of [{ at: NaN }, { at: -1 }, { at: 9e15 }, { messageIds: null }, { responseIds: [1] }, { jobId: '' }, { id: '' }]) {
    assert.deepEqual(readNonSaved([{ ...record(), ...bad }]), { state: 'ready', rows: [] })
  }
  assert.deepEqual(readNonSaved([record(), record()]), readNonSaved([record()]))
})

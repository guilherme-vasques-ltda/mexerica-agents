import assert from 'node:assert/strict'
import { test } from 'node:test'
import { openingOmittedReason, readOpeningOmitted, type MemoryOpeningOmitted, type OpeningOmittedReason } from './opening-omitted.ts'

const record = (reason: OpeningOmittedReason = 'overBudget', layer: 'yours' | 'bot' = 'bot', entry = 7): MemoryOpeningOmitted => ({
  entry, layer, reason, at: 1793718000000,
})

test('D263 aceita só os três motivos do Hub, conserva ordem e deduplica por camada+entrada', () => {
  const source = [
    record('overBudget', 'bot', 7), record('pinnedLimit', 'bot', 8), record('scope', 'yours', 9),
    record('overBudget', 'yours', 7), // mesma entrada 7 na outra camada: chave distinta
    record('pinnedLimit', 'bot', 7), // mesmo bot:7 de novo, motivo divergente: a primeira vale
  ]
  const result = readOpeningOmitted(source)
  assert.equal(result.state, 'ready')
  if (result.state !== 'ready') return
  assert.deepEqual(result.rows.map((row) => `${row.layer}:${row.entry}:${row.reason}`), ['bot:7:overBudget', 'bot:8:pinnedLimit', 'yours:9:scope', 'yours:7:overBudget'])
  assert.equal(result.rows.length, 4)
})

test('D263 campo ausente é indisponível; vazio confirmado difere de resposta inválida/desconhecida', () => {
  assert.deepEqual(readOpeningOmitted(undefined), { state: 'unavailable' })
  assert.deepEqual(readOpeningOmitted([]), { state: 'ready', rows: [] })
  for (const bad of [null, {}, 'timeout', Array.from({ length: 501 }, () => record())]) {
    assert.deepEqual(readOpeningOmitted(bad), { state: 'unknown' })
  }
})

test('D263 motivo futuro, camada estranha e metadados malformados são omitidos sem inferir causa', () => {
  const bad = [
    { ...record(), reason: 'future' }, { ...record(), reason: undefined },
    { ...record(), layer: 'space' }, { ...record(), layer: undefined },
    { automatic: { status: 'noChange' } },
  ]
  assert.deepEqual(readOpeningOmitted(bad), { state: 'ready', rows: [] })
  assert.deepEqual(readOpeningOmitted([...bad, record()]), readOpeningOmitted([record()]))
})

test('D263 projeta somente id/camada/motivo/data; não transporta conteúdo nem referência externa', () => {
  const raw = { ...record(), text: 'SEGREDO', entryText: 'trecho', targetBot: 'outro', space: 'alheio' }
  assert.deepEqual(readOpeningOmitted([raw]), { state: 'ready', rows: [record()] })
  assert.equal(JSON.stringify(readOpeningOmitted([raw])).includes('SEGREDO'), false)
})

test('D263 ids repetidos na mesma camada, entry inválido e datas ruins são omitidos', () => {
  const dup = [record(), record()]
  assert.equal((readOpeningOmitted(dup) as { rows: unknown[] }).rows.length, 1)
  for (const bad of [{ entry: 0 }, { entry: -3 }, { entry: 1.5 }, { entry: '7' }, { at: -1 }, { at: 9e15 }, { at: NaN }]) {
    assert.deepEqual(readOpeningOmitted([{ ...record(), ...bad }]), { state: 'ready', rows: [] })
  }
  const zero = { ...record(), at: 0 }
  assert.deepEqual(readOpeningOmitted([zero]), { state: 'ready', rows: [zero] })
})

test('D263 cada motivo aponta para uma chave do catálogo', () => {
  for (const reason of Object.keys(openingOmittedReason) as OpeningOmittedReason[]) {
    assert.match(openingOmittedReason[reason], /^memoria\.painel\.foraAbertura\.motivo\./)
  }
})

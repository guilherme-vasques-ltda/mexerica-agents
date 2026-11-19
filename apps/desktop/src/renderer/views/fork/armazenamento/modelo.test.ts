// D70 (DON-905, C6): só o fluxo alterado — tamanhos, contrato do relatório e textos de falha.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { bancoUsado, detalheConversas, fracoes, lerRelatorio, ordenarBots, tamanho, textoDaFalha } from './modelo.ts'

const bot = (id: string, bytes: number | null, extra: Record<string, unknown> = {}) => ({
  id, name: id,
  messages: { main: { bytes: 10, count: 2, oldestAt: null }, extras: null, bytes: null },
  attachmentsBytes: null, memory: { rowsBytes: null, filesBytes: null, bytes: null },
  whatsapp: { hubBytes: null, wacliBytes: null, bytes: null }, bytes, ...extra,
})
const resposta = (bots: unknown = [bot('a', 5)]) => ({ measuredAt: 1_700_000_000_000, total: { databaseBytes: 100, walBytes: 20, databaseFreeBytes: null, diskFreeBytes: 9 }, bots })

test('tamanho: base 1000, uma casa só abaixo de 10, sem "1.000" e indisponível para null', () => {
  assert.equal(tamanho(0), '0 B')
  assert.equal(tamanho(999), '999 B')
  assert.equal(tamanho(1_000), '1 KB')
  assert.equal(tamanho(1_500), '1,5 KB')
  assert.equal(tamanho(15_300), '15 KB')
  assert.equal(tamanho(999_499), '999 KB')
  assert.equal(tamanho(999_500), '1 MB')
  assert.equal(tamanho(1_234_000_000), '1,2 GB')
  assert.equal(tamanho(2_500_000_000_000_000), '2.500 TB')
  assert.equal(tamanho(null), 'Indisponível')
})

test('lerRelatorio: null fica null (nunca 0) e bots pode faltar inteiro', () => {
  const r = lerRelatorio(resposta())
  assert.equal(r.total.databaseFreeBytes, null)
  assert.equal(r.bots![0]!.messages.extras, null)
  assert.equal(r.bots![0]!.memory.bytes, null)
  assert.equal(lerRelatorio(resposta(null)).bots, null)
  assert.equal(lerRelatorio(resposta(null)).total.diskFreeBytes, 9)
})

test('lerRelatorio: fora do contrato recusa tudo', () => {
  for (const ruim of [null, {}, resposta('x'), resposta([bot('a', -1)]), resposta([bot('a', 1.5)]), resposta([bot('a', 1, { whatsapp: null })]),
    { ...resposta(), total: { ...resposta().total, walBytes: '20' } }, resposta([bot('a', 1, { messages: { main: { bytes: 1, count: 1 }, extras: null, bytes: 1 } })])]) {
    assert.throws(() => lerRelatorio(ruim), /storage_invalid_answer/)
  }
})

test('banco usado = arquivo + WAL, sem somar as partes dos bots; uma parte faltando, indisponível', () => {
  assert.equal(bancoUsado(lerRelatorio(resposta()).total), 120)
  assert.equal(bancoUsado({ databaseBytes: 100, walBytes: null, databaseFreeBytes: 0, diskFreeBytes: 0 }), null)
})

test('ordem e frações: maior primeiro, indisponível no fim e fora da soma', () => {
  const bots = lerRelatorio(resposta([bot('b', 25), bot('x', null), bot('a', 75), bot('c', 25)])).bots!
  assert.deepEqual(ordenarBots(bots).map((b) => b.id), ['a', 'b', 'c', 'x'])
  const f = fracoes(bots)
  assert.equal(f.get('a'), 0.6)
  assert.equal(f.get('x'), null)
  assert.equal(fracoes(lerRelatorio(resposta([bot('z', 0)])).bots!).get('z'), null)
})

test('detalhe das conversas: contagem e a mais antiga', () => {
  assert.equal(detalheConversas({ bytes: 1, count: 1234, oldestAt: Date.UTC(2026, 0, 3, 12) }, 'UTC'), '1.234 mensagens · desde 3 de jan. de 2026')
  assert.equal(detalheConversas({ bytes: 0, count: 1, oldestAt: null }), '1 mensagem')
  assert.equal(detalheConversas(null), null)
})

test('falhas: Hub antigo, não dono, resposta inválida e o resto', () => {
  assert.equal(textoDaFalha(Object.assign(new Error('x'), { status: 404 })), 'Atualize o Hub para ver o armazenamento.')
  assert.equal(textoDaFalha(new Error('unknown method donnaStorageReport')), 'Atualize o Hub para ver o armazenamento.')
  assert.equal(textoDaFalha(Object.assign(new Error('forbidden'), { status: 403 })), 'Só o dono deste computador vê o armazenamento dele.')
  assert.match(textoDaFalha(new Error('storage_invalid_answer')), /não entende/)
  assert.equal(textoDaFalha(new Error('timeout')), 'Não foi possível medir o armazenamento agora.')
})

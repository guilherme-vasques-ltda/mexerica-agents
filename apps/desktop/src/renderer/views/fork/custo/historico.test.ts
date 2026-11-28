// `donnaUsageHistory`: o parser do contrato (sem DOM). Janela por provider/accountId?/windowId; buckets { start, percent }.
import assert from 'node:assert/strict'
import test from 'node:test'
import { INVALIDA } from './modelo.ts'
import { lerHistorico, porConta } from './historico.ts'

const janela = (o: Record<string, unknown> = {}) => ({ provider: 'codex', windowId: 'primary', resetsAt: 2000, buckets: [{ start: 0, percent: 10 }, { start: 600000, percent: 12.5 }], forecast: { exhausted: false, exhaustsAt: null }, ...o })
const resposta = (windows: unknown[] = [janela()], o: Record<string, unknown> = {}) => ({ range: '7d', bucketMs: 3600000, measuredAt: 5, truncated: false, windows, ...o })

test('lê o contrato; accountId ausente ou null é conta não identificada, nunca inferida', () => {
  const h = lerHistorico(resposta([janela(), janela({ accountId: null, windowId: 'secondary' }), janela({ accountId: 'acc-1' })]), '7d')
  assert.deepEqual(h.windows.map((w) => w.accountId), [null, null, 'acc-1'])
  assert.equal(h.windows[0]!.buckets[1]!.percent, 12.5)
  assert.deepEqual(h.windows[0]!.forecast, { exhausted: false, exhaustsAt: null })
})

test('previsão null (Hub não soube prever) e resetsAt null ficam como estão, sem virar zero', () => {
  const h = lerHistorico(resposta([janela({ forecast: null, resetsAt: null, buckets: [] })]), '7d')
  assert.equal(h.windows[0]!.forecast, null)
  assert.equal(h.windows[0]!.resetsAt, null)
  assert.deepEqual(h.windows[0]!.buckets, [])
})

test('formato fora do contrato é recusado (faixa trocada, percent inválido, janela sem id)', () => {
  const ruins: [unknown, '7d' | '24h'][] = [
    [resposta(), '24h'],
    [resposta([janela({ buckets: [{ start: 0, percent: -1 }] })]), '7d'],
    [resposta([janela({ buckets: [{ start: 0, percent: null }] })]), '7d'],
    [resposta([janela({ buckets: [{ start: 1, percent: 10 }, { start: 0, percent: 20 }] })]), '7d'],
    [resposta([], { bucketMs: 0 }), '7d'],
    [resposta([janela({ windowId: '' })]), '7d'],
    [resposta([janela({ accountId: 7 })]), '7d'],
    [resposta([janela({ forecast: { exhausted: 'sim' } })]), '7d'],
    [resposta([], { truncated: 'não' }), '7d'],
    [null, '7d'],
  ]
  for (const [raw, faixa] of ruins) assert.throws(() => lerHistorico(raw, faixa), { message: INVALIDA })
})

test('agrupa por conta sem somar nem misturar contas', () => {
  const h = lerHistorico(resposta([janela({ accountId: 'a' }), janela({ accountId: 'b' }), janela({ accountId: 'a', windowId: 'secondary' })]), '7d')
  const grupos = porConta(h.windows)
  assert.deepEqual(grupos.map((g) => [g.accountId, g.janelas.map((w) => w.windowId)]), [['a', ['primary', 'secondary']], ['b', ['primary']]])
})

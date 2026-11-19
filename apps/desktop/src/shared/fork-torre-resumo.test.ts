import { test } from 'node:test'
import assert from 'node:assert/strict'
import { countByTone, parseTowerSnapshot } from './fork-torre.ts'
import { longDay, nextDay, parseWhatHappened, relativeDay, TIMES } from './fork-torre-resumo.ts'

// MB-3 (D70): o parser só aceita o que conhece; Hub antigo lê indisponível; a coluna fica fora dos contadores.
const value = (extra: Record<string, unknown> = {}) => ({
  enabled: true, time: '19:00', timeZone: 'America/Sao_Paulo', day: '2026-11-04', closesAt: 1, nextAt: 2,
  teams: [{ teamId: 't1', name: 'Estoque', line: { text: '2 compras', joined: false }, bots: [{ botId: 'a', line: { text: 'conferiu', joined: false } }, { botId: 'x' }] }],
  loose: [{ botId: 'v', line: { text: 'conferiu 14 pedidos', joined: false } }],
  out: [{ botId: 'r', reason: 'codex' }, { botId: 'z', reason: 'novo' }],
  ...extra,
})

test('resumo: item desconhecido é ignorado e campo inválido fica indisponível', () => {
  const parsed = parseWhatHappened(value())
  assert.deepEqual(parsed?.teams[0].bots.map((b) => b.botId), ['a'])
  assert.deepEqual(parsed?.out, [{ botId: 'r', reason: 'codex' }])
  assert.equal(parsed?.loose[0].botId, 'v')
  for (const bad of [null, value({ teams: 'x' }), value({ time: '' }), value({ enabled: 'sim' })]) assert.equal(parseWhatHappened(bad), null)
})

test('resumo: snapshot sem o campo lê indisponível; contadores não mudam com a coluna', () => {
  const bot = { botId: 'a', name: 'A', runtime: { availability: 'available', source: 's', coverage: 'c', value: { status: 'working' } } }
  const base = (extra: Record<string, unknown>) => ({ version: 1, hubId: 'h1', snapshotId: 's', collectedAt: 5, spaceId: 'p', bots: [bot], ...extra })
  const old = parseTowerSnapshot(base({}), { hubId: 'h1', spaceId: 'p' })
  assert.deepEqual(old.whatHappened, { availability: 'unavailable' })
  const snap = parseTowerSnapshot(base({ whatHappened: { availability: 'available', source: 'resumo_diario', coverage: 'c', value: value() } }), { hubId: 'h1', spaceId: 'p' })
  assert.equal(snap.whatHappened.availability, 'available')
  assert.deepEqual(countByTone(snap.bots), countByTone(old.bots))
})

test('resumo: ontem, hoje e o próximo fechamento no fuso salvo', () => {
  const at = Date.UTC(2026, 10, 5, 18, 6) // 15:06 em São Paulo
  assert.deepEqual(relativeDay('2026-11-04', at, 'America/Sao_Paulo'), { offset: -1 })
  assert.deepEqual(relativeDay('2026-11-05', at, 'America/Sao_Paulo'), { offset: 0 })
  assert.deepEqual(relativeDay('2026-11-01', at, 'America/Sao_Paulo'), { day: '2026-11-01' })
  assert.deepEqual(nextDay(Date.UTC(2026, 10, 5, 22), at, 'America/Sao_Paulo'), { offset: 0 })
  assert.deepEqual(nextDay(Date.UTC(2026, 10, 6, 22), at, 'America/Sao_Paulo'), { offset: 1 })
  assert.equal(longDay('2026-11-04'), 'Quarta-feira, 4 de nov')
  assert.equal(longDay('2026-11-04', 'en-US'), 'Wednesday, Nov 4')
})

test('resumo: o seletor vai de 02:00 a 23:45, para o nível mais fundo não cruzar a meia-noite', () => {
  assert.equal(TIMES[0], '02:00')
  assert.equal(TIMES.at(-1), '23:45')
  assert.ok(!TIMES.includes('00:00') && !TIMES.includes('01:45') && TIMES.includes('19:00'))
})

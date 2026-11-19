import assert from 'node:assert/strict'
import test from 'node:test'
import { formatGmtOffset, monthGrid, packCollisions, rangeLabel, startOfDay, viewRange, weekdayShort, weekdayShortNames } from './routine-calendar.ts'

test('o mês tem 6 semanas completas a partir do domingo e cobre o mês todo', () => {
  const days = monthGrid(new Date(2026, 9, 20).getTime())
  assert.equal(days.length, 42)
  assert.equal(new Date(days[0]!).getDay(), 0)
  assert.deepEqual([new Date(days[0]!).getMonth(), new Date(days[0]!).getDate()], [8, 27])
  assert.ok(days.includes(new Date(2026, 9, 31).getTime()))
  const week = viewRange('week', new Date(2026, 9, 21, 15).getTime())
  assert.deepEqual([new Date(week.start).getDate(), week.days.length], [18, 7])
})

test('rotinas no mesmo horário dividem a largura; as seguintes voltam à primeira coluna', () => {
  const at = Date.UTC(2026, 9, 20, 12)
  const layout = packCollisions([
    { id: 'a', at, durationMinutes: 30 },
    { id: 'b', at, durationMinutes: 30 },
    { id: 'c', at: at + 30 * 60_000, durationMinutes: 30 },
  ])
  assert.deepEqual(layout.get('a'), { column: 0, columns: 2 })
  assert.deepEqual(layout.get('b'), { column: 1, columns: 2 })
  assert.deepEqual(layout.get('c'), { column: 0, columns: 1 })
})

test('o fuso aparece como GMT', () => {
  assert.equal(formatGmtOffset(-180), 'GMT-3')
  assert.equal(formatGmtOffset(330), 'GMT+5:30')
  assert.equal(formatGmtOffset(0), 'GMT+0')
})

test('na semana em que o horário de verão começa à meia-noite, cada dia bate com startOfDay', () => {
  const zone = process.env.TZ
  // O horário de verão brasileiro começava às 00:00 de domingo: 04/11/2018 começou à 01:00.
  process.env.TZ = 'America/Sao_Paulo'
  try {
    const week = viewRange('week', new Date(2018, 10, 8, 15).getTime())
    assert.deepEqual(week.days.map((day) => startOfDay(day) === day), Array(7).fill(true))
    assert.ok(week.days.includes(startOfDay(new Date(2018, 10, 8, 9).getTime())), 'a quinta às 09:00 cai num dos dias')
    assert.ok(monthGrid(new Date(2018, 10, 1).getTime()).every((day) => startOfDay(day) === day))
  } finally {
    if (zone === undefined) delete process.env.TZ
    else process.env.TZ = zone
  }
})

test('auditoria D249, F2: dias da semana em caixa de frase, sem ponto', () => {
  assert.equal(weekdayShort(new Date(2026, 10, 1).getTime(), 'pt-BR'), 'Dom')
  assert.equal(weekdayShort(new Date(2026, 10, 7).getTime(), 'pt-BR'), 'Sáb')
  assert.deepEqual(weekdayShortNames('pt-BR'), ['Dom', 'Seg', 'Ter', 'Qua', 'Qui', 'Sex', 'Sáb'])
})

test('DON-917: datas da agenda no idioma do app (en sem escolha, pt-BR com escolha)', () => {
  assert.deepEqual(weekdayShortNames('en'), ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'])
  const quarta = new Date(2026, 9, 21).getTime()
  // O Intl separa intervalos com espaço fino; o teste compara o texto.
  const plano = (texto: string) => texto.replace(/\s/g, ' ')
  assert.equal(plano(rangeLabel('week', quarta, 'en')), 'October 18 – 24, 2026')
  assert.equal(plano(rangeLabel('week', quarta, 'pt-BR')), '18 – 24 de outubro de 2026')
  assert.equal(rangeLabel('month', quarta, 'pt-BR'), 'Outubro de 2026')
})

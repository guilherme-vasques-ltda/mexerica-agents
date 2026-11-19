import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { ALL_DAYS, MAX_TIMES, WEEKDAYS, addTime, badTimes, buildClockTriggers, daysOfStyle, readClockTriggers, removeTimeAt, setTimeAt, singleCron, sortedTimes, styleOfDays, timesProblem, triggersToSave } from './routine-times.ts'
import { agendaTexto, diasTexto, horaTexto, problemaTexto, vezesTexto } from './routine-times-texto.ts'
import type { RoutineTrigger } from './routine.ts'
import { APP_LANGUAGES } from '../../../shared/fork-idioma.ts'

const ZONE = 'America/Sao_Paulo'
const cron = (expression: string, timeZone: string | null = ZONE): RoutineTrigger => ({ type: 'cron', expression, timeZone })
/** Uma rotina existente (criada pelo bot por `save_routine`) com `n` horários cheios, um gatilho cada, todos os dias úteis. */
const daily = (n: number, from = 4): RoutineTrigger[] => Array.from({ length: n }, (_, i) => cron(`0 ${from + i} * * 1-5`))

/** Abrir, editar um horário e salvar, com as mesmas funções que o editor usa (leitura, edição, ordem ao sair do campo, conferência, gatilhos). */
const openEditSave = (triggers: RoutineTrigger[], index: number, value: string) => {
  const read = readClockTriggers(triggers)
  assert.ok(read, 'a rotina abre como pílulas')
  const times = sortedTimes(setTimeAt(read.times, index, value))
  assert.equal(timesProblem(times, read.days), null)
  return buildClockTriggers({ times, days: read.days, zone: read.zone })
}

test('rotina existente com 13 horários abre com as 13 pílulas e salva mantendo os outros 12', () => {
  const triggers = daily(13, 8) // 8h … 20h
  const read = readClockTriggers(triggers)
  assert.equal(read?.times.length, 13)
  const saved = openEditSave(triggers, 0, '07:30') // 8h vira 7h30
  assert.equal(saved.length, 13)
  assert.deepEqual(saved.map((t) => t.expression), ['30 7 * * 1-5', ...Array.from({ length: 12 }, (_, i) => `0 ${9 + i} * * 1-5`)])
  assert.ok(saved.every((t) => t.type === 'cron' && t.timeZone === ZONE))
})

test('rotina existente com 20 horários (o teto do Hub) abre com as 20 e salva as 20, sem cortar', () => {
  const triggers = daily(20, 4) // 4h … 23h
  assert.equal(readClockTriggers(triggers)?.times.length, MAX_TIMES)
  const saved = openEditSave(triggers, 19, '23:45') // o último
  assert.equal(saved.length, 20)
  assert.deepEqual(saved.slice(0, 19), triggers.slice(0, 19))
  assert.equal(saved[19]!.expression, '45 23 * * 1-5')
  // Sem editar nada, salvar devolve os mesmos 20 gatilhos.
  const untouched = readClockTriggers(triggers)!
  assert.deepEqual(buildClockTriggers(untouched), triggers)
})

test('o editor envia os gatilhos montados a partir de todas as pílulas, sem cortar', () => {
  const editor = readFileSync(new URL('./RoutineEditorView.tsx', import.meta.url), 'utf8')
  const campos = readFileSync(new URL('./RoutineScheduleFields.tsx', import.meta.url), 'utf8')
  assert.match(editor, /triggers: clockTriggers\(\)/)
  assert.match(editor, /triggersToSave\(routine\?\.triggers, \{ times, days: clockDays, zone: schedule\.zone \}\)/)
  // Nenhum corte: nem `slice` nem teto de 12 na tela; o teto é o do Hub (MAX_TIMES = 20).
  for (const source of [editor, campos]) assert.doesNotMatch(source, /times\.slice\(|\.slice\(0, 1[0-9]\)|times\.length > 12|MAX_TIMES\s*=\s*12/)
  assert.equal(MAX_TIMES, 20)
})

test('mais de 20 horários não viram pílulas: a rotina fica como o bot a fez, sem corte', () => {
  // 21 gatilhos (o Hub não deixa gravar, mas um cron em lista pode passar de 20): fica "Definida pelo bot".
  assert.equal(readClockTriggers(daily(21, 0)), null)
  assert.equal(readClockTriggers([cron('0 0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22 * * *')]), null)
  // Dois crons em lista que juntos passam de 20 também não.
  assert.equal(readClockTriggers([cron('0 0,1,2,3,4,5,6,7,8,9,10,11 * * *'), cron('30 0,1,2,3,4,5,6,7,8,9,10,11 * * *')]), null)
})

test('o cron composto "0 9,15 * * *" e as listas de hora e de minuto viram pílulas', () => {
  assert.deepEqual(readClockTriggers([cron('0 9,15 * * *')]), { times: ['09:00', '15:00'], days: [0, 1, 2, 3, 4, 5, 6], zone: ZONE })
  assert.deepEqual(readClockTriggers([cron('0,30 9 * * 1-5')])?.times, ['09:00', '09:30'])
  assert.deepEqual(readClockTriggers([cron('0,30 9,15 * * 1,3,5')])?.times, ['09:00', '09:30', '15:00', '15:30'])
  assert.deepEqual(readClockTriggers([cron('0 9,15 * * 1,3,5'), cron('30 18 * * 1,3,5')])?.times, ['09:00', '15:00', '18:30'])
  // Salvar grava um gatilho por horário, não a lista.
  const read = readClockTriggers([cron('0 9,15 * * *')])!
  assert.deepEqual(buildClockTriggers(read), [cron('0 9 * * *'), cron('0 15 * * *')])
})

test('um gatilho só continua a mesma agenda: dias úteis, um dia, alguns dias', () => {
  assert.deepEqual(readClockTriggers([cron('0 9 * * 1-5')]), { times: ['09:00'], days: [1, 2, 3, 4, 5], zone: ZONE })
  assert.deepEqual(readClockTriggers([cron('30 17 * * 5')])?.days, [5])
  assert.deepEqual(readClockTriggers([cron('5 23 * * 1,3,6')])?.days, [1, 3, 6])
  assert.deepEqual(readClockTriggers([cron('0 9 * * 0')])?.days, [0])
  assert.deepEqual(buildClockTriggers(readClockTriggers([cron('5 23 * * 1,3,6')])!), [cron('5 23 * * 1,3,6')])
  assert.deepEqual(buildClockTriggers(readClockTriggers([cron('0 0 * * *')])!), [cron('0 0 * * *')])
})

test('o que as pílulas não cobrem fica como está (Definida pelo bot): nada é lido nem cortado', () => {
  assert.equal(readClockTriggers([]), null)
  assert.equal(readClockTriggers([cron('0 9 * * 1-5'), cron('0 15 * * 1,3')]), null, 'dias diferentes')
  assert.equal(readClockTriggers([cron('0 9 * * *'), cron('0 15 * * *', 'Asia/Tokyo')]), null, 'fusos diferentes')
  assert.equal(readClockTriggers([cron('0 9 * * *'), { type: 'webhook' }]), null)
  assert.equal(readClockTriggers([cron('0 9 * * *'), { type: 'interval', seconds: 5400 }]), null)
  assert.equal(readClockTriggers([{ type: 'once', at: 1000 }]), null)
  assert.equal(readClockTriggers([cron('*/15 * * * *')]), null, 'a cada 15 minutos')
  assert.equal(readClockTriggers([cron('37 * * * *')]), null, 'a cada hora')
  assert.equal(readClockTriggers([cron('45 8 31 * *')]), null, 'dia do mês')
  assert.equal(readClockTriggers([cron('0 9-17 * * *')]), null, 'faixa de horas')
  assert.equal(readClockTriggers([cron('0 9 * * 8')]), null)
  assert.equal(readClockTriggers([cron('0 9 * * FRI-SUN')]), null, 'faixa que passa do domingo é ambígua')
  assert.equal(readClockTriggers([cron('0 9 * * 6-0')]), null)
  assert.equal(readClockTriggers([cron('0 25 * * *')]), null)
  assert.equal(readClockTriggers([cron('0 9 * * *', null)]), null, 'sem fuso')
})

test('os dias valem para todos os horários e o estilo de "Repetir" os acompanha', () => {
  assert.equal(styleOfDays([0, 1, 2, 3, 4, 5, 6]), 'daily')
  assert.equal(styleOfDays([1, 2, 3, 4, 5]), 'weekdays')
  assert.equal(styleOfDays([1, 3]), 'some')
  assert.equal(styleOfDays([2]), 'some')
  assert.equal(styleOfDays([]), 'some')
  assert.deepEqual(daysOfStyle('daily', []), [0, 1, 2, 3, 4, 5, 6])
  assert.deepEqual(daysOfStyle('weekdays', []), [1, 2, 3, 4, 5])
  assert.deepEqual(daysOfStyle('some', [5, 1]), [1, 5])
  assert.deepEqual(buildClockTriggers({ times: ['09:00', '13:30'], days: [1, 3, 5], zone: ZONE }), [cron('0 9 * * 1,3,5'), cron('30 13 * * 1,3,5')])
})

test('"+ horário" põe a hora seguinte sem repetir, ordena, e some no teto de 20 (a tela não passa dele)', () => {
  assert.deepEqual(addTime(['09:00']), ['09:00', '10:00'])
  assert.deepEqual(addTime(['09:30', '15:30']), ['09:30', '15:30', '16:30'])
  assert.deepEqual(addTime(['23:00']), ['00:00', '23:00'])
  let times = ['04:00']
  while (times.length < MAX_TIMES) times = addTime(times)
  assert.equal(times.length, 20)
  assert.equal(new Set(times).size, 20)
  assert.deepEqual(addTime(times), times, 'no teto não passa')
  assert.deepEqual(removeTimeAt(times, 0).length, 19)
  assert.equal(addTime(removeTimeAt(times, 0)).length, 20, 'ao tirar uma, volta a caber')
})

test('mínimo de um horário e ordem só ao sair do campo, com tudo preenchido', () => {
  assert.deepEqual(removeTimeAt(['09:00'], 0), ['09:00'])
  assert.deepEqual(removeTimeAt(['09:00', '15:00'], 0), ['15:00'])
  assert.deepEqual(sortedTimes(['15:00', '09:00']), ['09:00', '15:00'])
  assert.deepEqual(sortedTimes(['15:00', '', '09:00']), ['15:00', '', '09:00'])
})

test('horário repetido, vazio ou sem dia: recusado, com a pílula pintada de erro', () => {
  assert.equal(timesProblem(['09:00', '15:00'], [1]), null)
  assert.deepEqual(timesProblem(['09:00', '15:00', '15:00'], [1]), { kind: 'repeat', times: ['15:00'] })
  assert.deepEqual(timesProblem(['09:00', ''], [1]), { kind: 'empty' })
  assert.deepEqual(timesProblem([], [1]), { kind: 'empty' })
  assert.deepEqual(timesProblem(['09:00'], []), { kind: 'noDay' })
  assert.deepEqual(badTimes(['09:00', '15:00', '15:00', '']), [false, true, true, true])
})

test('textos: "9h", "Dias úteis às 9h e às 15h", "à 0h", a conta e as mensagens, em pt-BR', () => {
  assert.equal(horaTexto('pt-BR', '09:00'), '9h')
  assert.equal(horaTexto('pt-BR', '17:30'), '17h30')
  assert.equal(horaTexto('pt-BR', '00:05'), '0h05')
  assert.equal(agendaTexto('pt-BR', [1, 2, 3, 4, 5], ['09:00', '15:00']), 'Dias úteis às 9h e às 15h')
  assert.equal(agendaTexto('pt-BR', [0, 1, 2, 3, 4, 5, 6], ['09:00', '15:00']), 'Todo dia às 9h e às 15h')
  assert.equal(agendaTexto('pt-BR', [1], ['00:00', '01:00', '13:00']), 'Seg à 0h, à 1h e às 13h')
  assert.equal(agendaTexto('pt-BR', [1, 3, 5], ['09:00', '13:00', '17:30']), 'Seg, qua e sex às 9h, às 13h e às 17h30')
  assert.equal(vezesTexto('pt-BR', 3, 5), '3 horários × 5 dias = 15 vezes por semana')
  assert.equal(vezesTexto('pt-BR', 2, 1), '2 horários × 1 dia = 2 vezes por semana')
  assert.equal(problemaTexto('pt-BR', { kind: 'repeat', times: ['15:00'] }, 3), 'Dois horários iguais: 15h. Troque um deles ou tire um.')
  assert.equal(problemaTexto('pt-BR', { kind: 'empty' }, 1), 'Escolha um horário.')
  assert.equal(problemaTexto('pt-BR', { kind: 'empty' }, 2), 'Preencha cada horário ou tire o que sobrou.')
  assert.equal(problemaTexto('pt-BR', { kind: 'noDay' }, 2), 'Escolha pelo menos um dia.')
})

test('os 6 idiomas têm todos os textos dos horários, sem chave crua', () => {
  for (const idioma of APP_LANGUAGES) {
    const textos = [
      horaTexto(idioma, '09:00'), horaTexto(idioma, '17:30'), diasTexto(idioma, [1, 2, 3, 4, 5]), diasTexto(idioma, [0, 1, 2, 3, 4, 5, 6]), diasTexto(idioma, [1, 3, 5]),
      agendaTexto(idioma, [1, 3, 5], ['00:00', '01:00', '13:00']), vezesTexto(idioma, 2, 1), vezesTexto(idioma, 3, 5), vezesTexto(idioma, 20, 7),
      problemaTexto(idioma, { kind: 'repeat', times: ['15:00'] }, 3), problemaTexto(idioma, { kind: 'empty' }, 1), problemaTexto(idioma, { kind: 'empty' }, 3), problemaTexto(idioma, { kind: 'noDay' }, 3),
    ]
    for (const texto of textos) {
      assert.ok(texto.length > 0, idioma)
      assert.doesNotMatch(texto, /rotinas\.editor\.|\{[a-z]+\}/, `${idioma}: ${texto}`)
    }
  }
})

test('reduzir movimento: as pílulas não têm animação própria, só as variáveis de tempo do app (que o tema zera)', () => {
  const css = readFileSync(new URL('./routines.css', import.meta.url), 'utf8')
  const pilulas = css.slice(css.indexOf('.time-pills {'))
  assert.doesNotMatch(pilulas, /@keyframes|animation\s*:/)
  for (const [, tempo] of pilulas.matchAll(/transition\s*:([^;]+);/g)) assert.doesNotMatch(tempo!.replace(/var\(--[a-z-]+\)/g, ''), /\d+(\.\d+)?m?s\b/, `tempo fixo em: ${tempo}`)
})

test('dia 7 (domingo) e nomes de dia viram pílulas de dia; o que é ambíguo fica como está', () => {
  assert.deepEqual(readClockTriggers([cron('0 9 * * 7')])?.days, [0])
  assert.deepEqual(readClockTriggers([cron('0 9 * * 5-7')])?.days, [0, 5, 6])
  assert.deepEqual(readClockTriggers([cron('0 9 * * MON-FRI')])?.days, [1, 2, 3, 4, 5])
  assert.deepEqual(readClockTriggers([cron('0 9 * * sun,sat')])?.days, [0, 6])
  assert.deepEqual(readClockTriggers([cron('0 9 * * 0-7')])?.days, [0, 1, 2, 3, 4, 5, 6])
  assert.deepEqual(readClockTriggers([cron('0 9 * * 1-5'), cron('0 15 * * MON-FRI')])?.times, ['09:00', '15:00'])
  assert.equal(readClockTriggers([cron('0 9 * * MON-WHO')]), null)
  assert.equal(readClockTriggers([cron('0 9 * * 1-2-3')]), null)
})

/** Os disparos de um cron "M H * * D" por semana, como "dia hora:minuto": expansão independente do código da tela. */
const firings = (expressions: string[]) => {
  const out = new Set<string>()
  const list = (field: string, max: number) => (field === '*' ? Array.from({ length: max + 1 }, (_, i) => i) : field.split(',').flatMap((p) => { const [a, b] = p.split('-').map(Number); return b === undefined ? [a!] : Array.from({ length: b - a! + 1 }, (_, i) => a! + i) }))
  for (const expression of expressions) {
    const [m, h, , , d] = expression.split(' ') as [string, string, string, string, string]
    for (const day of list(d, 6)) for (const hour of list(h, 23)) for (const minute of list(m, 59)) out.add(`${day} ${hour}:${minute}`)
  }
  return [...out].sort()
}

test('propriedade: passar por Personalizado nunca muda o conjunto de disparos (1, 2, 13 e 20 gatilhos)', () => {
  const casos: RoutineTrigger[][] = [
    daily(1, 9),
    [cron('0 9 * * 1-5'), cron('0 15 * * 1-5')],
    daily(13, 8),
    daily(20, 4),
    [cron('0 9 * * 1,3'), cron('30 9 * * 1,3'), cron('0 15 * * 1,3'), cron('30 15 * * 1,3')],
    [cron('0,30 9,15 * * *')],
    [cron('0 9 * * *'), cron('0 15 * * *')],
  ]
  for (const triggers of casos) {
    const read = readClockTriggers(triggers)!
    const expression = singleCron(read.times, read.days)
    assert.ok(expression, `${triggers.length} gatilhos têm um cron único`)
    assert.deepEqual(firings([expression]), firings(triggers.map((t) => t.expression!)), expression)
  }
})

test('horários que não são minutos x horas não têm cron único: "Personalizado" não é oferecido e nada some', () => {
  for (const triggers of [[cron('0 9 * * 1-5'), cron('30 15 * * 1-5')], [cron('0 9 * * *'), cron('30 9 * * *'), cron('0 15 * * *')]]) {
    const read = readClockTriggers(triggers)!
    assert.equal(singleCron(read.times, read.days), null)
  }
  assert.equal(singleCron(['09:00', '09:00'], [1]), null, 'repetido')
  assert.equal(singleCron(['09:00'], []), null, 'sem dia')
  assert.equal(singleCron(['09:00', ''], [1]), null, 'vazio')
  const campos = readFileSync(new URL('./RoutineScheduleFields.tsx', import.meta.url), 'utf8')
  assert.match(campos, /repeatChoices\.filter\(\(c\) => c\.id !== 'custom' \|\| !clock \|\| customCron !== null\)/)
  assert.match(campos, /expression: customCron \?\? compiledCron\(preview\)/)
})

test('"Alguns dias" vindo de "Todo dia" começa sem dia marcado; vindo de "Dias úteis" mantém os cinco', () => {
  const campos = readFileSync(new URL('./RoutineScheduleFields.tsx', import.meta.url), 'utf8')
  assert.match(campos, /selectedDays: clock \? \(days\.length === 7 \? \[\] : days\) : \[1, 2, 3, 4, 5\]/)
})

/** O que o editor envia ao abrir uma rotina e salvar: o estado vem da leitura e `change` mexe só no que a pessoa mexeu. */
const openAndSave = (triggers: RoutineTrigger[], change: (state: { times: string[]; days: number[]; zone: string }) => void = () => {}) => {
  const read = readClockTriggers(triggers)
  assert.ok(read, 'a rotina abre como pílulas')
  const state = { times: [...read.times], days: [...read.days], zone: read.zone }
  change(state)
  return triggersToSave(triggers, { ...state, times: sortedTimes(state.times) })
}

test('propriedade: abrir e salvar sem mexer na agenda (só nome, instrução ou ligado) devolve os gatilhos salvos, iguais', () => {
  const NY = 'America/New_York'
  const casos: RoutineTrigger[][] = [
    [cron('0 9,15 * * *')],
    [cron('0,30 9,15 * * 1-5')],
    [cron('0 9,15 * * MON-FRI')],
    [cron('0 9,15 * * 7')],
    [cron('0 2,3 * * 0', NY)],
    [cron('0 9,15 * * 1,3'), cron('30 17 * * 1,3')],
    daily(1),
    daily(13, 8),
    daily(20, 4),
  ]
  for (const triggers of casos) {
    const antes = structuredClone(triggers)
    const enviados = openAndSave(triggers)
    assert.deepEqual(enviados, antes, `reescreveu ${JSON.stringify(antes)}`)
    // Os mesmos valores (o Hub compara os gatilhos para manter as próximas execuções), e a lista de entrada não é tocada.
    assert.deepEqual(triggers, antes)
  }
})

test('mudou algo da agenda (horário novo, tirar horário, dias, fuso): um gatilho por horário, como antes', () => {
  const composto = [cron('0 9,15 * * 1-5')]
  assert.deepEqual(openAndSave(composto, (s) => (s.times = ['09:00', '16:30'])).map((t) => t.expression), ['0 9 * * 1-5', '30 16 * * 1-5'])
  assert.deepEqual(openAndSave(composto, (s) => s.times.push('18:00')).map((t) => t.expression), ['0 9 * * 1-5', '0 15 * * 1-5', '0 18 * * 1-5'])
  assert.deepEqual(openAndSave(composto, (s) => (s.times = ['09:00'])).map((t) => t.expression), ['0 9 * * 1-5'])
  assert.deepEqual(openAndSave(composto, (s) => (s.days = [1, 3])).map((t) => t.expression), ['0 9 * * 1,3', '0 15 * * 1,3'])
  assert.deepEqual(openAndSave(composto, (s) => (s.zone = 'America/New_York')).map((t) => t.timeZone), ['America/New_York', 'America/New_York'])
  // Voltar ao que era também é "não mudou": nada é reescrito.
  assert.deepEqual(openAndSave(composto, (s) => { s.times.push('18:00'); s.times.pop() }), composto)
  // Rotina nova (ou vinda de outro caminho, sem gatilhos salvos): monta como sempre.
  assert.deepEqual(triggersToSave(null, { times: ['09:00', '15:00'], days: [...ALL_DAYS], zone: ZONE }).map((t) => t.expression), ['0 9 * * *', '0 15 * * *'])
  assert.deepEqual(triggersToSave([], { times: ['09:00'], days: [...WEEKDAYS], zone: ZONE }).map((t) => t.expression), ['0 9 * * 1-5'])
})

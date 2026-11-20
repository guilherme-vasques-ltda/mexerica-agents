import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { Entry } from '../../../shared/models.ts'
import { durationLabel, liveSummary, stepsByReply, stepsSummary } from './cards/tool-steps.ts'
import { tradutor } from '../../lib/fork-i18n/index.ts'
import { cellLines, numericColumns, numericValue, toCSV, toTSV, visibleRows } from './cards/data-table.ts'

const t = tradutor('pt-BR')

// D165 fase F: passos vêm só das entradas `tool` do Hub; a tabela só lê a tabela da mensagem.
const entry = (id: string, kind: string, turn: number, at: number, data: Entry['data'] = {}): Entry =>
  ({ id, seq: at, botId: 'bot', rev: 1, kind, turn, data, createdAt: at, updatedAt: at })

test('passos de um turno vão para a última resposta do mesmo autor; sem resposta, ficam ao vivo', () => {
  const entries = [
    entry('u1', 'user', 1, 0, { text: 'oi' }),
    entry('t1', 'tool', 1, 1000, { title: 'npm test', toolKind: 'execute', status: 'completed' }),
    entry('t2', 'tool', 1, 5000, { title: 'Editar src/a.ts', toolKind: 'edit', status: 'failed' }),
    entry('a1', 'agent', 1, 42000, { text: 'pronto', final: true }),
    entry('u2', 'user', 2, 50000, { text: 'de novo' }),
    entry('t3', 'tool', 2, 51000, { title: 'ls', toolKind: 'execute', status: 'in_progress' }),
  ]
  const { byReply, pending } = stepsByReply(entries)
  const turn = byReply.get('a1')!
  assert.deepEqual(turn.steps.map((s) => [s.id, s.state]), [['t1', 'done'], ['t2', 'failed']])
  assert.equal(stepsSummary(t, turn), '2 passos · Comando, Edição · 41 s')
  assert.deepEqual([...pending.values()][0]!.steps.map((s) => s.state), ['running'])
  assert.equal(byReply.size, 1, 'nenhum passo inventado para respostas sem entradas tool')
})

test('D169: resumo ao vivo e "Ferramenta" só quando não há tipo nomeado; nenhum tempo por passo', () => {
  const entries = [
    entry('u1', 'user', 1, 0, { text: 'oi' }),
    entry('t1', 'tool', 1, 1000, { title: 'Leu 38 erros', toolKind: 'read', status: 'completed' }),
    entry('t2', 'tool', 1, 9000, { title: 'Perguntou à Sprint', toolKind: 'other', status: 'completed' }),
    entry('a1', 'agent', 1, 42000, { text: 'pronto', final: true }),
    entry('u2', 'user', 2, 50000, { text: 'e agora?' }),
    entry('t3', 'tool', 2, 51000, { title: 'Abriu o diff', toolKind: 'other', status: 'completed' }),
    entry('t4', 'tool', 2, 54000, { title: 'Procurando…', toolKind: 'other', status: 'in_progress' }),
  ]
  const { byReply, pending } = stepsByReply(entries)
  assert.equal(stepsSummary(t, byReply.get('a1')!), '2 passos · Leitura · 41 s')
  const live = [...pending.values()][0]!
  assert.equal(liveSummary(t, live), 'Trabalhando · 2 passos')
  assert.equal(stepsSummary(t, live), '2 passos · Ferramenta · 3 s')
  assert.ok(live.steps.every((s) => !('duration' in s)), 'o Hub não manda início/fim do passo (D169 B1)')
})

test('duração legível', () => {
  assert.equal(durationLabel(t, 41000), '41 s')
  assert.equal(durationLabel(t, 125000), '2 min 5 s')
  assert.equal(durationLabel(t, 3780000), '1 h 3 min')
})

test('resumo dos passos em inglês: plural e tipos no idioma', () => {
  const en = tradutor('en')
  const { byReply } = stepsByReply([
    entry('t1', 'tool', 1, 1000, { title: 'npm test', toolKind: 'execute', status: 'completed' }),
    entry('a1', 'agent', 1, 42000, { text: 'ok', final: true }),
  ])
  assert.equal(stepsSummary(en, byReply.get('a1')!), '1 step · Command · 41 s')
  assert.equal(liveSummary(en, byReply.get('a1')!), 'Working · 1 step')
})

test('tabela: números, busca sem acento, ordenação estável e exportação segura', () => {
  assert.equal(numericValue('1.234,56'), 1234.56)
  assert.equal(numericValue('R$ 40'), 40)
  assert.equal(numericValue('12%'), 12)
  assert.equal(numericValue('abc'), null)
  const rows = [['Ação', '10'], ['Café', '2'], ['ação extra', '2']]
  const numeric = numericColumns(rows, 2)
  assert.deepEqual(numeric, [false, true])
  assert.deepEqual(visibleRows(rows, 'acao', null, numeric).map((r) => r[0]), ['Ação', 'ação extra'])
  assert.deepEqual(visibleRows(rows, '', { column: 1, dir: 'asc' }, numeric).map((r) => r[0]), ['Café', 'ação extra', 'Ação'])
  assert.equal(toTSV(['a', 'b'], [['x\ty', 'z']]), 'a\tb\nx y\tz')
  assert.equal(toCSV(['nome'], [['=SOMA(A1)'], ['tem, vírgula'], ['-3']]), "nome\r\n'=SOMA(A1)\r\n\"tem, vírgula\"\r\n-3\r\n")
})


test('exportações neutralizam fórmulas em cabeçalhos e células sem transformar números negativos em texto', () => {
  const header = ['=header', '+SUM(A1)', '@SUM(A1)', '-1+2']
  const rows = [['=1+1', '-3', '-1.234,56', '-1,234.56'], [' +SUM(A1)', '\t@SUM(A1)', '-1*2', 'x\ty\nz']]
  const before = JSON.stringify({ header, rows })
  assert.equal(toTSV(header, rows), "'=header\t'+SUM(A1)\t'@SUM(A1)\t'-1+2\n'=1+1\t-3\t-1.234,56\t-1,234.56\n' +SUM(A1)\t' @SUM(A1)\t'-1*2\tx y z")
  assert.equal(toCSV(['h'], [['=1+1'], [' +SUM(A1)'], ['-1*2'], ['-3']]), "h\r\n'=1+1\r\n' +SUM(A1)\r\n'-1*2\r\n-3\r\n")
  assert.equal(toTSV(['h'], [['\u0000=1+1'], ['"=1+1"'], ['a\r\nb']]), "h\n' =1+1\n\"\"\"=1+1\"\"\"\na b")
  assert.deepEqual(['1.234,56', '1,234.56', '12%', 'R$ 40', '-3'].map(numericValue), [1234.56, 1234.56, 12, 40, -3])
  assert.equal(JSON.stringify({ header, rows }), before)
})

test('texto parcial mantém passos ao vivo até a resposta final do mesmo autor', () => {
  const tool = entry('tool', 'tool', 1, 1, { author: 'A', title: 'Ler', status: 'completed' })
  const stream = entry('stream', 'agent', 1, 2, { author: 'A', final: false, text: 'parcial' })
  const live = stepsByReply([tool, stream])
  assert.deepEqual([...live.pending.values()].flatMap((t) => t.steps.map((s) => s.id)), ['tool'])
  assert.equal(live.byReply.size, 0)
  const done = stepsByReply([tool, { ...stream, data: { ...stream.data, final: true } }])
  assert.deepEqual(done.byReply.get('stream')?.steps.map((s) => s.id), ['tool'])
  assert.equal(done.pending.size, 0)
})

test('resposta interrompida leva a ferramenta cortada depois dela, sem deixar passo ao vivo', () => {
  const text = entry('cut', 'agent', 1, 1, { author: 'A', final: false, interrupted: true, text: 'Vou ler o arquivo.' })
  const tool = entry('tool', 'tool', 1, 2, { author: 'A', title: 'Ler', status: 'failed', interrupted: true })
  const { byReply, pending } = stepsByReply([text, tool])
  assert.deepEqual(byReply.get('cut')?.steps.map((s) => s.id), ['tool'])
  assert.equal(pending.size, 0)
  // Nova tentativa do mesmo turno depois do reinício: o passo vivo não vai para a bolha interrompida.
  const again = entry('again', 'tool', 1, 3, { author: 'A', title: 'Ler', status: 'in_progress' })
  const resumed = stepsByReply([text, tool, again])
  assert.equal(resumed.byReply.size, 0)
  assert.deepEqual([...resumed.pending.values()].flatMap((t) => t.steps.map((s) => s.id)), ['tool', 'again'])
  // A ferramenta da nova tentativa terminou e a resposta dela ainda está sendo escrita: os passos seguem ao vivo.
  const done = { ...again, data: { ...again.data, status: 'completed' } }
  const writing = entry('retry', 'agent', 1, 4, { author: 'A', final: false, text: 'Agora sim' })
  const streaming = stepsByReply([text, tool, done, writing])
  assert.equal(streaming.byReply.size, 0)
  assert.deepEqual([...streaming.pending.values()].flatMap((t) => t.steps.map((s) => s.id)), ['tool', 'again'])
})

test('turno sem final não vira atividade do pedido seguinte; histórico final e autores são preservados', () => {
  const old = [entry('done-tool', 'tool', 1, 1), entry('done-reply', 'agent', 1, 2, { final: true })]
  // Interrupção e (pass) realmente ficam sem final:true; não sintetizar uma resposta final.
  for (const tail of [null, entry('stop', 'notice', 2, 4, { text: 'Interrompido', style: 'info' }), entry('pass', 'agent', 2, 4, { final: false, text: '(pass)' })]) {
    const history = [...old, entry('failed', 'tool', 2, 3, { author: 'A', status: 'failed' }), ...(tail ? [tail] : [])]
    const next = [...history, entry('user3', 'user', 3, 5, { text: 'novo pedido' })]
    const quiet = stepsByReply(next)
    assert.equal(quiet.pending.size, 0, tail?.id)
    assert.deepEqual(quiet.byReply.get('done-reply')?.steps.map((s) => s.id), ['done-tool'])
    const working = stepsByReply([...next, entry('new-A', 'tool', 3, 6, { author: 'A' }), entry('new-B', 'tool', 3, 7, { author: 'B' })])
    assert.deepEqual([...working.pending.values()].map((t) => t.steps.map((s) => s.id)), [['new-A'], ['new-B']])
  }
})

test('exportação só isenta literais numéricos estritos, sem herdar a tolerância do sort', () => {
  const ambiguous = ['-R$1', '-US$1', '-R$42', '-US$42', '-R$1%', '-US$1%', '-R$42%', '-US$42%', '-A$1', '-$A$1', '-A1', '-12%', '-3kg', '-1e3', '-1,23,4', '-1.23.4', '-1 234', '- 3', '-1*2']
  // Cabeçalhos e dados passam pela mesma fronteira, em ambos os formatos.
  const escaped = ambiguous.map((s) => `'${s}`).join('\t')
  assert.equal(toTSV(ambiguous, [ambiguous]), `${escaped}\n${escaped}`)
  assert.equal(toCSV(['-R$1', '-US$1'], [['-R$42', '-US$42'], ['-R$1%', '-US$1%']]), "'-R$1,'-US$1\r\n'-R$42,'-US$42\r\n'-R$1%,'-US$1%\r\n")
  assert.equal(toCSV(['h'], ambiguous.map((s) => [s])), "h\r\n'-R$1\r\n'-US$1\r\n'-R$42\r\n'-US$42\r\n'-R$1%\r\n'-US$1%\r\n'-R$42%\r\n'-US$42%\r\n'-A$1\r\n'-$A$1\r\n'-A1\r\n'-12%\r\n'-3kg\r\n'-1e3\r\n\"'-1,23,4\"\r\n'-1.23.4\r\n'-1 234\r\n'- 3\r\n'-1*2\r\n")
  const literals = ['-3', '-0,50', '-3.5', '-1.234,56', '-1,234.56', '-1.234.567,89', '-1,234,567.89']
  assert.equal(toTSV(literals, [literals]), `${literals.join('\t')}\n${literals.join('\t')}`)
  assert.equal(toCSV(['h'], [['-3'], ['-1.234,56'], ['-1,234.56']]), 'h\r\n-3\r\n"-1.234,56"\r\n"-1,234.56"\r\n')
  assert.equal(toCSV(['h'], [['-3\n'], [' -3'], ['-1\t2']]), 'h\r\n"\'-3\n"\r\n\' -3\r\n\'-1\t2\r\n')
  assert.deepEqual(['-R$1', '-US$42', '-12%'].map(numericValue), [-1, -42, -12], 'sort continua tolerante')
})

test('célula da tabela: só <br> quebra a linha; outra marcação continua texto (D169 R5)', () => {
  assert.deepEqual(cellLines('Seg 07h<br>Ter 08h<BR/>Qua 09h<br />Qui'), ['Seg 07h', 'Ter 08h', 'Qua 09h', 'Qui'])
  assert.deepEqual(cellLines('sem quebra'), ['sem quebra'])
  // Canário: nada vira elemento; o texto segue literal para o renderizador inline.
  assert.deepEqual(cellLines('<img src=x onerror=alert(1)><br><script>x</script>'), ['<img src=x onerror=alert(1)>', '<script>x</script>'])
  assert.deepEqual(cellLines('<b>negrito</b> <brx> <br-x>'), ['<b>negrito</b> <brx> <br-x>'])
})

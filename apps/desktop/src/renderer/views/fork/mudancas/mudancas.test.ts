import assert from 'node:assert/strict'
import test from 'node:test'
import type { Entry, FileDiff } from '../../../../shared/models.ts'
import { changesByReply, DIFF_LINE_LIMIT, shouldShowChangesTrigger } from './mudancas.ts'

const entry = (id: string, kind: string, turn: number, at: number, data: Entry['data'] = {}, sessionId?: string, botId = 'bot-1'): Entry => ({
  id,
  seq: at,
  botId,
  rev: 1,
  kind,
  turn,
  data,
  createdAt: at,
  updatedAt: at,
  ...(sessionId !== undefined ? { sessionId } : {}),
})

const diff = (path: string, added: number, removed: number, patch = '+linha', isNew = false, startLine = 1): FileDiff => ({
  path,
  added,
  removed,
  isNew,
  startLine,
  patch,
})

test('agrupa 3 edições em 2 arquivos preservando ordem e soma aproximada', () => {
  const entries = [
    entry('u1', 'user', 1, 10, { text: 'ajuste' }),
    entry('t1', 'tool', 1, 20, {
      title: 'Edit src/a.ts',
      toolKind: 'edit',
      status: 'completed',
      diffs: [diff('src/a.ts', 4, 1, '+a1', false, 10)],
    }),
    entry('t2', 'tool', 1, 30, {
      title: 'Write src/b.ts',
      toolKind: 'edit',
      status: 'completed',
      diffs: [diff('src/b.ts', 12, 0, '+b1', true, 1)],
    }),
    entry('t3', 'tool', 1, 40, {
      title: 'Edit src/a.ts',
      toolKind: 'edit',
      status: 'completed',
      diffs: [diff('src/a.ts', 2, 3, '-a2\n+a3', false, 25)],
    }),
    entry('r1', 'agent', 1, 50, { text: 'feito', final: true }),
  ]

  const map = changesByReply(entries)
  assert.equal(map.size, 1)
  const changes = map.get('r1')!
  assert.equal(changes.files.length, 2)
  assert.deepEqual(changes.files.map((f) => f.path), ['src/a.ts', 'src/b.ts'])
  assert.equal(changes.files[0]!.fileName, 'a.ts')
  assert.equal(changes.files[0]!.isNew, false)
  assert.equal(changes.files[0]!.approxAdded, 6)
  assert.equal(changes.files[0]!.approxRemoved, 4)
  assert.deepEqual(changes.files[0]!.hunks.map((h) => [h.entryId, h.stepIndex]), [['t1', 1], ['t3', 3]])

  assert.equal(changes.files[1]!.fileName, 'b.ts')
  assert.equal(changes.files[1]!.isNew, true)
  assert.equal(changes.files[1]!.approxAdded, 12)
  assert.equal(changes.files[1]!.approxRemoved, 0)
  assert.equal(changes.files[1]!.hunks.length, 1)

  assert.equal(changes.totalSteps, 3)
  assert.equal(changes.approxAdded, 18)
  assert.equal(changes.approxRemoved, 4)
  assert.deepEqual(changes.gaps, { partial: false, untrackedCommandCount: 0, historyIncomplete: false })
  assert.equal(shouldShowChangesTrigger(changes), true)
})

test('conta passos execute, edit, delete e move sem diffs como sem registro, ignorando read', () => {
  const entries = [
    entry('u1', 'user', 1, 10, { text: 'rode' }),
    entry('t1', 'tool', 1, 20, { title: 'Read README.md', toolKind: 'read', status: 'completed' }),
    entry('t2', 'tool', 1, 30, { title: 'Run npm test', toolKind: 'execute', status: 'completed' }),
    entry('t3', 'tool', 1, 40, { title: 'Edit config.json', toolKind: 'edit', status: 'completed', diffs: [] }),
    entry('t4', 'tool', 1, 50, { title: 'Delete temp.txt', toolKind: 'delete', status: 'completed' }),
    entry('t5', 'tool', 1, 60, { title: 'Move a.txt', toolKind: 'move', status: 'completed' }),
    entry('r1', 'agent', 1, 70, { text: 'pronto', final: true }),
  ]

  const map = changesByReply(entries)
  const changes = map.get('r1')!
  assert.equal(changes.files.length, 0)
  assert.equal(changes.gaps.untrackedCommandCount, 4)
  assert.equal(changes.gaps.partial, false)
  assert.equal(changes.gaps.historyIncomplete, false)
  assert.equal(shouldShowChangesTrigger(changes), true)
})

test('ignora entradas permission pendentes, aceitas ou recusadas mesmo quando trazem diffs', () => {
  const entries = [
    entry('u1', 'user', 1, 10, { text: 'edite' }),
    entry('p1', 'permission', 1, 20, {
      status: 'pending',
      diffs: [diff('src/recusado.ts', 5, 2, '+nao')],
    }),
    entry('p2', 'permission', 1, 25, {
      status: 'answered',
      selected: 'reject_once',
      diffs: [diff('src/recusado.ts', 5, 2, '+nao')],
    }),
    entry('p3', 'permission', 1, 30, {
      status: 'answered',
      selected: 'allow_once',
      diffs: [diff('src/aceito.ts', 3, 1, '+previa')],
    }),
    entry('t1', 'tool', 1, 35, {
      title: 'Edit src/aceito.ts',
      toolKind: 'edit',
      status: 'completed',
      diffs: [diff('src/aceito.ts', 3, 1, '+execucao')],
    }),
    entry('r1', 'agent', 1, 40, { text: 'ok', final: true }),
    entry('u2', 'user', 2, 50, { text: 'outro' }),
    entry('p4', 'permission', 2, 60, {
      status: 'expired',
      diffs: [diff('src/expirado.ts', 9, 0, '+exp')],
    }),
    entry('r2', 'agent', 2, 70, { text: 'nada feito', final: true }),
  ]

  const map = changesByReply(entries)
  assert.equal(map.has('r2'), false)
  const r1 = map.get('r1')!
  assert.equal(r1.files.length, 1)
  assert.equal(r1.files[0]!.path, 'src/aceito.ts')
  assert.equal(r1.files[0]!.hunks.length, 1)
  assert.equal(r1.approxAdded, 3)
  assert.equal(r1.approxRemoved, 1)
})

test('ignora prévia pré-permissão e passos não concluídos (pending, failed, cancelled) em entradas tool', () => {
  const entries = [
    entry('u1', 'user', 1, 10, { text: 'altere com cuidado' }),
    entry('t-pending-diff', 'tool', 1, 20, {
      title: 'Edit src/previa-pending.ts',
      toolKind: 'edit',
      status: 'pending',
      diffs: [diff('src/previa-pending.ts', 10, 2, '+nao-aplicado')],
    }),
    entry('p-recusada', 'permission', 1, 22, {
      status: 'answered',
      selected: 'reject_once',
      diffs: [diff('src/previa-pending.ts', 10, 2, '+nao-aplicado')],
    }),
    entry('t-failed-diff', 'tool', 1, 25, {
      title: 'Edit src/previa-failed.ts',
      toolKind: 'edit',
      status: 'failed',
      diffs: [diff('src/previa-failed.ts', 7, 1, '+falhou')],
    }),
    entry('t-cancelled-diff', 'tool', 1, 30, {
      title: 'Edit src/previa-cancelled.ts',
      toolKind: 'edit',
      status: 'cancelled',
      diffs: [diff('src/previa-cancelled.ts', 4, 3, '+cancelado')],
    }),
    entry('p-cancelada', 'permission', 1, 32, {
      status: 'cancelled',
      diffs: [diff('src/previa-cancelled.ts', 4, 3, '+cancelado')],
    }),
    entry('t-pending-cmd', 'tool', 1, 35, {
      title: 'Run rm -rf',
      toolKind: 'execute',
      status: 'pending',
    }),
    entry('t-failed-edit', 'tool', 1, 40, {
      title: 'Edit src/sem-diff.ts',
      toolKind: 'edit',
      status: 'failed',
    }),
    entry('t-cancelled-cmd', 'tool', 1, 45, {
      title: 'Run npm run build',
      toolKind: 'execute',
      status: 'cancelled',
    }),
    entry('t-completed-diff', 'tool', 1, 50, {
      title: 'Edit src/confirmado.ts',
      toolKind: 'edit',
      status: 'completed',
      diffs: [diff('src/confirmado.ts', 5, 2, '+ok')],
    }),
    entry('t-completed-cmd', 'tool', 1, 55, {
      title: 'Run npm test',
      toolKind: 'execute',
      status: 'completed',
    }),
    entry('r1', 'agent', 1, 60, { text: 'concluído', final: true }),
  ]

  const map = changesByReply(entries)
  assert.equal(map.size, 1)
  const changes = map.get('r1')!
  assert.deepEqual(changes.files.map((f) => f.path), ['src/confirmado.ts'])
  assert.equal(changes.approxAdded, 5)
  assert.equal(changes.approxRemoved, 2)
  assert.equal(changes.gaps.untrackedCommandCount, 1)
  assert.equal(changes.totalSteps, 2)
})

test('marca partial e truncated no corte por bytes (…) ou acima de 80 linhas', () => {
  const exactly80 = Array.from({ length: DIFF_LINE_LIMIT }, (_, i) => `+l${i + 1}`).join('\n')
  const over80 = Array.from({ length: DIFF_LINE_LIMIT + 1 }, (_, i) => `+l${i + 1}`).join('\n')

  const entries = [
    entry('u1', 'user', 1, 10, { text: 'cortes' }),
    entry('t1', 'tool', 1, 20, {
      toolKind: 'edit',
      status: 'completed',
      diffs: [diff('src/exato.ts', 80, 0, exactly80)],
    }),
    entry('t2', 'tool', 1, 30, {
      toolKind: 'edit',
      status: 'completed',
      diffs: [diff('src/bytes.ts', 10, 0, '+parte\n…')],
    }),
    entry('t3', 'tool', 1, 40, {
      toolKind: 'edit',
      status: 'completed',
      diffs: [diff('src/linhas.ts', 81, 0, over80)],
    }),
    entry('r1', 'agent', 1, 50, { text: 'ok', final: true }),
  ]

  const changes = changesByReply(entries).get('r1')!
  assert.equal(changes.gaps.partial, true)
  assert.equal(changes.files[0]!.hasTruncated, false)
  assert.equal(changes.files[0]!.hunks[0]!.lineCount, 80)
  assert.equal(changes.files[1]!.hasTruncated, true)
  assert.equal(changes.files[1]!.hunks[0]!.truncated, true)
  assert.equal(changes.files[2]!.hasTruncated, true)
  assert.equal(changes.files[2]!.hunks[0]!.lineCount, 81)
  assert.equal(changes.files[2]!.hunks[0]!.truncated, true)
})

test('sinaliza historyIncomplete apenas no menor turno carregado (>0) sem entrada user quando hasOlderHistory é true', () => {
  const entries = [
    entry('t2', 'tool', 2, 10, { toolKind: 'read', status: 'completed' }),
    entry('r2', 'agent', 2, 20, { text: 'fim do turno 2', final: true }),
    entry('u3', 'user', 3, 30, { text: 'novo pedido' }),
    entry('t3', 'tool', 3, 40, { toolKind: 'execute', status: 'completed' }),
    entry('r3', 'agent', 3, 50, { text: 'fim do turno 3', final: true }),
  ]

  const withoutOlder = changesByReply(entries, false)
  assert.equal(withoutOlder.has('r2'), false)
  assert.equal(withoutOlder.get('r3')!.gaps.historyIncomplete, false)

  const withOlder = changesByReply(entries, true)
  const r2 = withOlder.get('r2')!
  assert.equal(r2.files.length, 0)
  assert.equal(r2.gaps.historyIncomplete, true)
  assert.equal(withOlder.get('r3')!.gaps.historyIncomplete, false)

  const withUserInMinTurn = changesByReply([entry('u2', 'user', 2, 5, { text: 'inicio' }), ...entries], true)
  assert.equal(withUserInMinTurn.has('r2'), false)
})

test('mantém isolamento estrito entre sessões do mesmo bot, autores em grupo e turnos diferentes', () => {
  const entries = [
    entry('u1a', 'user', 1, 10, { text: 's1' }, 'sess-1'),
    entry('t1a', 'tool', 1, 20, { author: 'bot-a', toolKind: 'edit', status: 'completed', diffs: [diff('s1.ts', 1, 0)] }, 'sess-1'),
    entry('t1b', 'tool', 1, 25, { author: 'bot-b', toolKind: 'edit', status: 'completed', diffs: [diff('grupo-b.ts', 2, 0)] }, 'sess-1'),
    entry('t1s2', 'tool', 1, 28, { author: 'bot-a', toolKind: 'edit', status: 'completed', diffs: [diff('s2.ts', 3, 0)] }, 'sess-2'),
    entry('t2a', 'tool', 2, 30, { author: 'bot-a', toolKind: 'edit', status: 'completed', diffs: [diff('turno2.ts', 4, 0)] }, 'sess-1'),
    entry('r1a', 'agent', 1, 40, { author: 'bot-a', text: 'r1a', final: true }, 'sess-1'),
    entry('r1b', 'agent', 1, 45, { author: 'bot-b', text: 'r1b', final: true }, 'sess-1'),
    entry('r1s2', 'agent', 1, 50, { author: 'bot-a', text: 'r1s2', final: true }, 'sess-2'),
    entry('r2a', 'agent', 2, 60, { author: 'bot-a', text: 'r2a', final: true }, 'sess-1'),
  ]

  const map = changesByReply(entries)
  assert.deepEqual(map.get('r1a')!.files.map((f) => f.path), ['s1.ts'])
  assert.deepEqual(map.get('r1b')!.files.map((f) => f.path), ['grupo-b.ts'])
  assert.deepEqual(map.get('r1s2')!.files.map((f) => f.path), ['s2.ts'])
  assert.deepEqual(map.get('r2a')!.files.map((f) => f.path), ['turno2.ts'])
})

test('vincula apenas passos concluídos até o corte em resposta interrompida e mantém ao vivo quando há nova tentativa', () => {
  const toolDone = entry('t0', 'tool', 1, 5, {
    author: 'A',
    toolKind: 'edit',
    status: 'completed',
    diffs: [diff('src/antes.ts', 2, 0)],
  })
  const cut = entry('a1', 'agent', 1, 10, { author: 'A', final: false, interrupted: true, text: 'parcial' })
  const toolCut = entry('t1', 'tool', 1, 20, {
    author: 'A',
    toolKind: 'edit',
    status: 'failed',
    interrupted: true,
    diffs: [diff('src/corte.ts', 3, 1)],
  })

  const closedByCut = changesByReply([toolDone, cut, toolCut])
  assert.equal(closedByCut.size, 1)
  assert.deepEqual(closedByCut.get('a1')!.files.map((f) => f.path), ['src/antes.ts'])
  assert.equal(closedByCut.get('a1')!.totalSteps, 1)

  const retryTool = entry('t2', 'tool', 1, 30, {
    author: 'A',
    toolKind: 'edit',
    status: 'in_progress',
    diffs: [diff('src/retry.ts', 5, 0)],
  })
  const live = changesByReply([toolDone, cut, toolCut, retryTool])
  assert.equal(live.size, 0)
})

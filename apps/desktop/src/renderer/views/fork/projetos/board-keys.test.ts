import { test } from 'node:test'
import assert from 'node:assert/strict'
import { PROJECT_TASK_STATES, type ProjectTaskState } from '../../../../shared/fork-projetos.ts'
import { boardKey, type BoardKeyContext, type KeyCard } from './board-keys.ts'

// D541: o handler do teclado do quadro (o mesmo que o grid de ProjetosView chama). ←/→ seguem a direção VISUAL: em RTL
// a coluna seguinte do quadro fica à ESQUERDA, então ← avança e → volta; Alt+seta move pela mesma regra e só se mover
// estiver liberado (sem conexão confirmada ou Projeto arquivado: explica, não grava).

// Quadro: A fazer [a, b] · Em andamento [c] · Em conferência [] · Concluída [d]
const layout: Record<string, ProjectTaskState> = { a: 'backlog', b: 'backlog', c: 'in_progress', d: 'done' }
function board(direction: 'ltr' | 'rtl', canMove = true) {
  let focused = ''
  const moves: string[] = []
  let blocked = 0
  const rows = new Map<ProjectTaskState, number>()
  const cards: KeyCard[] = Object.entries(layout).map(([id, state]) => {
    const row = rows.get(state) ?? 0
    rows.set(state, row + 1)
    return { dataset: { task: id, col: String(PROJECT_TASK_STATES.indexOf(state)), row: String(row) }, focus: () => { focused = id } }
  })
  const ctx: BoardKeyContext = {
    direction, cards: () => cards, stateOf: (id) => layout[id], canMove,
    move: (id, to) => moves.push(`${id}→${to}`), blocked: () => { blocked++ },
  }
  const press = (id: string, key: string, altKey = false) => {
    let prevented = false
    boardKey({ key, altKey, target: cards.find((c) => c.dataset.task === id), preventDefault: () => { prevented = true } }, ctx)
    return prevented
  }
  return { press, focused: () => focused, moves, blocked: () => blocked }
}

test('LTR: → vai à próxima coluna com cartão, ← volta; ↑/↓ na coluna', () => {
  const b = board('ltr')
  b.press('a', 'ArrowRight')
  assert.equal(b.focused(), 'c')
  b.press('c', 'ArrowRight')
  assert.equal(b.focused(), 'd', 'pula a coluna vazia')
  b.press('d', 'ArrowLeft')
  assert.equal(b.focused(), 'c')
  b.press('a', 'ArrowDown')
  assert.equal(b.focused(), 'b')
  b.press('b', 'ArrowUp')
  assert.equal(b.focused(), 'a')
})

test('RTL: as setas seguem a tela — ← avança no quadro, → volta', () => {
  const b = board('rtl')
  b.press('a', 'ArrowLeft')
  assert.equal(b.focused(), 'c', 'em RTL a coluna seguinte fica à esquerda')
  b.press('c', 'ArrowLeft')
  assert.equal(b.focused(), 'd')
  b.press('d', 'ArrowRight')
  assert.equal(b.focused(), 'c')
  b.press('c', 'ArrowRight')
  assert.equal(b.focused(), 'a')
})

test('Alt+seta move para a coluna vizinha na direção visual (LTR e RTL), sem dar a volta', () => {
  const ltr = board('ltr')
  ltr.press('c', 'ArrowRight', true)
  ltr.press('c', 'ArrowLeft', true)
  ltr.press('a', 'ArrowLeft', true)
  assert.deepEqual(ltr.moves, ['c→review', 'c→backlog'])
  const rtl = board('rtl')
  rtl.press('c', 'ArrowLeft', true)
  rtl.press('c', 'ArrowRight', true)
  rtl.press('a', 'ArrowRight', true)
  assert.deepEqual(rtl.moves, ['c→review', 'c→backlog'])
})

test('mover bloqueado (offline, sem autoridade ou arquivado): Alt+seta explica e não grava; navegar continua', () => {
  for (const dir of ['ltr', 'rtl'] as const) {
    const b = board(dir, false)
    assert.equal(b.press('c', 'ArrowRight', true), true)
    assert.equal(b.press('c', 'ArrowLeft', true), true)
    assert.deepEqual(b.moves, [])
    assert.equal(b.blocked(), 2)
    b.press('a', dir === 'ltr' ? 'ArrowRight' : 'ArrowLeft')
    assert.equal(b.focused(), 'c', 'só leitura ainda navega')
  }
})

test('tecla que não é seta ou foco fora de um cartão: nada é tocado', () => {
  const b = board('ltr')
  assert.equal(b.press('a', 'Enter'), false)
  let prevented = false
  boardKey({ key: 'ArrowRight', altKey: true, target: { dataset: {}, focus() {} }, preventDefault: () => { prevented = true } },
    { direction: 'ltr', cards: () => [], stateOf: () => 'backlog', canMove: true, move: () => assert.fail('moveu'), blocked: () => assert.fail('bloqueou') })
  assert.equal(prevented, false)
})

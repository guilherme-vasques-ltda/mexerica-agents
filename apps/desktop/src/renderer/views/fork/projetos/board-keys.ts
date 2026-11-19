import { neighborState, PROJECT_TASK_STATES, type ProjectTaskState } from '../../../../shared/fork-projetos.ts'

// D541: o teclado do quadro (o handler `onKeyDown` do grid em ProjetosView chama ESTA função, sem cópia). ↑/↓ andam na
// coluna, ←/→ entre colunas e Alt+←/→ movem a tarefa para a coluna vizinha. ←/→ seguem a direção VISUAL: em RTL o grid
// corre da direita para a esquerda, então → leva à coluna anterior na ordem do quadro. Sem DOM próprio: recebe o
// mínimo (elemento focado, cartões do grid, direção calculada), para os testes do Node exercitarem o handler real.

export interface KeyCard { dataset: { task?: string; col?: string; row?: string }; focus(): void }
export interface BoardKeyEvent { key: string; altKey: boolean; target: unknown; preventDefault(): void }
export interface BoardKeyContext {
  direction: 'ltr' | 'rtl'
  /** Os cartões do grid (botões com `data-task`, `data-col`, `data-row`). */
  cards: () => readonly KeyCard[]
  stateOf: (taskId: string) => ProjectTaskState | undefined
  /** Mover liberado (gravação liberada e Projeto ativo); o handler confere antes de qualquer pedido. */
  canMove: boolean
  move: (taskId: string, to: ProjectTaskState) => void
  /** Alt+seta com mover bloqueado: a tela explica em vez de não fazer nada. */
  blocked: () => void
}

const ARROWS = ['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight']

export function boardKey(e: BoardKeyEvent, ctx: BoardKeyContext) {
  const el = e.target as KeyCard | null
  const id = el?.dataset?.task
  if (!el || !id || !ARROWS.includes(e.key)) return
  const state = ctx.stateOf(id)
  if (!state) return
  e.preventDefault()
  const dir: -1 | 1 = (e.key === 'ArrowLeft') !== (ctx.direction === 'rtl') ? -1 : 1
  if (e.altKey && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
    if (!ctx.canMove) return ctx.blocked()
    const next = neighborState(state, dir)
    if (next) ctx.move(id, next)
    return
  }
  const col = Number(el.dataset.col)
  const row = Number(el.dataset.row)
  const all = ctx.cards()
  if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
    all.find((x) => Number(x.dataset.col) === col && Number(x.dataset.row) === row + (e.key === 'ArrowDown' ? 1 : -1))?.focus()
    return
  }
  for (let c = col + dir; c >= 0 && c < PROJECT_TASK_STATES.length; c += dir) {
    const inCol = all.filter((x) => Number(x.dataset.col) === c)
    if (inCol.length) {
      inCol[Math.min(row, inCol.length - 1)]?.focus()
      return
    }
  }
}

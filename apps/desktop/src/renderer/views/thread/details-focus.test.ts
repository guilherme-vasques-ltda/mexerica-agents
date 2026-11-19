import { test } from 'node:test'
import assert from 'node:assert/strict'
import { returnFocusAfterClose } from './details-focus.ts'

// D169 R6: o retorno de foco pertence a um fechamento só; reabrir ou sair o invalida, e um foco que o
// usuário moveu depois fica onde está.
type Node = { isConnected: boolean; closest: (selector: string) => unknown }

function setup() {
  const body: Node = { isConnected: true, closest: () => null }
  const panel: Node = { isConnected: true, closest: (s: string) => (s === '.details-panel' ? {} : null) }
  const textarea: Node = { isConnected: true, closest: () => null }
  const state = { active: body as Node | null, frames: [] as (() => void)[], cancelled: [] as number[], focused: 0 }
  const target = { isConnected: true, focus: () => { state.focused++ } }
  const env = {
    active: () => state.active,
    body: () => body,
    frame: (f: () => void) => state.frames.push(f),
    cancel: (id: number) => { state.cancelled.push(id) },
  }
  return { body, panel, textarea, state, target, env }
}

test('foco ao fechar Detalhes: volta ao alvo quando o foco ficou perdido', () => {
  const { state, target, env } = setup()
  returnFocusAfterClose(target, env)
  state.frames.forEach((f) => f())
  assert.equal(state.focused, 1)
})

test('foco ao fechar Detalhes: reabrir antes do quadro invalida o retorno antigo', () => {
  const { state, target, env, panel } = setup()
  const cancel = returnFocusAfterClose(target, env)
  cancel()
  state.active = panel
  // Mesmo entregue depois do cancelamento, o quadro antigo não mexe no foco do painel novo.
  state.frames.forEach((f) => f())
  assert.equal(state.focused, 0)
  assert.equal(state.cancelled.length, 1)
})

test('foco ao fechar Detalhes: intenção posterior e alvo removido são preservados', () => {
  const moved = setup()
  returnFocusAfterClose(moved.target, moved.env)
  moved.state.active = moved.textarea
  moved.state.frames.forEach((f) => f())
  assert.equal(moved.state.focused, 0)

  const gone = setup()
  const target = { isConnected: false, focus: () => { gone.state.focused++ } }
  returnFocusAfterClose(target, gone.env)
  gone.state.frames.forEach((f) => f())
  assert.equal(gone.state.focused, 0)
})

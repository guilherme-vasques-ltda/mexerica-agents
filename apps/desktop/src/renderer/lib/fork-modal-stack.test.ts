// Adaptado de Codync 219935410dffccb950f90d0565bd1d9f1661bd5b, Apache-2.0.
// leepokai; coautoria histórica Claude Opus 5.5. Modificado pela DONNA Bot em 2026-11-06.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { stackModals, type PageLayer } from './fork-modal-stack.ts'

const layer = (el: string, props: Partial<PageLayer<string>> = {}): PageLayer<string> => ({ el, z: 0, modal: false, inert: false, muted: false, ...props })

test('z-index decide antes da ordem DOM; empate escolhe o último modal', () => {
  for (const order of [['sheet', 'dialog'], ['dialog', 'sheet']]) {
    const result = stackModals([layer('root'), ...order.map((el) => layer(el, { modal: true, z: el === 'dialog' ? 200 : 100 }))])
    assert.equal(result.top, 'dialog')
    assert.deepEqual(result.mute.sort(), ['root', 'sheet'])
  }
  assert.equal(stackModals([layer('a', { modal: true, z: 100 }), layer('b', { modal: true, z: 100 })]).top, 'b')
})

test('fechamento restaura próxima folha, mantendo a saída inerte', () => {
  const result = stackModals([layer('root', { inert: true, muted: true }), layer('sheet', { z: 100, modal: true, inert: true, muted: true }), layer('dialog', { z: 200, exiting: true })])
  assert.equal(result.top, 'sheet')
  assert.deepEqual(result.unmute, ['sheet'])
  assert.deepEqual(result.mute, ['dialog'])
})

test('menu posterior acima do topo funciona; menu anterior e camada visual inferior cedem', () => {
  const result = stackModals([layer('old-menu', { z: 300 }), layer('sheet', { z: 100, modal: true }), layer('menu', { z: 300, inert: true, muted: true }), layer('behind', { z: 50 })])
  assert.deepEqual(result.unmute, ['menu'])
  assert.deepEqual(result.mute, ['old-menu', 'behind'])
})

test('último fechamento restaura só inert próprio e conserva saída montada', () => {
  const result = stackModals([layer('root', { inert: true, muted: true }), layer('external', { inert: true }), layer('exit', { inert: true, muted: true, exiting: true })])
  assert.equal(result.top, null)
  assert.deepEqual(result.unmute, ['root'])
  assert.deepEqual(result.mute, [])
})

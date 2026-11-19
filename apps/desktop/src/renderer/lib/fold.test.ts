import { test } from 'node:test'
import assert from 'node:assert/strict'
import { FOLD_CLOSE, FOLD_OPEN, foldKeyframes, WarmQueue } from './fold.ts'

test('dobra: abre e fecha inteira com as curvas do contrato; no meio, inverte a partir de onde está', () => {
  assert.deepEqual(foldKeyframes({ height: 0, opacity: 0 }, 120, true), { keyframes: [{ height: '0px', opacity: 0 }, { height: '120px', opacity: 1 }], options: FOLD_OPEN })
  assert.deepEqual(foldKeyframes({ height: 120, opacity: 1 }, 120, false), { keyframes: [{ height: '120px', opacity: 1 }, { height: '0px', opacity: 0 }], options: FOLD_CLOSE })
  // Segundo clique durante o fechamento (A355-1): reabre da altura e da opacidade atuais, sem alongar a animação.
  const back = foldKeyframes({ height: 48, opacity: 0.4 }, 120, true)
  assert.deepEqual(back.keyframes[0], { height: '48px', opacity: 0.4 })
  assert.deepEqual(back.keyframes[1], { height: '120px', opacity: 1 })
  assert.equal(back.options.duration, 240)
  assert.equal(foldKeyframes({ height: 70, opacity: 0.6 }, 120, false).options.duration, 180)
})

test('montagem antes de abrir: um corpo por vez no ocioso, sem repetir; o ponteiro adianta; desmontado sai da fila', () => {
  const idle: (() => void)[] = []
  const q = new WarmQueue((run) => { idle.push(run) })
  const warmed: string[] = []
  q.add('a', () => warmed.push('a'))
  q.add('b', () => warmed.push('b'))
  q.add('a', () => warmed.push('a de novo'))
  q.add('c', () => warmed.push('c'))
  assert.equal(idle.length, 1, 'uma chamada ociosa por vez')
  q.now('c')
  assert.deepEqual(warmed, ['c'], 'ponteiro no título monta já')
  q.now('c')
  assert.deepEqual(warmed, ['c'], 'monta uma vez só')
  idle.shift()!()
  assert.deepEqual(warmed, ['c', 'a'])
  q.drop('b')
  assert.equal(q.size, 0)
  while (idle.length) idle.shift()!()
  assert.deepEqual(warmed, ['c', 'a'], 'quem desmontou não monta')
})

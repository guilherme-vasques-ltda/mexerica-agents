import assert from 'node:assert/strict'
import test from 'node:test'
import { clickIntent, redirectMs, staggerDelays, staggerEnd } from './movimento.ts'

test('A352-2: 26 ms entre abas; traços entram com a aba seguinte; "···" e "+" como a próxima', () => {
  // csep, aba, traço, aba, traço, aba, traço, aba, traço, aba, traço, aba, ···
  const kinds = ['sep', 'tab', 'sep', 'tab', 'sep', 'tab', 'sep', 'tab', 'sep', 'tab', 'sep', 'tab', 'tail'] as const
  const delays = staggerDelays(kinds)
  assert.deepEqual(delays.filter((_, i) => kinds[i] === 'tab'), [0, 26, 52, 78, 104, 130])
  assert.equal(delays[2], 26)
  assert.equal(delays.at(-1), 156)
  // O fim é a última entrada inteira (156 + 280), não os 320 ms da largura.
  assert.equal(staggerEnd(delays, 280), 436)
})

test('A352-3: o clique no meio vale e o caminho de volta parte do ponto atual', () => {
  let intent = true
  intent = clickIntent(intent) // ocultar
  intent = clickIntent(intent) // mostrar de novo, no meio
  assert.equal(intent, true)
  // Cartão em 700 px, voltando para a pílula de 128: falta 572 de 1220 → 150 ms.
  assert.equal(redirectMs(700, 128, 1220, 320), 150)
  // Quase lá: nunca abaixo do mínimo; distância cheia: a duração inteira.
  assert.equal(redirectMs(140, 128, 1220, 320), 120)
  assert.equal(redirectMs(1348, 128, 1220, 320), 320)
})

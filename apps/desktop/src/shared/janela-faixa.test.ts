import assert from 'node:assert/strict'
import test from 'node:test'
import { coresControles, FAIXA_ALTURA, hexOpaco, logoComNome, SEMAFOROS } from './janela-faixa.ts'

test('faixa de 29 px com os botões do Mac centrados', () => {
  assert.equal(FAIXA_ALTURA, 29)
  // Botões de 12 px: o centro fica no meio da faixa (meio pixel de tolerância).
  assert.ok(Math.abs(SEMAFOROS.y + 6 + 0.5 - FAIXA_ALTURA / 2) <= 0.5)
})

test('o nome aparece só com 16 px livres entre o seletor e a logo; senão, só a fruta', () => {
  // Janela mínima (760 px), seletor no máximo (78 + 200 px): o nome ainda cabe.
  assert.equal(logoComNome(760, 78 + 200), true)
  // A conta exata: 380 - 44,5 - 16 = 319,5.
  assert.equal(logoComNome(760, 319.5), true)
  assert.equal(logoComNome(760, 320), false)
  // Zoom do app (a faixa encolhe em px CSS): vira a fruta.
  assert.equal(logoComNome(560, 278), false)
})

test('cores dos controles do sistema: só #rrggbb, nada além', () => {
  assert.deepEqual(coresControles({ color: '#F4F1EA', symbolColor: '#1b1511' }), { color: '#f4f1ea', symbolColor: '#1b1511' })
  for (const ruim of [null, 'x', {}, { color: 'red', symbolColor: '#000000' }, { color: '#fff', symbolColor: '#000000' }, { color: '#000000', symbolColor: 'url(x)' }]) {
    assert.equal(coresControles(ruim), null)
  }
})

test('cor calculada vira #rrggbb opaco, composta sobre o fundo quando translúcida', () => {
  assert.equal(hexOpaco('rgb(10, 20, 30)'), '#0a141e')
  assert.equal(hexOpaco('rgba(0, 0, 0, 0.5)', 'rgb(255, 255, 255)'), '#808080')
  assert.equal(hexOpaco('rgb(0 0 0 / 50%)', 'rgb(200, 200, 200)'), '#646464')
  assert.equal(hexOpaco('transparent'), null)
})

test('a altura da faixa é a mesma no CSS (token) e no main (titleBarOverlay)', async () => {
  const { readFileSync } = await import('node:fs')
  const css = readFileSync(new URL('../renderer/styles/tokens.css', import.meta.url), 'utf8')
  assert.match(css, new RegExp(`--janela-header-h: ${FAIXA_ALTURA}px;`))
})

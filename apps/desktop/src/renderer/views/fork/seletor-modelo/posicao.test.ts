import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ALTURA_SELETOR, LARGURA_SELETOR, MARGEM, posicaoLista, TAMANHO_MENU_APROVACAO, type Posicao } from './posicao.ts'

// D473: a lista abre presa ao chip e cabe inteira na janela (nenhuma borda para fora).
const cabe = (p: Posicao, largura: number, altura: number) => {
  assert.ok(p.left >= MARGEM && p.left + p.width <= largura - MARGEM, 'cabe na largura')
  const topo = 'top' in p ? p.top : altura - p.bottom - p.maxHeight
  const base = 'top' in p ? p.top + p.maxHeight : altura - p.bottom
  assert.ok(topo >= MARGEM && base <= altura - MARGEM, 'cabe na altura')
}

test('janela larga: abre acima do chip, alinhada à borda dele, no tamanho inteiro', () => {
  const chip = { top: 834, bottom: 856, left: 532 }
  const p = posicaoLista(chip, { largura: 1440, altura: 900 })
  assert.equal(p.left, 532)
  assert.equal(p.width, LARGURA_SELETOR)
  assert.equal(p.maxHeight, ALTURA_SELETOR)
  assert.ok('bottom' in p && p.bottom === 900 - 834 + 6)
  cabe(p, 1440, 900)
})

test('chip perto da borda direita: a lista recua para dentro da janela', () => {
  const p = posicaoLista({ top: 834, bottom: 856, left: 1300 }, { largura: 1440, altura: 900 })
  assert.equal(p.left, 1440 - LARGURA_SELETOR - MARGEM)
  cabe(p, 1440, 900)
})

test('janela estreita: a largura encolhe e a lista não passa das bordas', () => {
  const p = posicaoLista({ top: 834, bottom: 856, left: 126 }, { largura: 520, altura: 900 })
  assert.equal(p.width, 520 - 2 * MARGEM)
  assert.equal(p.left, MARGEM)
  cabe(p, 520, 900)
})

test('janela baixa: a altura vira a sobra acima do chip, com rolagem interna', () => {
  const p = posicaoLista({ top: 300, bottom: 322, left: 100 }, { largura: 760, altura: 340 })
  assert.ok('bottom' in p)
  assert.equal(p.maxHeight, 300 - 6 - MARGEM)
  cabe(p, 760, 340)
})

test('chip no alto da janela: abre para baixo', () => {
  const p = posicaoLista({ top: 40, bottom: 62, left: 100 }, { largura: 760, altura: 900 })
  assert.ok('top' in p && p.top === 68)
  assert.equal(p.maxHeight, ALTURA_SELETOR)
  cabe(p, 760, 900)
})

test('menu da mãozinha: preso ao chip dela, acima da caixa do campo, cabendo em janela larga e estreita', () => {
  // A âncora já vem com o topo da caixa do campo (ancoraNoCampo): o menu termina acima dela.
  const mao = { top: 822, bottom: 894, left: 74 }
  for (const largura of [1440, 760, 320]) {
    const p = posicaoLista(mao, { largura, altura: 900 }, TAMANHO_MENU_APROVACAO)
    assert.ok('bottom' in p && p.bottom === 900 - 822 + 6, 'acima da caixa')
    assert.equal(p.width, Math.min(TAMANHO_MENU_APROVACAO.largura, largura - 2 * MARGEM))
    assert.equal(p.left, Math.max(MARGEM, Math.min(74, largura - p.width - MARGEM)))
    cabe(p, largura, 900)
  }
  // Janela baixa: o menu encolhe para a sobra acima da caixa e rola por dentro.
  const baixa = posicaoLista({ top: 322, bottom: 394, left: 74 }, { largura: 760, altura: 400 }, TAMANHO_MENU_APROVACAO)
  assert.equal(baixa.maxHeight, 322 - 6 - MARGEM)
  cabe(baixa, 760, 400)
})

test('R2c: lista de modelos em janela de 320 px: largura encolhe e cabe inteira, presa à borda', () => {
  const p = posicaoLista({ top: 700, bottom: 720, left: 74 }, { largura: 320, altura: 800 })
  assert.equal(p.width, 320 - 2 * MARGEM)
  assert.equal(p.left, MARGEM)
  assert.ok('bottom' in p)
  cabe(p, 320, 800)
})

import assert from 'node:assert/strict'
import { test } from 'node:test'
import { ALTURA_LINHA, FOLGA_LINHAS, janela, LINHAS_A_VISTA, proximaLinha, rolagemPara } from './virtual.ts'

test('janela: só as linhas à vista e a folga, em qualquer rolagem', () => {
  assert.deepEqual(janela(0, 400), { inicio: 0, fim: LINHAS_A_VISTA + 1 + FOLGA_LINHAS })
  assert.deepEqual(janela(100 * ALTURA_LINHA + 5, 400), { inicio: 96, fim: 113 })
  // No fim (e além dele, como logo depois de a lista encolher), a janela fica nas últimas linhas.
  assert.deepEqual(janela(392 * ALTURA_LINHA, 400), { inicio: 388, fim: 400 })
  assert.deepEqual(janela(9999 * ALTURA_LINHA, 400), { inicio: 388, fim: 400 })
  // Lista curta ou vazia: tudo, sem índice negativo.
  assert.deepEqual(janela(0, 3), { inicio: 0, fim: 3 })
  assert.deepEqual(janela(500, 3), { inicio: 0, fim: 3 })
  assert.deepEqual(janela(0, 0), { inicio: 0, fim: 0 })
  for (const topo of [0, 31, 32, 1000, 12_544]) {
    const { inicio, fim } = janela(topo, 400)
    assert.ok(fim - inicio <= LINHAS_A_VISTA + 1 + 2 * FOLGA_LINHAS, 'nunca mais que a janela')
    assert.ok(inicio * ALTURA_LINHA <= topo && fim * ALTURA_LINHA >= Math.min(topo + LINHAS_A_VISTA * ALTURA_LINHA, 400 * ALTURA_LINHA), 'cobre a área visível')
  }
})

test('rolagemPara: mexe o mínimo para a linha ficar inteira à vista', () => {
  assert.equal(rolagemPara(3, 0, 256), 0, 'já à vista')
  assert.equal(rolagemPara(8, 0, 256), 32, 'logo abaixo: uma linha')
  assert.equal(rolagemPara(399, 0, 256), 400 * 32 - 256, 'a última: o fim da lista')
  assert.equal(rolagemPara(2, 320, 256), 64, 'acima: ela vira a primeira')
  assert.equal(rolagemPara(0, 500, 256), 0)
  // Com um aviso de 40px antes do bloco, a conta inclui esse espaço; janela baixa (4 linhas) também.
  assert.equal(rolagemPara(0, 100, 256, 40), 40)
  assert.equal(rolagemPara(5, 0, 128), 64)
})

test('proximaLinha: pula nome de motor e linha desativada, dá a volta e começa da busca pela primeira', () => {
  // 0 = nome de motor, 3 = desativada.
  const serve = (i: number) => i !== 0 && i !== 3
  assert.equal(proximaLinha(-1, 1, 6, serve), 1, 'da busca, para baixo')
  assert.equal(proximaLinha(-1, -1, 6, serve), 1, 'da busca, para cima: também a primeira')
  assert.equal(proximaLinha(2, 1, 6, serve), 4)
  assert.equal(proximaLinha(4, -1, 6, serve), 2)
  assert.equal(proximaLinha(5, 1, 6, serve), 1, 'do fim volta ao começo')
  assert.equal(proximaLinha(1, -1, 6, serve), 5, 'do começo vai ao fim')
  assert.equal(proximaLinha(2, 1, 6, () => false), -1)
  assert.equal(proximaLinha(-1, 1, 0, serve), -1)
  assert.equal(proximaLinha(0, 1, 1, () => true), 0, 'uma linha só')
})

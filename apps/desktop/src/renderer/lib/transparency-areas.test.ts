import { test } from 'node:test'
import assert from 'node:assert/strict'
import { contrast, effectiveGlass, glassAlpha, layerGround, legibleOn, safeFloor, type RGB } from './legibility.ts'
import { areaChoice, conversationPlate, detailsLayer, followsGeneral, isAreaValues, isTransparencyMode, withArea } from './transparency-areas.ts'

// D169 (personalizacao.md, Transparência por área): Geral põe um valor nas quatro áreas; Por área deixa
// cada uma com o seu, e a que não tem segue a geral.

test('Geral usa o valor geral em todas as áreas, mesmo com valores próprios guardados', () => {
  const own = { sidebar: 10, composer: 90 }
  for (const area of ['sidebar', 'details', 'composer', 'conversation'] as const) {
    assert.equal(areaChoice(area, 'general', 80, own), 80)
    assert.equal(followsGeneral(area, 'general', own), true)
  }
})

test('Por área: a área ajustada usa o seu valor, as outras seguem a geral; Usar a geral apaga só aquela', () => {
  const own = withArea(withArea({}, 'composer', 90), 'sidebar', 10)
  assert.equal(areaChoice('composer', 'custom', 50, own), 90)
  assert.equal(areaChoice('sidebar', 'custom', 50, own), 10)
  assert.equal(areaChoice('details', 'custom', 50, own), 50)
  assert.equal(followsGeneral('details', 'custom', own), true)
  const reset = withArea(own, 'composer', null)
  assert.deepEqual(reset, { sidebar: 10 })
  assert.equal(areaChoice('composer', 'custom', 50, reset), 50)
  // Valores fora da escala são presos e arredondados; o objeto original não muda.
  assert.deepEqual(withArea({}, 'details', 140.4), { details: 100 })
  assert.deepEqual(withArea({}, 'details', -3), { details: 0 })
  assert.deepEqual(own, { composer: 90, sidebar: 10 })
})

test('persistência: só modos e áreas válidos são aceitos; o resto volta ao padrão sem reset do valor geral', () => {
  assert.ok(isTransparencyMode('general') && isTransparencyMode('custom'))
  for (const bad of ['Geral', null, 1, undefined]) assert.equal(isTransparencyMode(bad), false)
  assert.ok(isAreaValues({}) && isAreaValues({ sidebar: 0, details: 100, composer: 55, conversation: 7 }))
  for (const bad of [null, [], 'x', { sidebar: 101 }, { sidebar: 1.5 }, { sidebar: '10' }, { bolhas: 10 }]) assert.equal(isAreaValues(bad), false)
  // Um valor geral antigo, sem modo nem áreas guardados, vale nas quatro áreas.
  assert.equal(areaChoice('details', 'general', 73, {}), 73)
})

test('fundo da conversa: placa opaca em 0%, nenhuma em 100% (como antes)', () => {
  assert.equal(conversationPlate(0), 1)
  assert.equal(conversationPlate(100), 0)
  assert.equal(conversationPlate(50), 0.5)
  assert.equal(conversationPlate(250), 0)
})

test('painel de detalhes no fundo extremo: o piso segura o vidro só para o texto principal; o secundário fica a 4,5:1 pela cor', () => {
  // Tokens reais (tokens.css): glass-raised, ink e ink-2 no claro e no escuro. D169 (pedido do dono 28/10): antes o
  // piso segurava ink-2, e o vidro ficava quase opaco em qualquer fundo; agora segura ink e o ink-2 se ajusta.
  const themes: { dark: boolean; worst: RGB; tokens: Record<string, RGB> }[] = [
    { dark: false, worst: [0, 0, 0], tokens: { 'glass-raised': [255, 255, 255], ink: [23, 22, 26], 'ink-2': [92, 90, 99] } },
    { dark: true, worst: [255, 255, 255], tokens: { 'glass-raised': [30, 30, 35], ink: [244, 243, 241], 'ink-2': [169, 167, 176] } },
  ]
  for (const { dark, worst, tokens } of themes) {
    const t = (name: string) => tokens[name]!
    const layer = detailsLayer(t)
    const floor = safeFloor(worst, [layer], t('ink'))
    assert.ok(floor < safeFloor(worst, [layer], t('ink-2')), 'o piso do texto principal é mais baixo: dá para ver o fundo')
    const glass = effectiveGlass(glassAlpha(100), floor, { moreContrast: false, reduceTransparency: false })
    const ground = layerGround(worst, layer, glass)
    assert.ok(contrast(t('ink'), ground) >= 4.5, 'texto principal a 4,5:1')
    const secondary = legibleOn(t('ink-2'), [ground], dark)
    assert.ok(contrast(secondary, ground) >= 4.5, 'secundário a 4,5:1 pela própria cor')
    // Reduzir transparência deixa o painel opaco, e o secundário opaco não precisa mudar.
    assert.equal(effectiveGlass(glassAlpha(100), floor, { moreContrast: false, reduceTransparency: true }), 1)
    assert.deepEqual(legibleOn(t('ink-2'), [layerGround(worst, layer, 1)], dark), t('ink-2'))
  }
})

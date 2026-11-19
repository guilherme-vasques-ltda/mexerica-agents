import { test } from 'node:test'
import assert from 'node:assert/strict'
import { channelBounds, contrast, controlGlassAlpha, effectiveGlass, foregroundOn, glassAlpha, layerGround, legibleOn, luminance, over, readableForeground, safeFloor, saturatedBounds, transparencyFor, veiledBounds, worstOf, type RGB } from './legibility.ts'

// D165 (revisão A, F2): the floor holds for any crop, scale or blur of the picture, because every color
// the window shows is an average of the picture's pixels and so lies in its per-channel box.
const canvas: RGB = [251, 250, 247]
const sidebar: RGB = [242, 241, 237]
const ink: RGB = [23, 22, 26]
const ink2: RGB = [92, 90, 99]
const darkSidebar: RGB = [20, 20, 23]
const darkInk2: RGB = [169, 167, 176]
const layers = [{ color: sidebar, offset: 0, tint: { color: ink, alpha: 0.1 } }, { color: canvas, offset: 0 }]

test('faixa conserva controles em 3:1 sobre fundo extremo sem aumentar o piso das outras superfícies', () => {
  const rail = { color: [233, 232, 227] as RGB, offset: 0, tint: { color: ink, alpha: 0.08 } }
  const selectedSidebar = 0.59
  assert.ok(contrast(ink, layerGround([0, 0, 0], rail, selectedSidebar * 0.7)) < 3)
  const actual = controlGlassAlpha(selectedSidebar * 0.7, [0, 0, 0], rail, ink)
  assert.ok(contrast(ink, layerGround([0, 0, 0], rail, actual)) >= 3)
  assert.ok(actual < selectedSidebar, 'piso próprio só da faixa, não da sidebar/caixa')
  assert.equal(controlGlassAlpha(1, [0, 0, 0], rail, ink), 1)
  const darkInk: RGB = [244, 243, 241]
  const darkRail = { color: [8, 8, 10] as RGB, offset: 0, tint: { color: darkInk, alpha: 0.1 } }
  const dark = controlGlassAlpha(0.4, [255, 255, 255], darkRail, darkInk)
  assert.ok(contrast(darkInk, layerGround([255, 255, 255], darkRail, dark)) >= 3)
})

test('foregrounds semânticos respeitam o limite de ink-2 sem alterar cores já seguras', () => {
  // Actual light tokens, including every accent, attention, danger and success (D165 followup).
  const colors: RGB[] = [[180, 73, 0], [54, 117, 31], [70, 29, 251], [31, 95, 224], [13, 122, 95], [154, 86, 0], [194, 24, 91], [43, 43, 48], [138, 90, 0], [194, 58, 43], [46, 125, 50]]
  const floor = safeFloor([0, 0, 0], layers, ink2)
  for (const color of colors) {
    const actual = readableForeground(color, ink2, false)
    assert.ok(luminance(actual) <= luminance(ink2))
    for (const layer of layers) {
      const ground = over(layer.color, floor, [0, 0, 0])
      assert.ok(contrast(actual, layer.tint ? over(layer.tint.color, layer.tint.alpha, ground) : ground) >= 4.5)
    }
    if (luminance(color) <= luminance(ink2)) assert.deepEqual(actual, color)
  }
  const darkAccent: RGB = [131, 181, 255]
  assert.deepEqual(readableForeground(darkAccent, darkInk2, true), darkAccent)
  const raised = readableForeground([90, 80, 70], darkInk2, true)
  assert.ok(luminance(raised) >= luminance(darkInk2))
})

test('opacidade 1 não é declarada suficiente quando o texto ainda falha', () => {
  const success: RGB = [46, 125, 50]
  assert.ok(contrast(success, over(ink, 0.1, sidebar)) < 4.5)
  assert.throws(() => safeFloor([0, 0, 0], layers, success), /nem com a camada opaca/)
})

test('Donna selecionada + hover compõe 12.6%/16.3% e ajusta também o texto secondary', () => {
  const lightGround = over(ink, 1 - (1 - 0.08) * (1 - 0.05), over(sidebar, 0.96, [0, 0, 0]))
  for (const color of [ink2, [125, 82, 0] as RGB]) {
    assert.ok(contrast(color, lightGround) < 4.5, 'contraexemplo anterior reproduzido')
    assert.ok(contrast(foregroundOn(color, lightGround, false), lightGround) >= 4.5)
  }
  const darkGround = over([244, 243, 241], 1 - (1 - 0.10) * (1 - 0.07), over(darkSidebar, 0.91, [255, 255, 255]))
  assert.ok(contrast(foregroundOn(darkInk2, darkGround, true), darkGround) >= 4.5)
  assert.throws(() => foregroundOn(ink2, [20, 20, 20], false), /não permite texto legível/)
})

function banner() {
  // The reviewer's case: 2880 × 100 white with a 20 px black stripe at x = 1370.
  const w = 2880
  const h = 4
  const px = new Uint8ClampedArray(w * h * 4)
  for (let i = 0; i < w * h; i++) {
    const black = (i % w) >= 1370 && (i % w) < 1390
    px.set(black ? [0, 0, 0, 255] : [255, 255, 255, 255], i * 4)
  }
  return px
}

test('uma faixa estreita entra na caixa por canal, por menor que seja na amostra', () => {
  const b = channelBounds(banner(), canvas)
  assert.deepEqual(b.lo, [0, 0, 0])
  assert.deepEqual(b.hi, [255, 255, 255])
})

test('o piso seguro deixa ink-2 em 4.5:1 no pior ponto, com tinta de linha e saturação do vidro', () => {
  const shown = saturatedBounds(veiledBounds(channelBounds(banner(), canvas), [245, 244, 240], 0), 1.4)
  const worst = worstOf(shown, false)
  const floor = safeFloor(worst, layers, ink2)
  for (const layer of layers) {
    const ground = over(layer.color, floor, worst)
    assert.ok(contrast(ink2, layer.tint ? over(layer.tint.color, layer.tint.alpha, ground) : ground) >= 4.5, `piso ${floor}`)
  }
  assert.ok(floor > 0.55, 'a faixa preta segura o vidro acima do mínimo do controle')
})

test('a saturação do vidro fica dentro da caixa calculada para qualquer cor da caixa', () => {
  const b = { lo: [40, 120, 60] as RGB, hi: [200, 140, 230] as RGB }
  const out = saturatedBounds(b, 1.4)
  const sat = ([r, g, bl]: RGB): RGB => [
    (0.213 + 0.787 * 1.4) * r + (0.715 - 0.715 * 1.4) * g + (0.072 - 0.072 * 1.4) * bl,
    (0.213 - 0.213 * 1.4) * r + (0.715 + 0.285 * 1.4) * g + (0.072 - 0.072 * 1.4) * bl,
    (0.213 - 0.213 * 1.4) * r + (0.715 - 0.715 * 1.4) * g + (0.072 + 0.928 * 1.4) * bl,
  ].map((v) => Math.min(255, Math.max(0, v))) as unknown as RGB
  for (const r of [40, 120, 200]) for (const g of [120, 130, 140]) for (const bl of [60, 150, 230]) {
    const c = sat([r, g, bl])
    for (let i = 0; i < 3; i++) assert.ok(c[i]! >= out.lo[i]! - 1e-9 && c[i]! <= out.hi[i]! + 1e-9)
  }
})

test('no escuro o pior é o mais claro, e o piso também vale', () => {
  const b = { lo: [10, 10, 10] as RGB, hi: [250, 250, 250] as RGB }
  const worst = worstOf(saturatedBounds(b, 1.4), true)
  const floor = safeFloor(worst, [{ color: darkSidebar, offset: 0 }], darkInk2)
  assert.ok(contrast(darkInk2, over(darkSidebar, floor, worst)) >= 4.5)
})

test('D169 28/10: no pior fundo o vidro segurado pelo texto principal fica bem mais claro, e o secundário/terciário seguem legíveis pela cor', () => {
  // A faixa preta (pior caso do claro): o piso antigo (ink-2) prendia o vidro; o novo (ink) deixa a escolha valer.
  const worst = worstOf(saturatedBounds(veiledBounds(channelBounds(banner(), canvas), [245, 244, 240], 0.18), 1.4), false)
  const old = safeFloor(worst, layers, ink2)
  const floor = safeFloor(worst, layers, ink)
  assert.ok(floor + 0.2 < old, `piso ${floor} contra ${old}`)
  const glass = effectiveGlass(glassAlpha(100), floor, { moreContrast: false, reduceTransparency: false })
  const grounds = layers.map((l) => layerGround(worst, l, glass))
  for (const g of grounds) assert.ok(contrast(ink, g) >= 4.5)
  const secondary = legibleOn(ink2, grounds, false)
  const tertiary = legibleOn([140, 138, 146], grounds, false, 3)
  for (const g of grounds) {
    assert.ok(contrast(secondary, g) >= 4.5)
    assert.ok(contrast(tertiary, g) >= 3)
  }
  assert.ok(luminance(secondary) <= luminance(ink2), 'só escurece, nunca clareia no tema claro')
  // Sobre vidro opaco a cor do secundário não muda.
  assert.deepEqual(legibleOn(ink2, layers.map((l) => layerGround(worst, l, 1)), false), ink2)
})

test('transparência mapeia de opaco até o piso do controle e o efetivo respeita sistema e fundo', () => {
  assert.equal(glassAlpha(0), 1)
  assert.equal(glassAlpha(100), 0.55)
  assert.equal(transparencyFor(glassAlpha(50)), 50)
  assert.equal(effectiveGlass(0.6, 0.8, { moreContrast: false, reduceTransparency: false }), 0.8)
  assert.equal(effectiveGlass(0.9, 0.8, { moreContrast: false, reduceTransparency: false }), 0.9)
  assert.equal(effectiveGlass(0.6, 0.1, { moreContrast: true, reduceTransparency: false }), 0.92)
  assert.equal(effectiveGlass(0.6, 0.1, { moreContrast: false, reduceTransparency: true }), 1)
})

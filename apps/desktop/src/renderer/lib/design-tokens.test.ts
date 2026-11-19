import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { designTokensCSS, type DesignTokens } from './design-tokens.ts'

// D165: tokens.json is the one source; the docs' tokens.css and the app's styles/tokens.css are its
// compilation. A token edited in only one place fails here (regenerate: node tools/design-tokens.mjs).
const root = new URL('../../../../../', import.meta.url)
const source = JSON.parse(readFileSync(new URL('apps/desktop/design/tokens.json', root), 'utf8')) as DesignTokens
const css = designTokensCSS(source)

test('tokens.css do app e da documentação saem de tokens.json', () => {
  assert.equal(readFileSync(new URL('apps/desktop/src/renderer/styles/tokens.css', root), 'utf8'), css)
  assert.equal(readFileSync(new URL('apps/desktop/design/tokens.css', root), 'utf8'), css)
})

test('cada tema declara os mesmos tokens com valor por tema', () => {
  const block = (selector: string) => css.split(`${selector} {`)[1]!.split('}')[0]!
  const names = (b: string) => [...b.matchAll(/--([\w-]+):/g)].map((m) => m[1]!)
  const themed = source.color.tokens.filter((t) => typeof t.value !== 'string').map((t) => t.name)
  for (const name of themed) {
    assert.ok(names(block(':root, [data-theme="light"]')).includes(name), `${name} no claro`)
    assert.ok(names(block('[data-theme="dark"]')).includes(name), `${name} no escuro`)
  }
})

test('ink-rgb deriva canais opacos de cada tema sem outra paleta', () => {
  const withInk = (light: string, dark: string): DesignTokens => ({
    ...source,
    color: { tokens: source.color.tokens.map((t) => t.name === 'ink' ? { ...t, value: { light, dark } } : t) },
  })
  const generated = designTokensCSS(withInk('#123456', 'rgb(7, 8, 9)'))
  assert.match(generated, /--ink-rgb: 18, 52, 86;/)
  assert.match(generated.split('[data-theme="dark"] {')[1]!, /--ink-rgb: 7, 8, 9;/)
  assert.match(css, /--ink-rgb: 23, 22, 26;/)
  assert.match(css.split('[data-theme="dark"] {')[1]!, /--ink-rgb: 244, 243, 241;/)
  for (const invalid of ['rgba(1, 2, 3, 0.5)', 'rgb(256, 0, 0)', 'rgb(1.5, 2, 3)', 'var(--other)']) {
    assert.throws(() => designTokensCSS(withInk('#123456', invalid)))
  }
})

// Garantia do sistema.md: ink-3 claro ≥4,5:1 em canvas e surface sob o pico do halo do glow do header
// (0,08 do destaque sobre o fundo, no centro), com TODOS os destaques, e abaixo de ink-2 em luminância.
test('ink-3 claro mantém 4,5:1 sob o pico do glow com todos os destaques', () => {
  const GLOW_PICO = 0.08
  const light = (name: string) => {
    const v = source.color.tokens.find((t) => t.name === name)!.value as { light: string }
    return v.light
  }
  const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16))
  const lum = (c: number[]) => {
    const [r, g, b] = c.map((v) => { const s = v / 255; return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4 })
    return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!
  }
  const ratio = (a: number[], b: number[]) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x! + 0.05) / (y! + 0.05) }
  const ink3 = rgb(light('ink-3'))
  const accents = source.color.tokens.filter((t) => /^accent-(?!soft$)/.test(t.name))
  assert.equal(accents.length, 11)
  for (const a of accents) for (const ground of ['canvas', 'surface']) {
    const g = rgb(light(ground)), ac = rgb((a.value as { light: string }).light)
    const bg = g.map((v, i) => GLOW_PICO * ac[i]! + (1 - GLOW_PICO) * v)
    assert.ok(ratio(ink3, bg) >= 4.5, `${a.name} sobre ${ground}: ${ratio(ink3, bg).toFixed(3)}`)
  }
  assert.ok(lum(ink3) > lum(rgb(light('ink-2'))), 'ink-3 continua mais claro que ink-2')
})

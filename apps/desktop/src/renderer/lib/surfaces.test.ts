import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

// D165 (revisão A, F3/F4): menus têm chão opaco e botões preenchidos têm foco fora do próprio preenchimento.
const css = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8')
const block = (source: string, selector: string) => {
  const at = source.indexOf(`${selector} {`)
  assert.ok(at >= 0, `${selector} existe`)
  return source.slice(at, source.indexOf('}', at))
}

test('menus ancorados e a lista de comandos usam `raised`, não uma tinta translúcida', () => {
  assert.match(block(css('../components/overlay.css'), '.anchored-panel'), /background: var\(--raised\)/)
  assert.match(block(css('../views/thread/composer.css'), '.command-menu'), /background: var\(--raised\)/)
})

test('o anel de foco de botões preenchidos fica 2px para fora', () => {
  const theme = css('../styles/theme.css')
  const rule = theme.slice(theme.indexOf(':is(.styled-button'), theme.indexOf('}', theme.indexOf(':is(.styled-button')))
  for (const selector of ['.styled-button', '.dialog-button', '.composer-send', '.icon-button.selected']) assert.ok(rule.includes(selector), selector)
  assert.match(rule, /outline-offset: 2px/)
})

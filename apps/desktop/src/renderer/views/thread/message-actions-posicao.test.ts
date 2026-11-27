// A barra de ações da mensagem (reagir, responder, mais) fica abaixo do balão, alinhada com ele, e não ao lado.
// Fonte, não render: confere que a barra está no fluxo, depois do balão, e que a linha é sempre reservada.
import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const css = readFileSync(fileURLToPath(new URL('./thread.css', import.meta.url)), 'utf8')
const rows = readFileSync(fileURLToPath(new URL('./ChatRows.tsx', import.meta.url)), 'utf8')
const regra = (seletor: string) => css.match(new RegExp(`^${seletor.replace(/\./g, '\\.')} \\{([^}]*)\\}`, 'm'))?.[1] ?? ''

test('barra de ações: no fluxo, abaixo do balão, com altura reservada', () => {
  const base = regra('.message-actions')
  assert.doesNotMatch(base, /position:\s*absolute/)
  assert.match(base, /height:\s*26px/)
  assert.match(base, /opacity:\s*0/)
  assert.match(regra('.message-actions.leading'), /align-self:\s*flex-end/)
  assert.match(regra('.message-actions.trailing'), /align-self:\s*flex-start/)
  assert.doesNotMatch(css, /\.message-actions\.(leading|trailing) \{[^}]*calc\(100%/)
})

test('barra de ações: vem depois do balão no corpo da mensagem', () => {
  for (const lado of ['leading', 'trailing']) {
    const barra = rows.indexOf(`<MessageActions className="${lado}"`)
    assert.ok(barra > 0)
    assert.ok(rows.lastIndexOf('</Bubble>', barra) > rows.lastIndexOf('<div className="message-body">', barra))
  }
})

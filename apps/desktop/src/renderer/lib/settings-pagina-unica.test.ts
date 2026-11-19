import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

// Ajustes monta só a página ativa: o DOM não cresce com as páginas visitadas, e o scroll da página volta ao
// montar (ela sai do DOM ao trocar; a aba fica na AjustesTela). Lido do fonte, como sidebar-toggle.test.ts.
const fonte = readFileSync(new URL('../views/settings/SettingsView.tsx', import.meta.url), 'utf8')
// Só o código: os comentários explicam a decisão e podem citar o que foi tirado.
const source = fonte.split('\n').filter((linha) => !/^\s*\/\//.test(linha)).join('\n')

test('só a página ativa fica no DOM: sem cache de visitadas nem página escondida com display none', () => {
  assert.doesNotMatch(source, /visitadas/)
  assert.doesNotMatch(source, /abasLembradas/)
  assert.doesNotMatch(source, /display: 'none'/)
  assert.match(source, /<div key=\{page\} ref=\{paginaRef\} className="settings-page"/)
  assert.match(source, /<PageContent page=\{page\} aba=\{aba\} onPage=\{onPage\} \/>/)
})

test('a flag de ativa sai das páginas: cada uma só existe montada', () => {
  assert.doesNotMatch(source, /ativa=\{/)
  assert.match(source, /export function GeneralSettings\(\) \{/)
})

test('o scroll da página é guardado no pai ao rolar e restaurado ao montar', () => {
  assert.match(source, /onScroll=\{\(e\) => \{ rolagens\.current\[page\] = e\.currentTarget\.scrollTop \}\}/)
  assert.match(source, /useLayoutEffect\(\(\) => \{ if \(paginaRef\.current\) paginaRef\.current\.scrollTop = rolagens\.current\[page\] \?\? 0 \}, \[page\]\)/)
})

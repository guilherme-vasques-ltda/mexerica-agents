import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { leavesFloating, toggledSidebar } from '../views/shell/sidebar-toggle.ts'

// D165 revisão B: B1 (alternar em janela estreita) e B4 (foco volta ao botão de Rotinas).
test('em janela estreita o botão mostra e esconde a barra flutuante; larga, alterna a preferência', () => {
  const narrow = { narrow: true, collapsed: false }
  const opened = toggledSidebar({ ...narrow, peek: false })
  assert.deepEqual(opened, { peek: true, collapsed: false })
  assert.deepEqual(toggledSidebar({ ...narrow, peek: opened.peek }), { peek: false, collapsed: false })
  assert.deepEqual(toggledSidebar({ narrow: false, peek: true, collapsed: true }), { peek: false, collapsed: false })
  assert.deepEqual(toggledSidebar({ narrow: false, peek: false, collapsed: false }), { peek: false, collapsed: true })
})

// ajustes-tela-01 (revisão Sol, achado 2): com Rotinas ou Ajustes na frente, ⌃⌘S não deixa uma barra flutuante
// escondida atrás da tela (que tomaria o Esc); larga, a preferência continua alternando.
test('com uma tela na frente, o atalho não abre a barra flutuante', () => {
  assert.deepEqual(toggledSidebar({ narrow: true, peek: false, collapsed: false, covered: true }), { peek: false, collapsed: false })
  assert.deepEqual(toggledSidebar({ narrow: false, peek: false, collapsed: false, covered: true }), { peek: false, collapsed: true })
})

test('o botão de Rotinas da faixa atende o contrato de foco ao voltar (`.rc-open`)', () => {
  assert.match(readFileSync(new URL('../views/shell/SpaceRail.tsx', import.meta.url), 'utf8'), /className="space-rail-button rc-open"/)
})

test('a barra flutuante só sai sem camada aberta, sem ponteiro e sem foco nela (B5/B6)', () => {
  const base = { collapsed: true, layersOpen: false, pointerInside: false, focusInside: false }
  assert.equal(leavesFloating(base), true)
  assert.equal(leavesFloating({ ...base, layersOpen: true }), false, 'menu, paleta ou painel aberto')
  assert.equal(leavesFloating({ ...base, pointerInside: true }), false, 'ponteiro no aside físico')
  assert.equal(leavesFloating({ ...base, focusInside: true }), false, 'foco em Buscar bots')
  assert.equal(leavesFloating({ ...base, collapsed: false }), false, 'barra aberta não flutua')
})

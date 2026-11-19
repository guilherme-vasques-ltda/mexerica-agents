import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

// D165 fase G (RoutinesCalendar): a grade usa 44 px por hora no CSS e no cálculo das posições, e a
// tela de Rotinas esconde (não só torna inerte) o que fica atrás dela, para nunca haver texto sobre texto.
const read = (path: string) => readFileSync(new URL(path, import.meta.url), 'utf8')

test('a hora tem a mesma altura no CSS e no TimeGrid', () => {
  assert.match(read('../views/rotinas-central/TimeGrid.tsx'), /const HOUR = 44\b/)
  assert.match(read('../views/rotinas-central/routines-central.css'), /--rc-hour: 44px;/)
})

test('o que fica atrás de Rotinas some e as faixas (de ícones e do topo da janela) continuam', () => {
  const css = read('../views/rotinas-central/routines-central.css')
  assert.match(css, /\.rc-host\[data-coberto\] \.split > :not\(\.space-rail, \.janela-header, \.app-wallpaper, \.app-veil\) \{\s*visibility: hidden;/)
  assert.match(read('../views/rotinas-central/camada.tsx'), /\.split > :not\(\.space-rail, \.janela-header, \.app-wallpaper, \.app-veil\)/)
  assert.match(read('../views/rotinas-central/camada.tsx'), /data-coberto=\{covered \? '' : undefined\}/)
})

// Toda camada de tela cheia marca o próprio host com data-coberto e o CSS esconde o .split por baixo.
// Como as três envolvem o ChatSplitView, qualquer tela aberta — inclusive na troca direta de uma para
// outra, quando um data-coberto sai e o outro entra no mesmo commit — deixa o conteúdo escondido.
test('qualquer camada aberta esconde o que está atrás, sem janela visível', () => {
  const chat = read('../views/ChatWindow.tsx')
  assert.match(chat, /<WithRoutinesScreen><WithTimesScreen><WithTorreScreen><ChatSplitView \/>/)
  const camadas: Array<[string, string, string]> = [
    ['../views/rotinas-central/camada.tsx', 'rc-host', '../views/rotinas-central/routines-central.css'],
    ['../views/fork/times/camada.tsx', 'times-host', '../views/fork/times/times.css'],
    ['../views/fork/torre/camada.tsx', 'torre-host', '../views/fork/torre/torre.css'],
  ]
  for (const [camada, host, cssPath] of camadas) {
    const tsx = read(camada)
    assert.match(tsx, new RegExp(`className="${host}" data-coberto=\\{[^}]*\\? '' : undefined\\}`), camada)
    assert.match(tsx, /\.split > :not\(\.space-rail, \.janela-header, \.app-wallpaper, \.app-veil\)/, camada)
    assert.match(read(cssPath), new RegExp(`\\.${host}\\[data-coberto\\] \.split > :not\\(\\.space-rail, \.janela-header, \.app-wallpaper, \.app-veil\\) \\{\\s*visibility: hidden;`), cssPath)
  }
  // Ajustes e Extensões abrem dentro do host de Rotinas: o covered precisa cobri-los também.
  assert.match(read('../views/rotinas-central/camada.tsx'), /screen\.open \|\| settings\.open \|\| extensions\.open/)
})

import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import React from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { createServer } from 'vite'

test('D533: anéis simultâneos têm máscaras próprias e não duplicam o anúncio do medidor', async () => {
  const vite = await createServer({ configFile: false, root: fileURLToPath(new URL('../../../', import.meta.url)), server: { middlewareMode: true } })
  try {
    const { AnelContexto } = await vite.ssrLoadModule('/src/renderer/views/fork/compositor/AnelContexto.tsx')
    const html = renderToStaticMarkup(React.createElement(React.Fragment, null,
      React.createElement(AnelContexto, { percentual: 21 }), React.createElement(AnelContexto, { percentual: 74 })))
    const ids = [...html.matchAll(/<mask id="([^"]+)"/g)].map(m => m[1]!)
    assert.equal(ids.length, 2)
    assert.equal(new Set(ids).size, 2, 'principal e sessão não compartilham a máscara de outra instância')
    for (const id of ids) assert.ok(html.includes(`mask="url(#${id})"`))
    assert.equal((html.match(/aria-hidden="true"/g) ?? []).length, 2)
    assert.equal((html.match(/focusable="false"/g) ?? []).length, 2)
    assert.doesNotMatch(html, /tabindex|role="progressbar"|<title/, 'o botão existente já anuncia o percentual e recebe o foco')
  } finally { await vite.close() }
})

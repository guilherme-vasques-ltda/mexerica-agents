// MX-214 (rodada 5): o cartão do aviso mostra o motivo do envio barrado como aviso (não erro), no próprio cartão.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import React from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { createServer } from 'vite'

const g = globalThis as Record<string, unknown>
g.window ??= {
  addEventListener: () => {}, removeEventListener: () => {},
  matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }),
}
g.localStorage ??= { getItem: () => null, setItem: () => {}, removeItem: () => {} }

test('com envio barrado o cartão mostra "Feche o aviso…" como status; sem envio barrado, não', async () => {
  const raiz = fileURLToPath(new URL('../../../../../', import.meta.url))
  const vite = await createServer({ configFile: false, root: raiz, resolve: { alias: { '@shared': `${raiz}src/shared` } }, server: { middlewareMode: true },
    plugins: [{ name: 'ssr-snapshot', transform: (code: string, id: string) => (id.includes('/src/renderer/') ? code.replace(/useSyncExternalStore\(/g, '((_s, get) => get())(') : null) }] })
  try {
    const { AvisoSenhaColada } = await vite.ssrLoadModule('/src/renderer/views/fork/compositor/AvisoSenhaColada.tsx')
    const render = (envioBarrado: boolean, recusada = false) => renderToStaticMarkup(React.createElement(AvisoSenhaColada, { nomeBot: 'Berga', recusada, envioBarrado, mover() {}, naoESenha() {} }))
    const sem = render(false)
    assert.doesNotMatch(sem, /Close the password notice|Feche o aviso/)
    const com = render(true)
    assert.match(com, /Close the password notice before sending\./)
    assert.match(com, /role="status"/, 'é um status, não um erro')
    // O cartão em si continua sendo o único alerta; nenhum diálogo de erro é montado aqui.
    assert.equal((com.match(/role="alert"/g) ?? []).length, 1)
    assert.match(render(true, true), /paste wasn’t inserted/, 'a linha da colagem recusada convive com a do envio barrado')
  } finally { await vite.close() }
})

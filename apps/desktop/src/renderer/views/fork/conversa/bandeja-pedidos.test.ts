// MX-77: a bandeja REAL (BandejaFila + useFila) mostra o pedido pendente de outro bot e o tira quando é respondido.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import React from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { createServer } from 'vite'
import type { Entry } from '@shared/models'

// Sem DOM: as preferências da janela (lib/prefs.ts) só precisam de `window` com ouvintes e um armazenamento vazio.
const g = globalThis as Record<string, unknown>
g.window ??= { addEventListener: () => {}, removeEventListener: () => {} }
g.localStorage ??= { getItem: () => null, setItem: () => {}, removeItem: () => {} }

const base = { rev: 1, turn: 1, createdAt: 0, updatedAt: 0 }
const dono = (): Entry => ({ ...base, id: 'u1', seq: 1, botId: 'donna', kind: 'user', data: { text: 'mensagem do dono', status: 'queued' } })
const pedido = (status: string): Entry => ({
  ...base, id: 'p1', seq: 2, botId: 'donna', kind: 'notice',
  data: {
    status, delegationId: 'del-1', sourceBotId: 'byte', targetBotId: 'donna', sourceChat: 'byte', sourceSession: null,
    botMessage: { sourceBotId: 'byte', targetBotId: 'donna', text: 'revisa o diff do pace.js' },
  },
})

test('um pedido pendente de outro bot aparece na bandeja "Na fila" e some quando é respondido', async () => {
  const raiz = fileURLToPath(new URL('../../../../../', import.meta.url))
  const vite = await createServer({ configFile: false, root: raiz, resolve: { alias: { '@shared': `${raiz}src/shared` } }, server: { middlewareMode: true },
    // O SSR do React exige `getServerSnapshot` no useSyncExternalStore dos modelos; no teste basta ler o valor atual.
    plugins: [{ name: 'ssr-snapshot', transform: (code: string, id: string) => (id.includes('/src/renderer/') ? code.replace(/useSyncExternalStore\(/g, '((_s, get) => get())(') : null) }] })
  try {
    const { BandejaFila, useFila } = await vite.ssrLoadModule('/src/renderer/views/fork/conversa/BandejaFila.tsx')
    const { StoreContext } = await vite.ssrLoadModule('/src/renderer/store/context.ts')
    let entries: Entry[] = []
    const store = {
      subscribe: () => () => {}, getVersion: () => 0,
      allEntries: () => entries, bots: new Map([['byte', { name: 'Byte' }]]),
      sessions: { of: () => null }, conversations: new Map(),
      queuedAction: async () => null, setError: () => {},
    }
    const Bandeja = () => React.createElement(BandejaFila, { fila: useFila('donna', null) })
    const render = () => renderToStaticMarkup(React.createElement(StoreContext.Provider, { value: store as never }, React.createElement(Bandeja)))

    entries = [dono(), pedido('queued')]
    const cheia = render()
    assert.match(cheia, /revisa o diff do pace\.js/, 'o texto do pedido está na bandeja')
    assert.match(cheia, /De Byte|From Byte/, 'a linha diz de quem veio')
    assert.match(cheia, /mensagem do dono/, 'a mensagem do dono continua ali')
    assert.match(cheia, /fila-conta">· 2</, 'dois itens na fila')
    assert.equal((cheia.match(/fila-alca"/g) ?? []).length, 1, 'só a mensagem do dono tem alça de arraste')

    entries = [dono(), pedido('sent')]
    const respondida = render()
    assert.doesNotMatch(respondida, /revisa o diff do pace\.js/, 'respondido, o pedido sai da bandeja')
    assert.match(respondida, /fila-conta">· 1</)

    entries = [pedido('queued')]
    assert.match(render(), /fila-conta">· 1</, 'sozinho, o pedido já abre a bandeja')
    entries = [pedido('sent')]
    assert.equal(render(), '', 'sem pendência, sem bandeja')
  } finally { await vite.close() }
})

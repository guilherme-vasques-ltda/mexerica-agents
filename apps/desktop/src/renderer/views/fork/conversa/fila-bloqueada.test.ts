// MX-214 (P2): com o aviso de senha colada aberto, a fila REAL (useFila + BandejaFila) não envia nada: nem o atalho
// Shift+Enter, nem "Mandar agora", nem salvar a edição. Sem o bloqueio, as mesmas ações chegam ao Hub (controle).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import React from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { createServer } from 'vite'
import type { Entry } from '@shared/models'

const g = globalThis as Record<string, unknown>
g.window ??= { addEventListener: () => {}, removeEventListener: () => {} }
g.localStorage ??= { getItem: () => null, setItem: () => {}, removeItem: () => {} }

const base = { rev: 1, turn: 1, createdAt: 0, updatedAt: 0 }
const dono: Entry = { ...base, id: 'u1', seq: 1, botId: 'donna', kind: 'user', data: { text: 'mensagem na fila', status: 'queued' } }
const shiftEnter = { key: 'Enter', shiftKey: true, altKey: false, metaKey: false, ctrlKey: false, isComposing: false }

test('aviso aberto: atalho da fila, Mandar agora e salvar edição não chegam ao Hub; sem aviso, chegam', async () => {
  const raiz = fileURLToPath(new URL('../../../../../', import.meta.url))
  const vite = await createServer({ configFile: false, root: raiz, resolve: { alias: { '@shared': `${raiz}src/shared` } }, server: { middlewareMode: true },
    plugins: [{ name: 'ssr-snapshot', transform: (code: string, id: string) => (id.includes('/src/renderer/') ? code.replace(/useSyncExternalStore\(/g, '((_s, get) => get())(') : null) }] })
  try {
    const { BandejaFila, useFila } = await vite.ssrLoadModule('/src/renderer/views/fork/conversa/BandejaFila.tsx')
    const { StoreContext } = await vite.ssrLoadModule('/src/renderer/store/context.ts')
    const chamadas: string[] = []
    const store = {
      subscribe: () => () => {}, getVersion: () => 0,
      allEntries: () => [dono], bots: new Map(), sessions: { of: () => null }, conversations: new Map(),
      queuedAction: async (method: string) => { chamadas.push(method); return null }, setError: () => {},
    }
    const capturar = (bloqueada: boolean, editando = false) => {
      let fila: ReturnType<typeof useFila> | undefined
      // Edição ATIVA de verdade: a fila já abre editando a mensagem (texto novo) como depois de "editar".
      const Probe = () => { fila = useFila('donna', null, bloqueada, editando ? { entry: dono, posicao: 0, texto: 'texto editado' } : null); return React.createElement(BandejaFila, { fila }) }
      const html = renderToStaticMarkup(React.createElement(StoreContext.Provider, { value: store as never }, React.createElement(Probe)))
      return { fila: fila!, html }
    }

    const livre = capturar(false)
    assert.equal(livre.fila.tecla(shiftEnter, true), true, 'controle: o atalho age sem aviso')
    livre.fila.mandarAgora(dono)
    assert.deepEqual(chamadas, ['queuedSendNow', 'queuedSendNow'], 'controle: os dois envios chegaram ao Hub')
    assert.doesNotMatch(livre.html, /fila-agora"[^>]*disabled/, 'sem aviso o botão está ligado')

    chamadas.length = 0
    const preso = capturar(true)
    assert.equal(preso.fila.tecla(shiftEnter, true), false, 'o atalho Shift+Enter não age')
    preso.fila.mandarAgora(dono)
    assert.deepEqual(chamadas, [], 'nada foi ao Hub')
    assert.match(preso.html, /fila-agora"[^>]*disabled|disabled=""[^>]*fila-agora/, 'o botão Mandar agora está desligado')

    // Salvar a edição: sem aviso chega ao Hub (controle: prova que a edição está ativa); com aviso, não.
    const editandoLivre = capturar(false, true)
    assert.equal(editandoLivre.fila.edicao?.texto, 'texto editado', 'a edição está ativa')
    await editandoLivre.fila.salvar()
    assert.deepEqual(chamadas, ['queuedEdit'], 'controle: a edição ativa é salva no Hub')
    chamadas.length = 0
    const editandoPreso = capturar(true, true)
    assert.equal(editandoPreso.fila.edicao?.texto, 'texto editado', 'a edição continua ativa com o aviso aberto')
    await editandoPreso.fila.salvar()
    assert.deepEqual(chamadas, [], 'com o aviso aberto, salvar a edição não chega ao Hub')
  } finally { await vite.close() }
})

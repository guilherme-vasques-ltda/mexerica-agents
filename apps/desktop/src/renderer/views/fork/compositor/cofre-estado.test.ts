// MX-277: o cartão do aviso depois de "Mover para o cofre" acompanha a janela segura, só com estado; e o gancho leva
// SÓ o id do computador (nunca o trecho escondido).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { fileURLToPath } from 'node:url'
import React from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { createServer } from 'vite'
import { faseAoAviso, faseAoResponder } from './cofre-estado.ts'
import { abrirCofreParaLoginNovo } from './gancho-cofre.ts'

const g = globalThis as Record<string, unknown>
g.window ??= {
  addEventListener: () => {}, removeEventListener: () => {},
  matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }),
}
g.localStorage ??= { getItem: () => null, setItem: () => {}, removeItem: () => {} }

test('faseAoResponder: aceita vira "aberta"; cada recusa do main vira o próprio motivo', () => {
  assert.equal(faseAoResponder({ ok: true, foco: false }), 'aberta')
  assert.equal(faseAoResponder({ ok: true, foco: true }), 'aberta')
  assert.equal(faseAoResponder({ ok: false, motivo: 'semSuporte' }), 'semSuporte')
  assert.equal(faseAoResponder({ ok: false, motivo: 'semConexao' }), 'semConexao')
  assert.equal(faseAoResponder({ ok: false, motivo: 'invalido' }), 'invalido')
})

test('faseAoAviso: salva = "guardado" (definitivo); fechar sem guardar tira o cartão; avisos de outros pedidos são ignorados', () => {
  assert.equal(faseAoAviso('abrindo', { entryId: 'login-novo', estado: 'aberta' }), 'aberta')
  assert.equal(faseAoAviso('aberta', { entryId: 'login-novo', estado: 'salva' }), 'guardado')
  assert.equal(faseAoAviso('guardado', { entryId: 'login-novo', estado: 'fechada' }), 'guardado', 'depois de guardar, fechar não apaga')
  assert.equal(faseAoAviso('guardado', { entryId: 'login-novo', estado: 'aberta' }), 'guardado')
  assert.equal(faseAoAviso('aberta', { entryId: 'login-novo', estado: 'fechada' }), null, 'cancelar: nada guardado, o cartão some')
  for (const estado of ['aberta', 'fechada', 'salva'] as const) {
    assert.equal(faseAoAviso('aberta', { entryId: 'e-do-bot', estado }), 'aberta', 'um pedido do bot não mexe neste cartão')
  }
})

test('o gancho leva SÓ o id do computador ao IPC; um erro do IPC vira "sem conexão" sem derrubar o chat', async () => {
  const chamadas: unknown[] = []
  ;(g.window as Record<string, unknown>).codync = { forkCofre: { abrirNovo: async (pedido: unknown) => { chamadas.push(pedido); return { ok: true, foco: false } } } }
  const r = await abrirCofreParaLoginNovo('pc-1')
  assert.deepEqual(r, { ok: true, foco: false })
  assert.deepEqual(chamadas, [{ computerId: 'pc-1' }], 'nenhum outro campo (valor, texto, bot) segue ao main')
  assert.equal(abrirCofreParaLoginNovo.length, 1, 'a assinatura só recebe o computador')
  ;(g.window as Record<string, unknown>).codync = { forkCofre: { abrirNovo: async () => { throw new Error('ipc caiu') } } }
  assert.deepEqual(await abrirCofreParaLoginNovo('pc-1'), { ok: false, motivo: 'semConexao' })
})

test('o cartão mostra "Guardado no cofre" (verde, sem valor), a janela aberta e a falha; só falha/guardado têm Fechar', async () => {
  const raiz = fileURLToPath(new URL('../../../../../', import.meta.url))
  const vite = await createServer({ configFile: false, root: raiz, resolve: { alias: { '@shared': `${raiz}src/shared` } }, server: { middlewareMode: true },
    plugins: [{ name: 'ssr-snapshot', transform: (code: string, id: string) => (id.includes('/src/renderer/') ? code.replace(/useSyncExternalStore\(/g, '((_s, get) => get())(') : null) }] })
  try {
    const { AvisoCofre } = await vite.ssrLoadModule('/src/renderer/views/fork/compositor/AvisoCofre.tsx')
    const html = (fase: string) => renderToStaticMarkup(React.createElement(AvisoCofre, { fase, fechar() {} }))
    const guardado = html('guardado')
    assert.match(guardado, /Saved in the vault|vault/i)
    assert.match(guardado, /is-guardado/)
    assert.match(guardado, /role="status"/)
    assert.match(guardado, /aria-label="Close"|Close/, 'dá para fechar o cartão')
    const aberta = html('aberta')
    assert.match(aberta, /Secure window open/)
    assert.equal(/Close/.test(aberta), false, 'enquanto a janela está aberta não há Fechar')
    assert.equal(/is-guardado/.test(aberta), false)
    assert.match(html('abrindo'), /Secure window open/)
    for (const falha of ['semSuporte', 'semConexao', 'invalido']) {
      const h = html(falha)
      assert.match(h, /Couldn’t open the secure window/)
      assert.match(h, /Close/)
    }
    assert.match(html('semSuporte'), /connection|conexão/i, 'relay: explica que não dá por esta conexão')
    for (const fase of ['guardado', 'aberta', 'semSuporte']) assert.equal(/<input|<textarea|type="password"/i.test(html(fase)), false, 'o chat nunca tem campo de valor')
  } finally { await vite.close() }
})

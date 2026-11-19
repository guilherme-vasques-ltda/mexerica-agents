import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { registerHooks } from 'node:module'
import test from 'node:test'
registerHooks({ resolve(specifier, context, next) {
  if (specifier.startsWith('.') && context.parentURL && !/\.[a-z]+$/.test(specifier)) {
    const url = new URL(`${specifier}.ts`, context.parentURL)
    if (existsSync(url)) return next(url.href, context)
  }
  return next(specifier, context)
} })

// MX-1265: na web o Sair esquece o vínculo do navegador; o aviso vem ANTES de qualquer saída.
function conta(web: boolean) {
  const chamadas = { signOut: 0 }
  const account = { signOut: async () => { chamadas.signOut++ }, reconciliationState: async () => ({ kind: 'none' }), ...(web ? { proximoAcessoExigePareamento: async () => false } : {}) }
  Object.assign(globalThis, { window: { codync: { account } } })
  return chamadas
}

test('web: pedir Sair só abre o aviso; sair exige o gesto de confirmar', async () => {
  const chamadas = conta(true)
  const { AccountSession } = await import('../../../store/fork-account-session.ts')
  const s = new AccountSession(); s.isReady = true
  await s.requestSignOut()
  assert.equal(s.confirmingSignOut, true); assert.equal(chamadas.signOut, 0, 'nada sai (nem apaga) antes de confirmar')
  s.cancelSignOut()
  assert.equal(s.confirmingSignOut, false); assert.equal(chamadas.signOut, 0, 'Cancelar não sai')
  await s.requestSignOut(); await s.requestSignOut()
  assert.equal(s.confirmingSignOut, true, 'pedido repetido não empilha nem sai')
  s.cancelSignOut(); await s.signOut() // o botão do aviso chama signOut depois de fechar o aviso
  assert.equal(chamadas.signOut, 1)
})

test('web: troca de sessão fecha o aviso aberto', async () => {
  conta(true)
  const { AccountSession } = await import('../../../store/fork-account-session.ts')
  const s = new AccountSession(); s.isReady = true
  await s.requestSignOut(); assert.equal(s.confirmingSignOut, true)
  ;(s as unknown as { apply(x: object): void }).apply({ ready: true, configured: true, busy: false, error: null, user: null, cloudURL: null })
  assert.equal(s.confirmingSignOut, false)
})

test('desktop (sem a ponte da web): Sair continua direto, sem aviso', async () => {
  const chamadas = conta(false)
  const { AccountSession } = await import('../../../store/fork-account-session.ts')
  const s = new AccountSession(); s.isReady = true
  await s.requestSignOut()
  assert.equal(s.confirmingSignOut, false); assert.equal(chamadas.signOut, 1)
})

test('o aviso existe nos seis idiomas, em linguagem de leigo e sem fornecedor', async () => {
  const { traduzir: tipada } = await import('../../../lib/fork-i18n/index.ts')
  const traduzir = tipada as (idioma: string, chave: string) => string // chaves em laço: sem parâmetros
  const chaves = ['titulo', 'oque', 'proxima', 'fica', 'confirmar'].map(k => `conta.web.sair.${k}`)
  for (const idioma of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) {
    for (const chave of chaves) {
      const texto = traduzir(idioma, chave)
      assert.ok(texto.length > 8 && texto !== chave && texto !== 'Aviso pendente', `${idioma} ${chave}`)
      assert.doesNotMatch(texto, /clerk|cloudflare|indexeddb|seed|semente|chave|agente|agent\b/i, `${idioma} ${chave}: jargão ou fornecedor`)
      if (idioma !== 'pt-BR') assert.notEqual(texto, traduzir('pt-BR', chave), `${idioma} ${chave}: sem tradução`)
    }
  }
  assert.equal(traduzir('pt-BR', 'conta.web.sair.confirmar'), 'Sair e esquecer este navegador')
  assert.match(traduzir('pt-BR', 'conta.web.sair.proxima'), /parear de novo/)
  assert.match(traduzir('pt-BR', 'conta.web.sair.fica'), /Hub/)
})

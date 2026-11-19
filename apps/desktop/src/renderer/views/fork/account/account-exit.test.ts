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
test('D185: transitório oferece Tentar de novo/Entrar novamente, sem promessa de revogação', async () => {
  Object.assign(globalThis, { window: { codync: { platform: 'darwin' } } })
  const { exitActions } = await import('../../../store/fork-account-session.ts')
  const { exitCopy, exitLabel, aparelhoDe } = await import('./exit-copy.ts')
  assert.deepEqual(exitActions('session_unverified', false), ['check', 'signOut'])
  assert.deepEqual(exitActions('none', false), [])
  const context = { kind: 'session_unverified' as const, busy: false, aparelho: aparelhoDe('darwin'), idioma: 'pt-BR' as const }
  assert.equal(exitLabel('check', context), 'Tentar de novo'); assert.equal(exitLabel('signOut', context), 'Entrar novamente')
  const copy = exitCopy('session_unverified', { ...context, hasSession: false, outcome: null })
  assert.equal(copy.title, 'Não foi possível verificar sua sessão agora')
  assert.match(copy.hint, /segundo plano/)
  // DON-917 (fatia 9b): a frase inteira vem do catálogo pelo tipo de aparelho, no idioma pedido.
  assert.match(copy.body, /deste Mac continua salvo/)
  assert.match(exitCopy('session_unverified', { ...context, aparelho: aparelhoDe('linux'), hasSession: false, outcome: null }).body, /deste computador continua salvo/)
  assert.match(exitCopy('session_unverified', { ...context, idioma: 'en', hasSession: false, outcome: null }).body, /your sign-in on this Mac is still saved/)
  assert.equal(exitLabel('check', { ...context, idioma: 'en' }), 'Try again')
})
test('D185: Sair durante retry é enviado e ignora resposta antiga', async () => {
  let release!: (value: { kind: 'none' }) => void; let exits = 0
  Object.assign(globalThis, { window: { codync: { account: { reconcile: () => new Promise(resolve => { release = resolve }), signOut: async () => { exits++ }, reconciliationState: async () => ({ kind: 'none' }) } } } })
  const { AccountSession } = await import('../../../store/fork-account-session.ts')
  const s = new AccountSession(); s.isReady = true; s.exit = 'session_unverified'
  const retry = s.checkExit(); await s.signOut(); release({ kind: 'none' }); await retry
  assert.equal(exits, 1); assert.equal(s.exitAction, null); assert.equal(s.exit, 'none')
})

test('cofre distingue indisponibilidade, chave e geração nos seis idiomas', async () => {
  const { exitCopy } = await import('./exit-copy.ts')
  const { traduzir } = await import('../../../lib/fork-i18n/index.ts')
  for (const idioma of ['pt-BR', 'en', 'es', 'fr', 'zh-CN', 'ar-AE'] as const) {
    const bodies = new Set<string>()
    for (const reason of ['unavailable', 'mismatch', 'unsupported'] as const) {
      const copy = exitCopy('session_unverified', { aparelho: 'mac', idioma, hasSession: false, outcome: null, error: `keychain.${reason}` })
      assert.equal(copy.title, traduzir(idioma, 'sistema.barra.cofre.titulo'))
      assert.equal(copy.body, traduzir(idioma, `sistema.barra.cofre.${reason}`))
      bodies.add(copy.body)
    }
    assert.equal(bodies.size, 3)
  }
})

import assert from 'node:assert/strict'
import test from 'node:test'
import { parseNavigationURL } from './navigation-policy.ts'
import { loadMainModule } from './test-support/main-module.ts'

function fixture() {
  const opened: string[] = []
  const callbacks: (() => void)[] = []
  const cleared: ReturnType<typeof setTimeout>[] = []
  const auth = loadMainModule<typeof import('../main/auth.ts')>('auth', {
    electron: {},
    './environment': { identity: { scheme: 'donnabot', appId: 'app.donnabot.mac' } },
    './navigation': { openExternalURL: async (url: string) => { opened.push(url); return true } },
    '../shared/navigation-policy': { parseNavigationURL },
  }, {
    URL,
    setTimeout: (callback: () => void, ms: number) => {
      callbacks.push(callback)
      return setTimeout(callback, ms)
    },
    clearTimeout: (timer: ReturnType<typeof setTimeout>) => { cleared.push(timer); clearTimeout(timer) },
  })
  return { ...auth, opened, callbacks, cleared }
}

test('substituir o login cancela seu timer e mantém o callback seguinte após o prazo antigo', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const auth = fixture()
  const first = auth.authenticate('https://accounts.example.test/oauth', 'app.donnabot.mac')
  const cancelled = assert.rejects(first, (error: unknown) =>
    error instanceof auth.AuthenticationEnded && error.reason === 'cancelled')
  t.mock.timers.tick(9.5 * 60_000)

  const second = auth.authenticate('https://connector.example.test/authorize', 'donnabot')
  await cancelled
  assert.equal(auth.cleared.length, 1, 'o timer substituído foi cancelado')
  t.mock.timers.tick(60_000) // 10:30: depois do timeout original de A.
  const callback = 'donnabot://oauth?code=example&state=example'
  auth.handleURL(callback)
  assert.equal(await second, callback)
  assert.equal(auth.cleared.length, 2, 'o callback também cancelou o timer atual')
  assert.deepEqual(auth.opened, ['https://accounts.example.test/oauth', 'https://connector.example.test/authorize'])
})

test('um timeout antigo já enfileirado não apaga o pedido atual', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const auth = fixture()
  const first = auth.authenticate('https://accounts.example.test/oauth', 'app.donnabot.mac')
  const cancelled = assert.rejects(first, (error: unknown) =>
    error instanceof auth.AuthenticationEnded && error.reason === 'cancelled')
  const second = auth.authenticate('https://connector.example.test/authorize', 'donnabot')
  await cancelled

  auth.callbacks[0]!()
  const callback = 'donnabot://oauth?code=example&state=example'
  auth.handleURL(callback)
  assert.equal(await second, callback)
})


test('retorno antigo ou destino diferente não conclui tentativa nova', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] })
  const auth = fixture()
  const callback = 'app.donnabot.mac://callback?attempt=new'
  const result = auth.authenticate('https://identity.example.test/oauth', 'app.donnabot.mac', callback)
  auth.handleURL('app.donnabot.mac://callback?attempt=old')
  auth.handleURL('app.donnabot.mac://other?attempt=new')
  assert.equal(auth.cleared.length, 0)
  const valid = `${callback}&rotating_token_nonce=fixture`
  auth.handleURL(valid)
  assert.equal(await result, valid)
})

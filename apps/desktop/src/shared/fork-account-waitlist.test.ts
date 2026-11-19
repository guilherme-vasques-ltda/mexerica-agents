import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import * as crypto from 'node:crypto'
import * as fs from 'node:fs'
import * as path from 'node:path'
import { tmpdir } from 'node:os'
import test from 'node:test'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { signUpMode, waitlistCopy, waitlistRefusal, WAITLIST_URL } from '../main/fork-account/entry-rules.ts'
import { newAccountOffer } from './fork-account-providers.ts'
import { loadMainModule, settle } from './test-support/main-module.ts'

// E7c (D70): cadastro do Clerk em lista de espera. Fail-closed lendo o ambiente; a lista abre só a URL
// fixa do Main; Google sem convite e e-mail desconhecido recebem a orientação, sem sessão.
const host = 'identity.example.test'
const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: `https://${host}`, cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
const env = (mode: unknown) => ({ user_settings: { sign_up: { mode, legal_consent_enabled: false }, social: { oauth_google: { enabled: true, authenticatable: true } }, attributes: { email_address: { enabled: true, required: true, used_for_first_factor: true, first_factors: ['email_code'], verifications: ['email_code'] }, password: { enabled: true, required: true } }, password_settings: { min_length: 8 } } })
class AuthenticationEnded extends Error {}

function world(environment: () => unknown, fapi: (url: URL, body: URLSearchParams, method: string) => { status: number; value: unknown; auth?: string } | null = () => null) {
  const dir = fs.mkdtempSync(path.join(tmpdir(), 'account-waitlist-'))
  const calls: string[] = []; const opened: string[] = []; const handlers = new Map<string, (...args: unknown[]) => unknown>()
  const reply = (status: number, value: unknown, auth?: string) => ({ ok: status < 300, status, headers: new Headers(auth ? { authorization: auth } : {}), json: async () => value })
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': crypto, 'node:fs': fs, 'node:path': path,
    electron: {
      app: { getPath: () => dir, getVersion: () => 'fixture', isPackaged: true },
      ipcMain: { handle: (name: string, fn: (...args: unknown[]) => unknown) => handlers.set(name, fn) },
      shell: { openExternal: async (url: string) => { opened.push(url) } },
      safeStorage: { isEncryptionAvailable: () => true, encryptString: (s: string) => Buffer.from(s), decryptString: (b: Buffer) => b.toString() },
    },
    './auth': { AuthenticationEnded, authenticate: async (_redirect: string, _scheme: string, callback: string) => callback },
    './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
  }, { URL, URLSearchParams, Buffer, Headers, __dirname: '/fixture/main', process: { platform: 'darwin', env: {} }, setTimeout: () => ({}), console: { warn: () => {} }, fetch: async (url: URL, init: { method: string; body?: string }) => {
    if (url.pathname === '/v1/environment') { const body = environment(); if (body instanceof Error) throw body; return reply(200, { response: body }) }
    calls.push(`${init.method} ${url.pathname}`)
    const answer = fapi(url, new URLSearchParams(init.body), init.method)
    if (answer) return reply(answer.status, answer.value, answer.auth)
    if (url.pathname === '/v1/client') return reply(200, { response: { sessions: [], last_active_session_id: null } })
    return reply(200, { response: {} })
  } })
  const account = new mod.AccountService()
  return { account, calls, opened, handlers, dir }
}
const restore = (s: unknown) => (s as { restore(): Promise<void> }).restore()

test('modo do cadastro: waitlist → lista de espera e não Criar conta; public → Criar conta; ilegível → nenhum', async () => {
  assert.equal(signUpMode(env('waitlist')), 'waitlist')
  assert.equal(signUpMode(env('public')), 'public')
  assert.equal(signUpMode(env('restricted')), null)
  assert.equal(signUpMode({}), null)
  assert.equal(signUpMode(null), null)

  for (const [body, mode, offer] of [[env('waitlist'), 'waitlist', 'waitlist'], [env('public'), 'public', 'signup'], [new Error('offline'), null, null], [env('restricted'), null, null]] as const) {
    const w = world(() => body)
    try {
      await restore(w.account); await w.account.loadSocialProviders()
      assert.equal(w.account.state.signUpMode, mode)
      assert.equal(newAccountOffer(w.account.state.signUpMode, w.account.state.signUp, w.account.state.waitlist), offer)
      if (mode === 'waitlist') assert.equal(w.account.state.signUp, null, 'sem Criar conta com a lista de espera')
    } finally { fs.rmSync(w.dir, { recursive: true, force: true }) }
  }
  assert.equal(newAccountOffer('public', null), null, 'público sem regras do Main → nada')
  assert.equal(newAccountOffer(null, null, true), 'waitlist', 'recusa do Clerk mostra o link')
})

test('lista de espera abre só a URL fixa do Main, e só pela janela da conta', async () => {
  const w = world(() => env('waitlist'))
  try {
    const frame = { url: 'file:///fixture/renderer/index.html?window=chat' }
    const wc = { mainFrame: frame, isDestroyed: () => false, send: () => {} }
    w.account.register(() => [{ isDestroyed: () => false, webContents: wc }] as never)
    await settle()
    const open = w.handlers.get('account:openWaitlist')!
    assert.equal(await open({ sender: wc, senderFrame: frame }, 'https://evil.example/'), true)
    assert.deepEqual(w.opened, [WAITLIST_URL])
    assert.equal(new URL(WAITLIST_URL).origin, 'https://accounts.example.invalid')
    assert.equal(await open({ sender: { ...wc, mainFrame: {} }, senderFrame: { url: 'https://evil.example/' } }), false)
    assert.deepEqual(w.opened, [WAITLIST_URL], 'outra origem não abre nada')
  } finally { fs.rmSync(w.dir, { recursive: true, force: true }) }
})

test('Google sem convite: transfer recusado por lista de espera → mensagem + link, sem sessão e sem nova tentativa', async () => {
  assert.equal(waitlistRefusal('sign_up_restricted_waitlist'), true)
  assert.equal(waitlistRefusal('sign_up_mode_restricted'), true)
  assert.equal(waitlistRefusal('form_identifier_exists'), false)
  const w = world(() => env('waitlist'), (url, _body, method) => {
    if (url.pathname === '/v1/client/sign_ins' && method === 'POST') return { status: 200, value: { response: { id: 'sia_1', status: 'needs_first_factor' } }, auth: 'fixture-client-1' }
    if (url.pathname === '/v1/client/sign_ins/sia_1/prepare_first_factor') return { status: 200, value: { response: { id: 'sia_1', first_factor_verification: { external_verification_redirect_url: `https://${host}/oauth` } } } }
    if (url.pathname === '/v1/client/sign_ins/sia_1') return { status: 200, value: { response: { id: 'sia_1', status: 'needs_identifier', first_factor_verification: { status: 'transferable', error: { code: 'external_account_not_found' } } } } }
    if (url.pathname === '/v1/client/sign_ups' && method === 'POST') return { status: 403, value: { errors: [{ code: 'sign_up_restricted_waitlist', message: 'Sign-ups are currently unavailable.' }] } }
    return null
  })
  try {
    await restore(w.account); await w.account.loadSocialProviders()
    assert.equal(await w.account.signIn('google'), 'verification')
    await settle()
    assert.equal(w.account.state.error, waitlistCopy)
    assert.equal(w.account.state.waitlist, true)
    assert.equal(w.account.state.user, null)
    assert.equal(w.account.captureCloudSession(), null)
    assert.equal(w.account.reconciliationState().kind, 'none', 'volta aos métodos, sem recuperação')
    assert.equal(w.calls.filter(c => c === 'POST /v1/client/sign_ups').length, 1, 'não repete sozinho')
    assert.equal(w.calls.filter(c => c === 'POST /v1/client/sign_ins').length, 1, 'nenhuma tentativa nova')
    assert.equal(newAccountOffer(w.account.state.signUpMode, w.account.state.signUp, w.account.state.waitlist), 'waitlist')
  } finally { fs.rmSync(w.dir, { recursive: true, force: true }) }
})

test('código por e-mail e esqueci a senha com e-mail desconhecido → mesma orientação, sem sessão', async () => {
  for (const mode of ['waitlist', 'public'] as const) {
    const w = world(() => env(mode), (url, _body, method) => url.pathname === '/v1/client/sign_ins' && method === 'POST' ? { status: 422, value: { errors: [{ code: 'form_identifier_not_found' }] } } : null)
    try {
      await restore(w.account); await w.account.loadSocialProviders()
      w.account.state = { ...w.account.state, emailCode: true, passwordReset: { minPasswordLength: 8 } }
      for (const run of [() => w.account.signInWithEmailCode('nobody@example.test'), () => w.account.forgotPassword('nobody@example.test')]) {
        assert.equal(await run(), 'error')
        await settle()
        assert.equal(w.account.state.error, mode === 'waitlist' ? waitlistCopy : 'Não foi possível enviar o código. Confira o e-mail ou crie uma conta.')
        assert.equal(w.account.state.waitlist, mode === 'waitlist')
        assert.equal(w.account.state.user, null); assert.equal(w.account.state.pending, null)
        assert.equal(JSON.stringify(w.account.state).includes('nobody@example.test'), false)
      }
    } finally { fs.rmSync(w.dir, { recursive: true, force: true }) }
  }
})

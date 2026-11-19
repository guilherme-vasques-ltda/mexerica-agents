import assert from 'node:assert/strict'
import { EventEmitter } from 'node:events'
import * as crypto from 'node:crypto'
import * as fs from 'node:fs'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import test from 'node:test'
import { accountConfig, accountBrowserURL } from '../main/fork-account/config.ts'
import { emailCodeSignIn, passwordResetRules, signUpRules } from '../main/fork-account/entry-rules.ts'
import { loadMainModule } from './test-support/main-module.ts'

// E6a (D70): segundo fator e cadastro por e-mail contra um FAPI falso. Fronteiras: sem o fator não há
// sessão; código errado não confirma; recuperação vale uma vez; senha e códigos não vão a estado/log/disco.
const host = 'identity.example.test'
const config = { environment: 'main', deploymentEnv: 'main', clerkPublishableKey: `pk_test_${Buffer.from(`${host}$`).toString('base64')}`, clerkFrontendOrigin: `https://${host}`, clerkProxyOrigin: 'https://auth.example.invalid', clerkTokenOrigin: `https://${host}`, clerkIssuer: `https://${host}`, cloudURL: 'https://cloud.example.test', deploymentId: 'fixture', manifestHash: 'a'.repeat(64) }
const identifier = 'fixture@example.test'
const password = 'fixture-secret-long-enough'
const totp = '246810'
const backup = 'rcv-7f3k-9q2m'
const emailCode = '135791'
const signInCode = '864201'
const expiredCode = '999999'
const resetCode = '520520'
const newPassword = 'nova-senha-bem-comprida'
const pwned = 'password1234567890'

type Reply = { ok: boolean; status: number; headers: { get: (name: string) => string | null }; json: () => Promise<unknown> }
function fakeFapi({ mfa = true, trust = false } = {}) {
  const signIns = new Map<string, { status: string; session?: string }>()
  const signUps = new Map<string, { status: string; session?: string }>()
  const sessions = new Map<string, string>() // credential → active session
  const usedBackup = new Set<string>()
  const calls: string[] = []
  let n = 0
  const reply = (value: unknown, auth: string | null = null, status = 200): Reply => ({ ok: status < 300, status, headers: { get: (k: string) => (k.toLowerCase() === 'authorization' ? auth : null) }, json: async () => value })
  const fetch = async (url: URL, init: { method: string; body?: string; headers: Record<string, string> }): Promise<Reply> => {
    if (url.pathname === '/v1/environment') return reply({})
    const body = new URLSearchParams(init.body)
    const auth = init.headers.Authorization ?? null
    calls.push(`${init.method} ${url.pathname}`)
    let m: RegExpMatchArray | null
    if (url.pathname === '/v1/client/sign_ins' && init.method === 'POST') {
      const id = `attempt-${++n}`; signIns.set(id, { status: 'needs_first_factor' })
      return reply({ response: { id, status: 'needs_first_factor', supported_first_factors: [{ strategy: 'password' }, { strategy: 'email_code', email_address_id: 'idn_fixture' }, { strategy: 'reset_password_email_code', email_address_id: 'idn_fixture' }] } }, `cred-${n}`)
    }
    if ((m = url.pathname.match(/^\/v1\/client\/sign_ins\/([^/]+)\/prepare_first_factor$/))) {
      assert.ok(['email_code', 'reset_password_email_code'].includes(body.get('strategy')!)); assert.equal(body.get('email_address_id'), 'idn_fixture')
      return reply({ response: { id: m[1], status: 'needs_first_factor' } })
    }
    if ((m = url.pathname.match(/^\/v1\/client\/sign_ins\/([^/]+)\/prepare_second_factor$/))) {
      assert.equal(body.get('strategy'), 'email_code'); assert.equal(body.get('email_address_id'), 'idn_fixture')
      return reply({ response: { id: m[1], status: signIns.get(m[1]!)!.status } })
    }
    if ((m = url.pathname.match(/^\/v1\/client\/sign_ins\/([^/]+)\/attempt_first_factor$/))) {
      const s = signIns.get(m[1]!)!
      if (body.get('strategy') === 'reset_password_email_code') {
        if (body.get('code') === expiredCode) return reply({ errors: [{ code: 'verification_expired' }] }, null, 422)
        if (body.get('code') !== resetCode) return reply({ errors: [{ code: 'form_code_incorrect' }] }, null, 422)
        s.status = 'needs_new_password'
        return reply({ response: { id: m[1], status: s.status } })
      }
      if (body.get('strategy') === 'email_code') {
        if (body.get('code') === expiredCode) return reply({ errors: [{ code: 'verification_expired' }] }, null, 422)
        if (body.get('code') !== signInCode) return reply({ errors: [{ code: 'form_code_incorrect' }] }, null, 422)
      } else if (body.get('password') !== password) return reply({ errors: [{ code: 'form_password_incorrect' }] }, null, 422)
      if (trust && body.get('strategy') === 'password') { s.status = 'needs_client_trust'; return reply({ response: { id: m[1], status: s.status, supported_second_factors: [{ strategy: 'email_code', email_address_id: 'idn_fixture' }] } }) }
      if (!mfa) { s.status = 'complete'; s.session = `session-${m[1]}`; sessions.set(auth!, s.session); return reply({ response: { id: m[1], status: 'complete', created_session_id: s.session } }) }
      s.status = 'needs_second_factor'
      return reply({ response: { id: m[1], status: s.status, supported_second_factors: [{ strategy: 'totp' }, { strategy: 'backup_code' }] } })
    }
    if ((m = url.pathname.match(/^\/v1\/client\/sign_ins\/([^/]+)\/attempt_second_factor$/))) {
      const s = signIns.get(m[1]!)
      if (!s || (s.status !== 'needs_second_factor' && s.status !== 'needs_client_trust')) return reply({ errors: [{ code: 'verification_expired' }] }, null, 422)
      const strategy = body.get('strategy'); const code = body.get('code') ?? ''
      const right = s.status === 'needs_client_trust' ? strategy === 'email_code' && code === signInCode : strategy === 'totp' ? code === totp : strategy === 'backup_code' && code === backup && !usedBackup.has(code)
      if (!right) return reply({ errors: [{ code: 'form_code_incorrect' }] }, null, 422)
      if (strategy === 'backup_code') usedBackup.add(code)
      s.status = 'complete'; s.session = `session-${m[1]}`; sessions.set(auth!, s.session)
      return reply({ response: { id: m[1], status: 'complete', created_session_id: s.session } })
    }
    if ((m = url.pathname.match(/^\/v1\/client\/sign_ins\/([^/]+)\/reset_password$/))) {
      const s = signIns.get(m[1]!)!
      assert.equal(s.status, 'needs_new_password')
      const next = body.get('password') ?? ''
      if (next.length < 15) return reply({ errors: [{ code: 'form_password_length_too_short' }] }, null, 422)
      if (next === pwned) return reply({ errors: [{ code: 'form_password_pwned' }] }, null, 422)
      if (mfa) { s.status = 'needs_second_factor'; return reply({ response: { id: m[1], status: s.status, supported_second_factors: [{ strategy: 'totp' }] } }) }
      s.status = 'complete'; s.session = `session-${m[1]}`; sessions.set(auth!, s.session)
      return reply({ response: { id: m[1], status: 'complete', created_session_id: s.session } })
    }
    if ((m = url.pathname.match(/^\/v1\/client\/sign_ins\/([^/]+)$/))) {
      const s = signIns.get(m[1]!)
      return s ? reply({ response: { id: m[1], identifier, status: s.status, created_session_id: s.session ?? null } }) : reply({}, null, 404)
    }
    if (url.pathname === '/v1/client/sign_ups' && init.method === 'POST') {
      assert.equal(body.get('email_address'), identifier)
      const id = `signup-${++n}`; signUps.set(id, { status: 'missing_requirements' })
      return reply({ response: { id, status: 'missing_requirements' } }, `cred-${n}`)
    }
    if ((m = url.pathname.match(/^\/v1\/client\/sign_ups\/([^/]+)\/prepare_verification$/))) return reply({ response: { id: m[1], status: 'missing_requirements' } })
    if ((m = url.pathname.match(/^\/v1\/client\/sign_ups\/([^/]+)\/attempt_verification$/))) {
      const s = signUps.get(m[1]!)!
      if (body.get('code') !== emailCode) return reply({ errors: [{ code: 'form_code_incorrect' }] }, null, 422)
      s.status = 'complete'; s.session = `session-${m[1]}`; sessions.set(auth!, s.session)
      return reply({ response: { id: m[1], status: 'complete', created_session_id: s.session } })
    }
    if (url.pathname === '/v1/client') {
      const session = auth ? sessions.get(auth) : undefined
      return reply({ response: { sessions: session ? [{ id: session, status: 'active', user: { id: `user-${session}` } }] : [], last_active_session_id: session ?? null } })
    }
    if ((m = url.pathname.match(/^\/v1\/client\/sessions\/([^/]+)\/remove$/))) { for (const [k, v] of sessions) if (v === m[1]) sessions.delete(k); return reply({ response: {} }) }
    return reply({}, null, 404)
  }
  return { fetch, calls, sessions }
}

function service(fetch: unknown, logs: unknown[]) {
  const dir = mkdtempSync(path.join(tmpdir(), 'entry-codes-'))
  const key = crypto.randomBytes(32)
  const safeStorage = {
    isEncryptionAvailable: () => true,
    encryptString(value: string) { const iv = crypto.randomBytes(12); const c = crypto.createCipheriv('aes-256-gcm', key, iv); const data = Buffer.concat([c.update(value, 'utf8'), c.final()]); return Buffer.concat([iv, c.getAuthTag(), data]) },
    decryptString(value: Buffer) { const c = crypto.createDecipheriv('aes-256-gcm', key, value.subarray(0, 12)); c.setAuthTag(value.subarray(12, 28)); return Buffer.concat([c.update(value.subarray(28)), c.final()]).toString('utf8') },
  }
  const mod = loadMainModule<typeof import('../main/account.ts')>('account', {
    'node:events': { EventEmitter }, 'node:crypto': crypto, 'node:fs': fs, 'node:path': path,
    electron: { app: { getPath: () => dir, getVersion: () => 'fixture', isPackaged: true }, ipcMain: { handle: () => {} }, safeStorage },
    './auth': { AuthenticationEnded: class extends Error {} }, './environment': { identity: { appId: 'app.fixture', releaseUpdates: false }, buildConfig: config },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: true } }, './fork-account/config': { accountConfig, accountBrowserURL },
  }, { URL, URLSearchParams, Buffer, __dirname: '/fixture/main', process: { platform: 'darwin', env: {} }, fetch, console: { warn: (...args: unknown[]) => logs.push(args) } })
  const account = new mod.AccountService()
  return { account, dir }
}

const restore = (s: unknown) => (s as { restore(): Promise<void> }).restore()
const secrets = [password, totp, backup, emailCode, signInCode, resetCode, newPassword]
function assertNoSecrets(where: string, text: string) { for (const secret of secrets) assert.equal(text.includes(secret), false, `${where} sem segredo`) }
function assertDiskClean(dir: string) {
  for (const sub of fs.readdirSync(dir)) {
    const folder = path.join(dir, sub)
    for (const name of fs.statSync(folder).isDirectory() ? fs.readdirSync(folder) : []) {
      const bytes = fs.readFileSync(path.join(folder, name))
      for (const secret of [...secrets, identifier]) assert.equal(bytes.includes(Buffer.from(secret)), false)
    }
  }
}

test('segundo fator: sem código não entra; código errado recusa; autenticador certo confirma', async () => {
  const fapi = fakeFapi(); const logs: unknown[] = []
  const { account, dir } = service(fapi.fetch, logs)
  try {
    await restore(account)
    assert.equal(await account.signInWithPassword(identifier, password), 'verification')
    assert.equal(account.state.user, null, 'sem o fator não há usuário')
    assert.equal(JSON.stringify(account.state.pending), JSON.stringify({ kind: 'second_factor', strategies: ['totp', 'backup_code'] }))
    assert.equal(account.captureCloudSession(), null, 'sem selo de nuvem')
    assert.equal(fapi.calls.includes('GET /v1/client'), false, 'sessão não lida antes do fator')
    // Outros métodos ficam fechados enquanto o fator espera.
    assert.equal(await account.signInWithPassword(identifier, password), 'error')

    assert.equal(await account.attemptSecondFactor('totp', '000000'), 'verification')
    assert.equal(account.state.user, null)
    assert.equal(account.state.error, 'Código incorreto ou expirado. Confira o app autenticador.')
    assert.equal(account.state.pending?.kind, 'second_factor')
    assert.equal(fapi.calls.includes('GET /v1/client'), false)
    assert.equal(await account.attemptSecondFactor('totp', '12ab'), 'error', 'formato inválido nem sai do Main')

    assert.equal(await account.attemptSecondFactor('totp', totp), null)
    assert.equal((account.state.user as { userId: string } | null)?.userId, 'user-session-attempt-1')
    assert.equal(account.state.pending, null)
    assert.notEqual(account.captureCloudSession(), null)
    assertNoSecrets('estado', JSON.stringify(account.state)); assertNoSecrets('log', JSON.stringify(logs)); assertDiskClean(dir)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test('código de recuperação vale uma vez; cancelar volta ao início sem sessão', async () => {
  const fapi = fakeFapi(); const logs: unknown[] = []
  const { account, dir } = service(fapi.fetch, logs)
  try {
    await restore(account)
    await account.signInWithPassword(identifier, password)
    assert.equal(await account.attemptSecondFactor('backup_code', backup), null)
    assert.ok(account.state.user)
    await account.signOut()
    assert.equal(account.state.user, null)

    assert.equal(await account.signInWithPassword(identifier, password), 'verification')
    assert.equal(await account.attemptSecondFactor('backup_code', backup), 'verification')
    assert.equal(account.state.user, null, 'segundo uso recusado pelo servidor')
    assert.match(account.state.error ?? '', /recuperação incorreto ou já usado/)

    await account.cancelPending()
    assert.equal(account.state.pending, null)
    assert.equal(account.state.user, null)
    assert.equal(account.state.error, null)
    assert.equal(await account.attemptSecondFactor('totp', totp), 'error', 'tentativa cancelada não aceita código')
    // Volta a aceitar um login novo, sem reiniciar.
    assert.equal(await account.signInWithPassword(identifier, password), 'verification')
    assertNoSecrets('estado', JSON.stringify(account.state)); assertNoSecrets('log', JSON.stringify(logs)); assertDiskClean(dir)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test('cadastro: regras do ambiente; código errado recusa; certo cria e confirma', async () => {
  const environment = { user_settings: { sign_up: { mode: 'public' }, password_settings: { min_length: 15 }, attributes: { email_address: { enabled: true, required: true, used_for_first_factor: true, verifications: ['email_code'] }, password: { enabled: true, required: true }, first_name: { enabled: true, required: false } } } }
  assert.deepEqual(signUpRules(environment), { minPasswordLength: 15 })
  assert.equal(signUpRules({ user_settings: { ...environment.user_settings, sign_up: { mode: 'restricted' } } }), null)
  assert.equal(signUpRules({ user_settings: { ...environment.user_settings, attributes: { ...environment.user_settings.attributes, first_name: { enabled: true, required: true } } } }), null)
  assert.equal(signUpRules({}), null)

  const fapi = fakeFapi(); const logs: unknown[] = []
  const { account, dir } = service(fapi.fetch, logs)
  try {
    await restore(account)
    assert.equal(await account.signUpWithPassword(identifier, password), 'error', 'sem ambiente lido, sem cadastro')
    account.state = { ...account.state, signUp: signUpRules(environment) }
    assert.equal(await account.signUpWithPassword(identifier, 'curta-demais'), 'error')
    assert.equal(account.state.error, 'A senha precisa ter pelo menos 15 caracteres.')
    assert.equal(fapi.calls.some(c => c.includes('sign_ups')), false)

    assert.equal(await account.signUpWithPassword(identifier, password), 'verification')
    assert.equal(account.state.pending?.kind, 'email_code')
    assert.equal(account.state.user, null)
    const early = await account.resendEmailCode()
    assert.equal(early.kind, 'limit', 'antes do prazo o resultado é o limite, não um envio')
    assert.equal(fapi.calls.filter(c => c.endsWith('/prepare_verification')).length, 1, 'reenviar espera')

    assert.equal(await account.verifyEmailCode('000000'), 'verification')
    assert.equal(account.state.user, null)
    assert.equal(account.state.error, 'Código incorreto ou expirado. Confira o e-mail mais recente.')
    assert.equal(fapi.calls.includes('GET /v1/client'), false)

    assert.equal(await account.verifyEmailCode(emailCode), null)
    assert.equal((account.state.user as { userId: string } | null)?.userId, 'user-session-signup-1')
    assert.equal(account.state.pending, null)
    assertNoSecrets('estado', JSON.stringify(account.state)); assertNoSecrets('log', JSON.stringify(logs)); assertDiskClean(dir)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test('entrar com código por e-mail: desligado não aparece e o Main recusa; certo entra; errado e expirado recusam', async () => {
  assert.equal(emailCodeSignIn({ user_settings: { attributes: { email_address: { used_for_first_factor: true, first_factors: ['email_code'] } } } }), true)
  assert.equal(emailCodeSignIn({ user_settings: { attributes: { email_address: { used_for_first_factor: true, first_factors: [] } } } }), false)
  assert.equal(emailCodeSignIn({}), false)

  const fapi = fakeFapi({ mfa: false }); const logs: unknown[] = []
  const { account, dir } = service(fapi.fetch, logs)
  try {
    await restore(account)
    assert.equal(account.state.emailCode, false, 'fail-closed antes do ambiente')
    assert.equal(await account.signInWithEmailCode(identifier), 'error')
    assert.equal(fapi.calls.length, 0, 'nada enviado com a opção desligada')
    account.state = { ...account.state, emailCode: true }

    assert.equal(await account.signInWithEmailCode(identifier), 'verification')
    assert.equal(JSON.stringify(account.state.pending && { kind: account.state.pending.kind, purpose: 'purpose' in account.state.pending ? account.state.pending.purpose : null }), JSON.stringify({ kind: 'email_code', purpose: 'sign_in' }))
    assert.equal(account.state.user, null)
    assert.equal(await account.signInWithPassword(identifier, password), 'error', 'outro método fechado enquanto o código espera')

    assert.equal(await account.verifyEmailCode('000000'), 'verification')
    assert.equal(account.state.error, 'Código incorreto ou expirado. Confira o e-mail mais recente.')
    assert.equal(await account.verifyEmailCode(expiredCode), 'verification')
    assert.equal(account.state.error, 'O código expirou. Use Reenviar código.')
    assert.equal(account.state.user, null)
    assert.equal(fapi.calls.includes('GET /v1/client'), false)
    assert.equal((await account.resendEmailCode()).kind, 'limit')
    assert.equal(fapi.calls.filter(c => c.endsWith('/prepare_first_factor')).length, 1, 'reenviar respeita a espera')

    assert.equal(await account.verifyEmailCode(signInCode), null)
    assert.equal((account.state.user as { userId: string } | null)?.userId, 'user-session-attempt-1')
    assert.equal(account.state.pending, null)
    assertNoSecrets('estado', JSON.stringify(account.state)); assertNoSecrets('log', JSON.stringify(logs)); assertDiskClean(dir)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test('código por e-mail com 2FA ligado: o segundo fator continua obrigatório', async () => {
  const fapi = fakeFapi(); const logs: unknown[] = []
  const { account, dir } = service(fapi.fetch, logs)
  try {
    await restore(account)
    account.state = { ...account.state, emailCode: true }
    await account.signInWithEmailCode(identifier)
    assert.equal(await account.verifyEmailCode(signInCode), 'verification')
    assert.equal(account.state.user, null, 'o código do e-mail não substitui o segundo fator')
    assert.equal(account.state.pending?.kind, 'second_factor')
    assert.equal(account.captureCloudSession(), null)
    assert.equal(await account.attemptSecondFactor('totp', totp), null)
    assert.ok(account.state.user)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test('Device Trust (produção): senha num aparelho novo pede o código do e-mail antes da sessão', async () => {
  const fapi = fakeFapi({ mfa: false, trust: true }); const logs: unknown[] = []
  const { account, dir } = service(fapi.fetch, logs)
  try {
    await restore(account)
    assert.equal(await account.signInWithPassword(identifier, password), 'verification')
    assert.equal(account.state.user, null)
    assert.equal(account.state.pending?.kind === 'email_code' && account.state.pending.purpose, 'device_trust')
    assert.equal(fapi.calls.filter(c => c.endsWith('/prepare_second_factor')).length, 1)
    assert.equal(await account.verifyEmailCode('000000'), 'verification')
    assert.equal(account.state.user, null)
    assert.equal(await account.verifyEmailCode(signInCode), null)
    assert.ok(account.state.user)
    assertNoSecrets('estado', JSON.stringify(account.state)); assertNoSecrets('log', JSON.stringify(logs)); assertDiskClean(dir)
  } finally { rmSync(dir, { recursive: true, force: true }) }
})

test('esqueci minha senha: código errado/expirado recusa; senha fraca recusa com a regra; certo entra; 2FA continua', async () => {
  const env = { user_settings: { password_settings: { min_length: 15 }, attributes: { password: { enabled: true }, email_address: { enabled: true, used_for_first_factor: true, verifications: ['email_code'] } } } }
  assert.deepEqual(passwordResetRules(env), { minPasswordLength: 15 })
  assert.equal(passwordResetRules({ user_settings: { ...env.user_settings, attributes: { ...env.user_settings.attributes, password: { enabled: false } } } }), null)

  const fapi = fakeFapi({ mfa: false }); const logs: unknown[] = []
  const { account, dir } = service(fapi.fetch, logs)
  try {
    await restore(account)
    assert.equal(await account.forgotPassword(identifier), 'error', 'desligado: o Main recusa')
    assert.equal(fapi.calls.length, 0)
    account.state = { ...account.state, passwordReset: passwordResetRules(env) }
    assert.equal(await account.forgotPassword(identifier), 'verification')
    assert.equal(account.state.pending?.kind === 'email_code' && account.state.pending.purpose, 'reset_password')
    assert.equal(await account.verifyEmailCode('000000'), 'verification')
    assert.equal(account.state.error, 'Código incorreto ou expirado. Confira o e-mail mais recente.')
    assert.equal(await account.verifyEmailCode(expiredCode), 'verification')
    assert.equal(account.state.error, 'O código expirou. Use Reenviar código.')
    assert.equal(await account.verifyEmailCode(resetCode), 'verification')
    assert.equal(account.state.pending?.kind, 'new_password')
    assert.equal(account.state.user, null)

    assert.equal(await account.resetPassword('curta'), 'error')
    assert.equal(account.state.error, 'A senha precisa ter pelo menos 15 caracteres.')
    assert.equal(fapi.calls.some(c => c.endsWith('/reset_password')), false, 'regra local antes de enviar')
    assert.equal(await account.resetPassword(pwned), 'verification')
    assert.equal(account.state.error, 'Esta senha aparece em vazamentos conhecidos. Escolha outra.')
    assert.equal(account.state.user, null)
    assert.equal(await account.resetPassword(newPassword), null)
    assert.ok(account.state.user)
    assert.equal(account.state.pending, null)
    assertNoSecrets('estado', JSON.stringify(account.state)); assertNoSecrets('log', JSON.stringify(logs)); assertDiskClean(dir)
  } finally { rmSync(dir, { recursive: true, force: true }) }

  const withMfa = fakeFapi(); const second = service(withMfa.fetch, [])
  try {
    await restore(second.account)
    second.account.state = { ...second.account.state, passwordReset: passwordResetRules(env) }
    await second.account.forgotPassword(identifier)
    await second.account.verifyEmailCode(resetCode)
    assert.equal(await second.account.resetPassword(newPassword), 'verification')
    assert.equal(second.account.state.user, null, 'nova senha não pula o segundo fator')
    assert.equal(second.account.state.pending?.kind, 'second_factor')
    assert.equal(await second.account.attemptSecondFactor('totp', totp), null)
    assert.ok(second.account.state.user)
  } finally { rmSync(second.dir, { recursive: true, force: true }) }
})

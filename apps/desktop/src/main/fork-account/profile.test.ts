import assert from 'node:assert/strict'
import test from 'node:test'
import * as shared from '../../shared/fork-account-profile.ts'
import { loadMainModule } from '../../shared/test-support/main-module.ts'
import type * as Profile from './profile.ts'

const { AccountProfileController, profileFromUser, recoveryFile } = loadMainModule<typeof Profile>('fork-account/profile', { '../../shared/fork-account-profile': shared })
const rules = { minLength: 12, maxLength: null, requireUppercase: false, requireLowercase: false, requireNumbers: false, requireSpecial: false }

function fapi(user: Record<string, unknown>, answers: Record<string, (body?: Record<string, unknown>) => unknown> = {}) {
  const calls: { method: string; path: string; body?: Record<string, unknown> }[] = []
  const controller = new AccountProfileController({
    call: async <T,>(method: string, path: string, body?: Record<string, string | boolean>) => {
      calls.push({ method, path, body })
      const answer = answers[`${method} ${path}`]
      if (answer) return answer(body) as T
      if (method === 'GET' && path === '/v1/me') return user as T
      throw { status: 404, code: 'resource_not_found' }
    },
    sessionId: () => 'sess_1', socialProviders: () => ['google'], passwordRules: () => rules,
    oauth: async () => new URL('app://callback'), callbackURL: () => 'app://callback?attempt=x',
  })
  return { controller, calls }
}
const google = { id: 'eac_1', provider: 'oauth_google', email_address: 'a@b.c', verification: { status: 'verified' } }

test('não desliga o último jeito de entrar (conta só com Google) e nem chama o Clerk', async () => {
  const { controller, calls } = fapi({ password_enabled: false, external_accounts: [google] })
  const result = await controller.unlink('eac_1')
  assert.equal(result.ok, false)
  assert.equal(!result.ok && result.failure, 'last_method')
  assert.ok(!calls.some(c => c.method === 'DELETE'))
})

test('desliga o Google quando há senha', async () => {
  const { controller, calls } = fapi({ password_enabled: true, external_accounts: [google] }, { 'DELETE /v1/me/external_accounts/eac_1': () => ({}) })
  assert.equal((await controller.unlink('eac_1')).ok, true)
  assert.ok(calls.some(c => c.method === 'DELETE' && c.path === '/v1/me/external_accounts/eac_1'))
})

test('recusa do Clerk ao desligar vira explicação simples', async () => {
  const { controller } = fapi({ password_enabled: true, external_accounts: [google] }, { 'DELETE /v1/me/external_accounts/eac_1': () => { throw { status: 422, code: 'something' } } })
  const result = await controller.unlink('eac_1')
  assert.equal(!result.ok && result.failure, 'last_method')
  assert.match(!result.ok ? result.message : '', /não deixou desligar/)
})

test('2FA só liga com código aceito pelo Clerk e mostra os códigos uma vez, sem guardá-los', async () => {
  let accepted = false
  const { controller, calls } = fapi({}, {
    'POST /v1/me/totp': () => ({ uri: 'otpauth://totp/Donnabot:a?secret=JBSWY3DP', secret: 'JBSWY3DP' }),
    'POST /v1/me/totp/attempt_verification': (body) => {
      if (body?.code !== '123456') throw { status: 422, code: 'form_code_incorrect' }
      accepted = true
      return { verified: true, backup_codes: ['aaaa1111', 'bbbb2222'] }
    },
  })
  const early = await controller.confirmTotp('123456')
  assert.equal(early.ok, false, 'sem QR iniciado não confirma')
  assert.equal((await controller.startTotp()).ok, true)
  const malformed = await controller.confirmTotp('12a')
  assert.equal(!malformed.ok && malformed.failure, 'code_invalid')
  assert.equal(calls.filter(c => c.path.endsWith('attempt_verification')).length, 0, 'formato inválido não vai ao Clerk')
  const wrong = await controller.confirmTotp('000000')
  assert.equal(!wrong.ok && wrong.failure, 'code_invalid')
  assert.equal(accepted, false)
  const right = await controller.confirmTotp('123 456')
  assert.equal(JSON.stringify(right.ok && right.value.codes), '["aaaa1111","bbbb2222"]')
  assert.ok(!JSON.stringify(controller).includes('aaaa1111'), 'códigos não ficam no controlador')
  assert.ok(!JSON.stringify(controller).includes('JBSWY3DP'), 'segredo não fica no controlador')
  assert.equal((await controller.confirmTotp('123456')).ok, false, 'não reaproveita o QR já confirmado')
})

test('Clerk que responde verified=false não liga a verificação', async () => {
  const { controller } = fapi({}, {
    'POST /v1/me/totp': () => ({ uri: 'otpauth://totp/x?secret=JBSWY3DP', secret: 'JBSWY3DP' }),
    'POST /v1/me/totp/attempt_verification': () => ({ verified: false }),
  })
  await controller.startTotp()
  const result = await controller.confirmTotp('123456')
  assert.equal(!result.ok && result.failure, 'code_invalid')
})

test('troca de senha com a atual errada é recusada; sem a atual nem chega ao Clerk', async () => {
  const { controller, calls } = fapi({ password_enabled: true }, { 'POST /v1/me/change_password': () => { throw { status: 422, code: 'form_password_incorrect' } } })
  const wrong = await controller.setPassword('errada', 'uma-senha-nova-longa')
  assert.equal(!wrong.ok && wrong.failure, 'password_incorrect')
  const missing = await controller.setPassword(null, 'uma-senha-nova-longa')
  assert.equal(!missing.ok && missing.failure, 'password_incorrect')
  assert.equal(calls.filter(c => c.path === '/v1/me/change_password').length, 1)
  const short = await controller.setPassword('atual', 'curta')
  assert.equal(!short.ok && short.failure, 'password_rules')
  assert.equal(calls.filter(c => c.path === '/v1/me/change_password').length, 1)
})

test('conta só com Google cria a primeira senha sem senha atual', async () => {
  const { controller, calls } = fapi({ password_enabled: false }, { 'POST /v1/me/change_password': () => ({}) })
  assert.equal((await controller.setPassword(null, 'uma-senha-nova-longa')).ok, true)
  const sent = calls.find(c => c.path === '/v1/me/change_password')?.body
  assert.equal(sent?.new_password, 'uma-senha-nova-longa')
  assert.equal('current_password' in (sent ?? {}), false)
})

test('nome é salvo pelo PATCH /v1/me, sem espaços nas pontas', async () => {
  const { controller, calls } = fapi({}, { 'PATCH /v1/me': (body) => ({ first_name: body?.first_name, last_name: body?.last_name }) })
  const result = await controller.saveName('  Ana ', ' Souza ')
  assert.equal(JSON.stringify(calls.at(-1)), JSON.stringify({ method: 'PATCH', path: '/v1/me', body: { first_name: 'Ana', last_name: 'Souza' } }))
  assert.equal(result.ok && `${result.value.firstName} ${result.value.lastName}`, 'Ana Souza')
})

test('só mostra provedores ligados na instância; arquivo de códigos só com códigos válidos', () => {
  const profile = profileFromUser({ external_accounts: [google, { id: 'eac_2', provider: 'oauth_apple', verification: { status: 'verified' } }] }, ['google'], null)
  assert.equal(profile.linked.map(a => a.provider).join(), 'google')
  assert.equal(recoveryFile(['../etc']), null)
  assert.match(recoveryFile(['aaaa1111']) ?? '', /aaaa1111/)
})

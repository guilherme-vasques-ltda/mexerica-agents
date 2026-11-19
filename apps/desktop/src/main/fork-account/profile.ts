import type { SignInProvider } from '../../shared/analytics'
import { canUnlink, passwordProblem, type AccountProfile, type PasswordRules, type ProfileFailure, type ProfileResult, type RecoveryCodes, type Reverification, type ReverifyAttempt, type TotpSetup } from '../../shared/fork-account-profile'
import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'

/**
 * E6b (DON-864): Ajustes → Conta pelo FAPI nativo do Clerk (`/v1/me`), no processo principal.
 * Sem electron aqui: o service entrega a chamada da sessão atual e o navegador; o IPC fica em
 * `profile-ipc.ts`. Senhas, segredo TOTP e códigos de recuperação só passam pela resposta do
 * pedido: nada é guardado, registrado ou enviado a análise de uso.
 */

export type ProfileMethod = 'GET' | 'POST' | 'PATCH' | 'DELETE'
export interface ProfileDeps {
  /** FAPI da sessão confirmada; lança `{ status, code }` numa recusa. */
  call<T>(method: ProfileMethod, path: string, body?: Record<string, string | boolean>): Promise<T>
  sessionId(): string | null
  socialProviders(): SignInProvider[]
  passwordRules(): PasswordRules | null
  /** Abre o navegador e devolve o retorno já conferido; lança `{ cancelled: true }` se a pessoa desistiu. */
  oauth(redirect: string, callback: string): Promise<URL>
  callbackURL(): string
}

interface FapiUser {
  first_name?: string | null
  last_name?: string | null
  password_enabled?: boolean
  totp_enabled?: boolean
  backup_code_enabled?: boolean
  primary_email_address_id?: string | null
  email_addresses?: { id: string; email_address: string }[]
  external_accounts?: { id?: string; provider?: string; email_address?: string | null; verification?: { status?: string } | null }[]
}
interface FapiVerification { status?: string; supported_first_factors?: { strategy?: string; email_address_id?: string }[] | null; supported_second_factors?: { strategy?: string }[] | null }

const PROVIDERS: Record<string, SignInProvider> = { oauth_google: 'google', google: 'google', oauth_apple: 'apple', apple: 'apple', oauth_x: 'x', x: 'x' }
const STRATEGY: Record<SignInProvider, string> = { google: 'oauth_google', apple: 'oauth_apple', x: 'oauth_x' }
const NAME: Record<SignInProvider, string> = { google: 'Google', apple: 'Apple', x: 'X' }

export function rulesFromEnvironment(environment: unknown): PasswordRules | null {
  const p = (environment as { user_settings?: { password_settings?: Record<string, unknown> } } | null)?.user_settings?.password_settings
  if (!p || typeof p !== 'object') return null
  const n = (v: unknown) => typeof v === 'number' && Number.isFinite(v) && v > 0 ? Math.floor(v) : null
  return { minLength: n(p.min_length) ?? 8, maxLength: n(p.max_length), requireUppercase: p.require_uppercase === true, requireLowercase: p.require_lowercase === true, requireNumbers: p.require_numbers === true, requireSpecial: p.require_special_char === true }
}

export function profileFromUser(user: FapiUser, social: SignInProvider[], rules: PasswordRules | null): AccountProfile {
  const linked = (user.external_accounts ?? []).flatMap(a => {
    const provider = PROVIDERS[a.provider ?? '']
    return provider && typeof a.id === 'string' && a.id ? [{ id: a.id, provider, email: a.email_address ?? null, verified: a.verification?.status === 'verified' }] : []
  })
  const email = user.email_addresses?.find(e => e.id === user.primary_email_address_id)?.email_address ?? user.email_addresses?.[0]?.email_address ?? null
  return {
    firstName: user.first_name ?? '', lastName: user.last_name ?? '', email,
    hasPassword: user.password_enabled === true, totp: user.totp_enabled === true, backupCodes: user.backup_code_enabled === true,
    // Só os provedores ligados na instância (#325): Apple e X seguem escondidos na produção.
    linked: linked.filter(a => social.includes(a.provider)),
    linkable: social.filter(p => !linked.some(a => a.provider === p && a.verified)),
    passwordRules: rules,
  }
}

function reverification(v: FapiVerification): Reverification | null {
  const status = v.status === 'needs_first_factor' || v.status === 'needs_second_factor' || v.status === 'complete' ? v.status : null
  if (!status) return null
  const offered = new Set([...(v.supported_first_factors ?? []), ...(v.supported_second_factors ?? [])].map(f => f.strategy))
  const order = status === 'needs_second_factor' ? ['totp', 'backup_code'] as const : ['password', 'email_code'] as const
  return { status, factors: order.filter(f => offered.has(f)) }
}

const fail = (failure: ProfileFailure, message: string): { ok: false; failure: ProfileFailure; message: string } => ({ ok: false, failure, message })
const UNAVAILABLE = 'Não foi possível falar com a sua conta agora. Confira a internet e tente de novo.'

/** Recusa do Clerk → frase simples. Nada da resposta original chega à tela ou ao log. */
export function refusal(error: unknown): { ok: false; failure: ProfileFailure; message: string } {
  const e = error as { status?: unknown; code?: unknown; cancelled?: unknown } | null
  if (e?.cancelled === true) return fail('cancelled', 'Ligação cancelada. Nada mudou na sua conta.')
  const code = typeof e?.code === 'string' ? e.code : null
  if (code === 'session_reverification_required') return fail('reverify', 'Confirme que é você para continuar.')
  if (code === 'form_password_incorrect' || code === 'form_password_or_identifier_incorrect') return fail('password_incorrect', 'A senha atual não confere.')
  if (code === 'form_password_pwned') return fail('password_rules', 'Essa senha apareceu em vazamentos conhecidos. Escolha outra.')
  if (code?.startsWith('form_password')) return fail('password_rules', 'Essa senha não atende às regras da conta. Escolha outra.')
  if (code === 'form_code_incorrect' || code === 'verification_failed' || code === 'form_param_format_invalid') return fail('code_invalid', 'Código incorreto ou vencido. Confira o app autenticador e tente de novo.')
  if (code === 'external_account_exists' || code === 'oauth_identification_claimed' || code === 'identification_exists' || code === 'form_identifier_exists') return fail('link_claimed', traduzir('pt-BR', 'marca.conta.googleLigado'))
  if (code === 'external_account_missing_refresh_token' || code === 'oauth_access_denied') return fail('cancelled', 'Ligação cancelada. Nada mudou na sua conta.')
  if (code === 'app_last_method') return fail('last_method', 'Esse é o seu único jeito de entrar. Crie uma senha antes de desligar o Google.')
  if (code === 'app_unlink_refused') return fail('last_method', 'O Clerk não deixou desligar essa conta. Ela continua ligada e nada mudou.')
  if (code === 'app_link_failed') return fail('unavailable', 'Não foi possível ligar a conta agora. Nada mudou na sua conta.')
  if (code === 'app_password_required') return fail('password_incorrect', 'Digite a senha atual.')
  return fail('unavailable', UNAVAILABLE)
}

export class AccountProfileController {
  private totpStarted = false
  private emailAddressId: string | null = null
  constructor(private readonly deps: ProfileDeps) {}

  private async me(): Promise<AccountProfile> {
    return profileFromUser(await this.deps.call<FapiUser>('GET', '/v1/me'), this.deps.socialProviders(), this.deps.passwordRules())
  }
  private async run<T>(work: () => Promise<T>): Promise<ProfileResult<T>> {
    try { return { ok: true, value: await work() } } catch (error) { return refusal(error) }
  }

  load() { return this.run(() => this.me()) }

  async saveName(firstName: unknown, lastName: unknown): Promise<ProfileResult> {
    if (typeof firstName !== 'string' || typeof lastName !== 'string') return fail('name_invalid', 'Nome inválido.')
    const first = firstName.trim(), last = lastName.trim()
    if (first.length > 100 || last.length > 100) return fail('name_invalid', 'Use no máximo 100 caracteres em cada campo.')
    return this.run(async () => profileFromUser(await this.deps.call<FapiUser>('PATCH', '/v1/me', { first_name: first, last_name: last }), this.deps.socialProviders(), this.deps.passwordRules()))
  }

  async setPassword(current: unknown, next: unknown): Promise<ProfileResult> {
    if (typeof next !== 'string' || (current !== null && typeof current !== 'string')) return fail('password_rules', 'Senha inválida.')
    const problem = passwordProblem(next, this.deps.passwordRules())
    if (problem) return fail('password_rules', problem)
    return this.run(async () => {
      const profile = await this.me()
      // Quem já tem senha precisa da atual; quem não tem (só Google) cria a primeira.
      if (profile.hasPassword && !current) throw { code: 'app_password_required' }
      const body: Record<string, string | boolean> = { new_password: next, sign_out_of_other_sessions: false }
      if (profile.hasPassword) body.current_password = current as string
      await this.deps.call('POST', '/v1/me/change_password', body)
      return this.me()
    })
  }

  async link(provider: unknown): Promise<ProfileResult> {
    if (provider !== 'google' && provider !== 'apple' && provider !== 'x') return fail('unavailable', UNAVAILABLE)
    if (!this.deps.socialProviders().includes(provider)) return fail('unavailable', `${NAME[provider]} não está disponível para esta conta.`)
    return this.run(async () => {
      const callback = this.deps.callbackURL()
      const created = await this.deps.call<{ id?: string; verification?: { external_verification_redirect_url?: string | null } | null }>('POST', '/v1/me/external_accounts', { strategy: STRATEGY[provider], redirect_url: callback })
      const redirect = created.verification?.external_verification_redirect_url
      if (!redirect || !created.id) throw new Error('Missing redirect.')
      await this.deps.oauth(redirect, callback)
      const profile = await this.me()
      const account = profile.linked.find(a => a.id === created.id)
      if (!account?.verified) {
        // Tentativa abandonada ou recusada: não deixa uma ligação pela metade.
        if (account) await this.deps.call('DELETE', `/v1/me/external_accounts/${encodeURIComponent(account.id)}`).catch(() => {})
        throw { code: 'app_link_failed' }
      }
      return profile
    })
  }

  async unlink(id: unknown): Promise<ProfileResult> {
    if (typeof id !== 'string' || !/^[A-Za-z0-9_]{1,64}$/.test(id)) return fail('unavailable', UNAVAILABLE)
    return this.run(async () => {
      const profile = await this.me()
      if (!profile.linked.some(a => a.id === id)) throw new Error('Unknown account.')
      if (!canUnlink(profile, id)) throw { code: 'app_last_method' }
      try { await this.deps.call('DELETE', `/v1/me/external_accounts/${encodeURIComponent(id)}`) }
      catch (error) {
        const e = error as { status?: unknown; code?: unknown } | null
        // Recusa do próprio Clerk (que não seja pedir confirmação): explica sem inventar o motivo.
        if (typeof e?.status === 'number' && e.status >= 400 && e.status < 500 && e.code !== 'session_reverification_required') throw { code: 'app_unlink_refused' }
        throw error
      }
      return this.me()
    })
  }

  async startTotp(): Promise<ProfileResult<TotpSetup>> {
    return this.run(async () => {
      const totp = await this.deps.call<{ uri?: unknown; secret?: unknown }>('POST', '/v1/me/totp')
      if (typeof totp.uri !== 'string' || !totp.uri.startsWith('otpauth://totp/') || typeof totp.secret !== 'string' || !/^[A-Z2-7]+=*$/i.test(totp.secret)) throw new Error('Invalid TOTP.')
      this.totpStarted = true
      return { uri: totp.uri, secret: totp.secret }
    })
  }

  /** Só liga com um código que o Clerk aceitou; os códigos de recuperação voltam uma única vez. */
  async confirmTotp(code: unknown): Promise<ProfileResult<RecoveryCodes>> {
    if (typeof code !== 'string' || !/^\d{6}$/.test(code.replace(/\s/g, ''))) return fail('code_invalid', 'Digite os 6 números do app autenticador.')
    if (!this.totpStarted) return fail('unavailable', 'Comece de novo: abra o QR para ligar a verificação.')
    return this.run(async () => {
      const totp = await this.deps.call<{ verified?: unknown; backup_codes?: unknown }>('POST', '/v1/me/totp/attempt_verification', { code: code.replace(/\s/g, '') })
      if (totp.verified !== true) throw { code: 'form_code_incorrect' }
      this.totpStarted = false
      const codes = validCodes(totp.backup_codes) ?? validCodes((await this.deps.call<{ codes?: unknown }>('POST', '/v1/me/backup_codes')).codes)
      if (!codes) throw new Error('Codes missing.')
      return { codes }
    })
  }

  disableTotp() { return this.run(async () => { await this.deps.call('DELETE', '/v1/me/totp'); this.totpStarted = false; return this.me() }) }

  regenerateCodes(): Promise<ProfileResult<RecoveryCodes>> {
    return this.run(async () => {
      const codes = validCodes((await this.deps.call<{ codes?: unknown }>('POST', '/v1/me/backup_codes')).codes)
      if (!codes) throw new Error('Codes missing.')
      return { codes }
    })
  }

  private sessionPath(suffix = '') {
    const id = this.deps.sessionId()
    if (!id) throw new Error('No session.')
    return `/v1/client/sessions/${encodeURIComponent(id)}/verify${suffix}`
  }
  private verified(v: FapiVerification): Reverification {
    const first = v.supported_first_factors?.find(f => f.strategy === 'email_code')
    if (first?.email_address_id) this.emailAddressId = first.email_address_id
    const r = reverification(v)
    if (!r) throw new Error('Invalid verification.')
    return r
  }

  startReverify() { return this.run(async () => this.verified(await this.deps.call<FapiVerification>('POST', this.sessionPath(), { level: 'second_factor' }))) }

  sendReverifyCode() {
    return this.run(async () => {
      if (!this.emailAddressId) throw new Error('No email.')
      return this.verified(await this.deps.call<FapiVerification>('POST', this.sessionPath('/prepare_first_factor'), { strategy: 'email_code', email_address_id: this.emailAddressId }))
    })
  }

  async attemptReverify(attempt: unknown): Promise<ProfileResult<Reverification>> {
    const a = attempt as ReverifyAttempt | null
    if (!a || typeof a !== 'object') return fail('unavailable', UNAVAILABLE)
    let body: Record<string, string>, second = false
    if (a.strategy === 'password' && typeof a.password === 'string' && a.password && a.password.length <= 4096) body = { strategy: 'password', password: a.password }
    else if ((a.strategy === 'email_code' || a.strategy === 'totp') && typeof a.code === 'string' && /^\d{6}$/.test(a.code.trim())) { body = { strategy: a.strategy, code: a.code.trim() }; second = a.strategy === 'totp' }
    else if (a.strategy === 'backup_code' && typeof a.code === 'string' && /^[a-z0-9]{6,16}$/i.test(a.code.trim())) { body = { strategy: 'backup_code', code: a.code.trim() }; second = true }
    else return fail('code_invalid', 'Confira o que digitou.')
    const result = await this.run(async () => this.verified(await this.deps.call<FapiVerification>('POST', this.sessionPath(second ? '/attempt_second_factor' : '/attempt_first_factor'), body)))
    return !result.ok && result.failure === 'password_incorrect' && a.strategy !== 'password' ? fail('code_invalid', 'Código incorreto ou vencido.') : result
  }
}

function validCodes(value: unknown): string[] | null {
  return Array.isArray(value) && value.length > 0 && value.length <= 32 && value.every(c => typeof c === 'string' && /^[a-z0-9]{4,32}$/i.test(c)) ? value as string[] : null
}

/** Conteúdo do .txt de recuperação; só vai para o arquivo escolhido no diálogo. */
export function recoveryFile(codes: unknown): string | null {
  const valid = validCodes(codes)
  if (!valid) return null
  return [traduzir('pt-BR', 'marca.conta.codigosTitulo'), '', 'Cada código entra uma vez, se você perder o app autenticador.', 'Guarde em lugar seguro.', '', ...valid, ''].join('\n')
}

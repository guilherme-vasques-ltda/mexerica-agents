import type { SecondFactorStrategy } from '../../shared/ipc'
import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'

/**
 * Pure rules for the entry's code steps (E6a): what the instance offers for sign-up, which second
 * factors the app can take, and the PT-BR copy for Clerk's refusals. No credential or code passes here.
 */

type Attribute = { enabled?: unknown; required?: unknown; used_for_first_factor?: unknown; verifications?: unknown }

/**
 * Sign-up by e-mail and password only when the public environment says it works without anything this
 * screen doesn't ask (names, legal consent, phone). The minimum length comes from the instance.
 */
export function signUpRules(environment: unknown): { minPasswordLength: number } | null {
  const settings = (environment as { user_settings?: Record<string, unknown> } | null)?.user_settings
  if (!settings || typeof settings !== 'object') return null
  const signUp = settings.sign_up as { mode?: unknown; legal_consent_enabled?: unknown } | undefined
  const attributes = settings.attributes as Record<string, Attribute | undefined> | undefined
  const password = settings.password_settings as { min_length?: unknown } | undefined
  if (signUp?.mode !== 'public' || signUp.legal_consent_enabled === true || !attributes) return null
  const email = attributes.email_address
  if (email?.enabled !== true || email.used_for_first_factor !== true || !Array.isArray(email.verifications) || !email.verifications.includes('email_code')) return null
  if (attributes.password?.enabled !== true) return null
  for (const [name, attribute] of Object.entries(attributes)) {
    if (name !== 'email_address' && name !== 'password' && attribute?.enabled === true && attribute.required === true) return null
  }
  const min = password?.min_length
  if (typeof min !== 'number' || !Number.isInteger(min) || min < 1 || min > 256) return null
  return { minPasswordLength: Math.max(8, min) }
}

/**
 * E7c: the instance's sign-up mode, read from the public environment. Only `public` and `waitlist` are
 * known; anything else (restricted, missing, unreadable) is null and the screen offers neither
 * "Criar conta" nor "Entrar na lista de espera" (fail-closed).
 */
export function signUpMode(environment: unknown): 'public' | 'waitlist' | null {
  const mode = (environment as { user_settings?: { sign_up?: { mode?: unknown } } } | null)?.user_settings?.sign_up?.mode
  return mode === 'public' || mode === 'waitlist' ? mode : null
}

/** E7c: the only page "Entrar na lista de espera" opens. Fixed here in the Main; the renderer never sends a URL. */
export const WAITLIST_URL = 'https://accounts.example.invalid/waitlist'
export const WAITLIST_ORIGIN = 'https://accounts.example.invalid'
export const waitlistCopy = traduzir('pt-BR', 'marca.conta.listaEspera')

/**
 * Clerk's FAPI refusals of a new account while sign-ups are closed (`ERROR_CODES` in @clerk/shared:
 * `sign_up_restricted_waitlist` in waitlist mode, `sign_up_mode_restricted` in restricted mode).
 */
export function waitlistRefusal(code: string | null | undefined): boolean {
  return code === 'sign_up_restricted_waitlist' || code === 'sign_up_mode_restricted'
}

/** Sign-in by e-mail code (first factor, no password): only when the environment lists it. */
export function emailCodeSignIn(environment: unknown): boolean {
  const email = (environment as { user_settings?: { attributes?: { email_address?: { used_for_first_factor?: unknown; first_factors?: unknown } } } } | null)?.user_settings?.attributes?.email_address
  return email?.used_for_first_factor === true && Array.isArray(email.first_factors) && email.first_factors.includes('email_code')
}

/**
 * The `email_address_id` Clerk offers for an `email_code` factor on this attempt: first factor
 * (sign-in without password) or second (Device Trust on a new device, `needs_client_trust`).
 */
export function emailCodeFactor(signIn: unknown, step: 'first' | 'second' = 'first', strategy = 'email_code'): string | null {
  const item = signIn as { supported_first_factors?: unknown; supported_second_factors?: unknown } | null
  const list = step === 'first' ? item?.supported_first_factors : item?.supported_second_factors
  if (!Array.isArray(list)) return null
  const factor = list.find(f => (f as { strategy?: unknown } | null)?.strategy === strategy) as { email_address_id?: unknown } | undefined
  return typeof factor?.email_address_id === 'string' && /^[A-Za-z0-9_-]{1,128}$/.test(factor.email_address_id) ? factor.email_address_id : null
}

/**
 * "Esqueci minha senha" (reset_password_email_code): only with password and e-mail codes on. The new
 * password follows the instance minimum; HIBP and strength stay with the server.
 */
export function passwordResetRules(environment: unknown): { minPasswordLength: number } | null {
  const settings = (environment as { user_settings?: Record<string, unknown> } | null)?.user_settings
  const attributes = settings?.attributes as Record<string, Attribute & { first_factors?: unknown } | undefined> | undefined
  const email = attributes?.email_address
  const min = (settings?.password_settings as { min_length?: unknown } | undefined)?.min_length
  if (attributes?.password?.enabled !== true || email?.enabled !== true || email.used_for_first_factor !== true) return null
  const codes = [email.verifications, email.first_factors].some(list => Array.isArray(list) && list.includes('email_code'))
  if (!codes || typeof min !== 'number' || !Number.isInteger(min) || min < 1 || min > 256) return null
  return { minPasswordLength: Math.max(8, min) }
}

/** The second factors this screen accepts, authenticator first. Others (SMS, e-mail) stay unsupported. */
export function secondFactorStrategies(signIn: unknown): SecondFactorStrategy[] {
  const list = (signIn as { supported_second_factors?: unknown } | null)?.supported_second_factors
  if (!Array.isArray(list)) return []
  const offered = new Set(list.map(f => (f as { strategy?: unknown } | null)?.strategy))
  return (['totp', 'backup_code'] as const).filter(s => offered.has(s))
}

/** Shape only: the server decides. Spaces from copy and paste are dropped. */
export function normalizeCode(strategy: SecondFactorStrategy | 'email_code', code: string): string | null {
  const clean = code.replace(/\s+/g, '')
  if (strategy === 'backup_code') return /^[A-Za-z0-9-]{4,64}$/.test(clean) ? clean : null
  return /^\d{6}$/.test(clean) ? clean : null
}

export const codeCopy = {
  totp: 'Código incorreto ou expirado. Confira o app autenticador.',
  backup_code: 'Código de recuperação incorreto ou já usado. Confira a lista que você guardou.',
  email_code: 'Código incorreto ou expirado. Confira o e-mail mais recente.',
  email_expired: 'O código expirou. Use Reenviar código.',
  tooMany: 'Muitas tentativas. Espere um pouco e tente de novo.',
  expired: 'A verificação expirou. Entre de novo.',
  unsupported: 'Esta conta pede uma verificação que o app ainda não aceita. Você ainda não entrou.',
  offline: 'Não deu para enviar o código agora. Confira a internet e tente de novo.',
}

export function signUpRefusal(codes: string[], minPasswordLength: number): string {
  const code = codes[0]
  if (code === 'form_identifier_exists') return 'Já existe uma conta com este e-mail. Use Entrar.'
  if (code === 'form_password_pwned') return 'Esta senha aparece em vazamentos conhecidos. Escolha outra.'
  if (code === 'form_password_length_too_short') return `A senha precisa ter pelo menos ${minPasswordLength} caracteres.`
  if (code === 'form_password_not_strong_enough' || code === 'form_password_validation_failed') return 'Senha fraca. Use uma senha mais longa e difícil de adivinhar.'
  if (code === 'form_param_format_invalid' || code === 'form_param_nil') return 'Confira o e-mail digitado.'
  return 'Não foi possível criar a conta. Confira os dados e tente de novo.'
}

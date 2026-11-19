import type { SignInProvider } from './analytics'

/**
 * E6b (DON-864): Ajustes → Conta. O que o renderer vê da conta Clerk e os pedidos que pode fazer.
 * Tudo passa pelo processo principal (FAPI nativo); senha, segredo TOTP e códigos de recuperação
 * só atravessam esta ponte no pedido ou na resposta, nunca em estado guardado.
 */
export interface PasswordRules {
  minLength: number
  maxLength: number | null
  requireUppercase: boolean
  requireLowercase: boolean
  requireNumbers: boolean
  requireSpecial: boolean
}

export interface LinkedAccount {
  id: string
  provider: SignInProvider
  email: string | null
  verified: boolean
}

export interface AccountProfile {
  firstName: string
  lastName: string
  email: string | null
  hasPassword: boolean
  totp: boolean
  backupCodes: boolean
  linked: LinkedAccount[]
  /** Ligados na instância e ainda não ligados nesta conta. */
  linkable: SignInProvider[]
  /** Regras de senha da instância; null enquanto o ambiente não foi lido. */
  passwordRules: PasswordRules | null
}

export type ProfileFailure =
  | 'reverify'
  | 'last_method'
  | 'password_incorrect'
  | 'password_rules'
  | 'code_invalid'
  | 'name_invalid'
  | 'link_claimed'
  | 'cancelled'
  | 'unavailable'

export type ProfileResult<T = AccountProfile> =
  | { ok: true; value: T }
  | { ok: false; failure: ProfileFailure; message: string }

export interface TotpSetup {
  /** `otpauth://totp/...` para o QR. */
  uri: string
  /** O mesmo segredo para digitar à mão. */
  secret: string
}

export interface RecoveryCodes { codes: string[] }

/** Confirmação de identidade pedida pelo Clerk antes de uma mudança sensível. */
export interface Reverification {
  status: 'needs_first_factor' | 'needs_second_factor' | 'complete'
  /** Fatores que a tela pode oferecer, na ordem preferida. */
  factors: ('password' | 'email_code' | 'totp' | 'backup_code')[]
}

export type ReverifyAttempt =
  | { strategy: 'password'; password: string }
  | { strategy: 'email_code'; code: string }
  | { strategy: 'totp' | 'backup_code'; code: string }

export interface AccountProfileBridge {
  load(): Promise<ProfileResult>
  saveName(firstName: string, lastName: string): Promise<ProfileResult>
  /** `current` null cria a primeira senha (conta só com Google). */
  setPassword(current: string | null, next: string): Promise<ProfileResult>
  link(provider: SignInProvider): Promise<ProfileResult>
  unlink(id: string): Promise<ProfileResult>
  startTotp(): Promise<ProfileResult<TotpSetup>>
  confirmTotp(code: string): Promise<ProfileResult<RecoveryCodes>>
  disableTotp(): Promise<ProfileResult>
  regenerateCodes(): Promise<ProfileResult<RecoveryCodes>>
  /** Diálogo do sistema; grava só onde a pessoa escolher. */
  saveCodes(codes: string[]): Promise<'saved' | 'cancelled' | 'error'>
  startReverify(): Promise<ProfileResult<Reverification>>
  sendReverifyCode(): Promise<ProfileResult<Reverification>>
  attemptReverify(attempt: ReverifyAttempt): Promise<ProfileResult<Reverification>>
}

/** Confere a senha nova com as regras da instância; null quando serve. Mesma regra no Main e na tela. */
export function passwordProblem(password: string, rules: PasswordRules | null): string | null {
  const min = rules?.minLength ?? 8
  if (password.length < min) return `Use pelo menos ${min} caracteres.`
  if (rules?.maxLength && password.length > rules.maxLength) return `Use no máximo ${rules.maxLength} caracteres.`
  if (password.length > 256) return 'Use no máximo 256 caracteres.'
  if (rules?.requireUppercase && !/[A-Z]/.test(password)) return 'Inclua uma letra maiúscula.'
  if (rules?.requireLowercase && !/[a-z]/.test(password)) return 'Inclua uma letra minúscula.'
  if (rules?.requireNumbers && !/[0-9]/.test(password)) return 'Inclua um número.'
  if (rules?.requireSpecial && !/[^A-Za-z0-9]/.test(password)) return 'Inclua um símbolo.'
  return null
}

/** Sobra outro jeito de entrar depois de desligar `id`? Senha ou outra conta ligada confirmada. */
export function canUnlink(profile: Pick<AccountProfile, 'hasPassword' | 'linked'>, id: string): boolean {
  return profile.hasPassword || profile.linked.some(a => a.id !== id && a.verified)
}

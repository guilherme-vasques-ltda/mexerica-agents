import { KeychainBlocked } from '../fork-keychain/failure'
import { terminalSession } from './reconciliation'
import { EventEmitter } from 'node:events'
import { randomBytes } from 'node:crypto'
import { app, ipcMain, safeStorage, shell, type BrowserWindow, type IpcMainInvokeEvent } from 'electron'
import type { SignInFailure, SignInProvider } from '../../shared/analytics'
import type { AccountState, AccountPendingStep, AccountReconciliationState as ReconciliationState, ResendEmailCodeResult, SecondFactorStrategy } from '../../shared/ipc'
import { forkFeatures } from '../../shared/fork-features'
import { authenticate, AuthenticationEnded } from '../auth'
import { buildConfig, identity } from '../environment'
import { accountConfig, accountBrowserURL } from './config'
import { AccountStorage } from './storage'
import { accountCallback } from './callback'
import { recoveryDiagnostic as diag, recoveryStatus, type RecoveryStage } from './recovery-diagnostic'
import { classifyClient, classifyAttempt } from './recovery-classification'
import { PasswordAttemptStore, type PasswordAttempt } from './password-attempt'
import { classifyPasswordResponse } from './password-flow'
import { registerAccountProfile } from './profile-ipc'
import { codeCopy, emailCodeFactor, emailCodeSignIn, normalizeCode, passwordResetRules, secondFactorStrategies, signUpMode, signUpRefusal, signUpRules, WAITLIST_ORIGIN, WAITLIST_URL, waitlistCopy, waitlistRefusal } from './entry-rules'
import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'

/**
 * The Codync account through Clerk's Frontend API, the way ClerkKit drives it on the native
 * apps: a device token in the `Authorization` header (`_is_native=true`), OAuth in the system
 * browser returning to `com.pokai.Codync://callback`, the token kept encrypted with the OS
 * keychain. An account never grants access to a computer by itself (spec §4.2).
 */

const SCHEME = identity.appId
const SOCIAL_STRATEGIES = { apple: 'oauth_apple', google: 'oauth_google', x: 'oauth_x' } as const
const REDIRECT = `${SCHEME}://callback`
function socialStrategy(provider: unknown): string | null {
  return provider === 'apple' || provider === 'google' || provider === 'x' ? SOCIAL_STRATEGIES[provider] : null
}
/** Providers the Clerk instance has on, from its public environment (`user_settings.social`). */
export function enabledSocialProviders(environment: unknown): SignInProvider[] {
  const social = (environment as { user_settings?: { social?: Record<string, { enabled?: unknown; authenticatable?: unknown } | undefined> } } | null)?.user_settings?.social
  if (!social || typeof social !== 'object') return []
  return (Object.keys(SOCIAL_STRATEGIES) as SignInProvider[]).filter(p => social[SOCIAL_STRATEGIES[p]]?.enabled === true && social[SOCIAL_STRATEGIES[p]]?.authenticatable === true)
}
const ENVIRONMENT_RETRY_MS = 30_000
const EMAIL_CODE_RESEND_MS = 30_000

export const isMainEnvironment = () => identity.releaseUpdates

interface ClerkUser {
  id: string
  image_url?: string
  primary_email_address_id?: string | null
  email_addresses?: { id: string; email_address: string }[]
}

interface ClerkSession {
  id: string
  status: string
  expire_at?: number
  user?: ClerkUser
}

interface ClerkClient {
  sessions?: ClerkSession[]
  last_active_session_id?: string | null
}

interface Verification {
  status?: string
  external_verification_redirect_url?: string | null
  error?: { code?: string; long_message?: string; message?: string } | null
}

interface SignInResource {
  identifier?: string | null
  id: string
  status: string
  created_session_id?: string | null
  first_factor_verification?: Verification | null
  second_factor_verification?: Verification | null
  supported_second_factors?: { strategy?: string }[] | null
}

interface SignUpResource { id?: string; status?: string; created_session_id?: string | null }
type FapiResult<T> = { ok: boolean; status: number; value: T; codes: string[]; credential: string | null }

class InvalidStoredAnchor extends Error {}

/** E7c: opens only the fixed waitlist page in the default browser. Nothing from the renderer chooses it. */
export function openWaitlist(open: (url: string) => Promise<void> = (url) => shell.openExternal(url)): Promise<boolean> {
  const url = new URL(WAITLIST_URL)
  if (url.origin !== WAITLIST_ORIGIN || url.protocol !== 'https:') return Promise.resolve(false)
  return open(url.href).then(() => true, () => false)
}

class ClerkError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly code: string | null = null,
  ) {
    super(message)
  }
}

class InvalidNativeCredential extends Error {}

// Native Authorization is opaque. Accept only values that can be sent as an
// HTTP field and are already normalized when recovered from encrypted storage.
function validNativeCredential(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0 && value === value.trim() &&
    value.toLowerCase() !== 'bearer' && /^[\x09\x20-\x7e\x80-\xff]+$/.test(value)
}

function nativeAuthorization(value: string): string | null {
  if (!/^[\x09\x20-\x7e\x80-\xff]*$/.test(value)) throw new InvalidNativeCredential('Invalid device token.')
  const normalized = value.trim()
  if (!normalized || normalized.toLowerCase() === 'bearer') return null
  if (!validNativeCredential(normalized)) throw new InvalidNativeCredential('Invalid device token.')
  return normalized
}

export interface CloudSeal { readonly epoch: number; readonly sessionId: string; readonly userId: string }
export interface DeviceBinding { version: 1; sessionId: string; userId: string; origin: string; issuer: string; deploymentId: string; manifestHash: string; deviceId: string; deviceKey: string }
interface PendingExit { version: 1; userId: string; sessionId: string; credential: string; binding: DeviceBinding | null; clerkEnded?: boolean; restricted?: { credential: string; sessionId: string; verified: boolean }; restrictedAttempt?: { credential: string; signInId: string; detached?: boolean }; cleanupSent?: boolean }
export type DeviceRevocation = { kind: 'revoked' | 'not_eligible' | 'unknown' }
interface ExitTarget { credential: string; sessionId?: string; attemptId?: string; all?: boolean }
type CapturedRevoker = (jwt: string, current: () => boolean) => Promise<DeviceRevocation>

export class AccountService extends EventEmitter {
  private config = accountConfig(buildConfig)
  browserURL(value: string): string { return accountBrowserURL(value, this.config) }
  private base = forkFeatures.cloudAccess && this.config.kind === 'configured' ? this.config.proxyOrigin : null
  private epoch = 0
  private locallyFenced = false
  private seals = new WeakSet<CloudSeal>()
  private restoring = false
  private restoreEpoch: number | null = null
  private storageQueue: Promise<void> = Promise.resolve()
  private deviceToken: string | null = null
  private client: ClerkClient | null = null
  private cachedToken: { sessionId: string; jwt: string; until: number } | null = null
  private deviceBinding: DeviceBinding | null = null
  private unverifiedBinding: DeviceBinding | null = null
  private unverifiedPrincipal: { userId: string; sessionId: string } | null = null
  private passwordAttempt: PasswordAttempt | null = null
  /** E6a: a sign-in that passed its first factor and waits for TOTP or a recovery code. Memory only. */
  private secondFactor: { via: 'password' | 'social'; attemptId: string; epoch: number; strategies: SecondFactorStrategy[] } | null = null
  /** E6a: an unverified e-mail sign-up. Memory only: no session exists until the code is accepted. */
  private signUpAttempt: { id: string; credential: string; epoch: number; resendAt: number } | null = null
  /** E6a: a sign-in by e-mail code (first factor, no password) waiting for the code. Uses the password journal. */
  private emailSignIn: { attemptId: string; emailAddressId: string; epoch: number; resendAt: number; step: 'first' | 'second' | 'reset' } | null = null
  /** E6a: the reset code was accepted; the sign-in waits for a new password. */
  private newPassword: { attemptId: string; epoch: number } | null = null
  private transient = false
  private localResetPending = false
  private bindingRestorer: ((seal: CloudSeal, binding: DeviceBinding) => Promise<boolean>) | null = null
  private deviceRevoker: ((seal: CloudSeal, binding: DeviceBinding | null) => CapturedRevoker | null) | null = null
  state: AccountState = {
    ready: !this.base, configured: !!this.base, busy: false, error: null, user: null,
    cloudURL: this.base && this.config.kind === 'configured' ? this.config.cloudOrigin : null,
    socialProviders: [], signUp: null, signUpMode: null, waitlist: false, emailCode: false, passwordReset: null, pending: null,
  }
  // Compatibility hooks for callers from older Cloud registration code. D185
  // no longer registers/re-authenticates a device to settle a local sign-out.
  setReconciliationRegistrar(_fn: (userId: string, publicKey: string, jwt: string, origin: string, current: () => boolean) => Promise<string | null>) {}
  setReconciliationKeyReader(_fn: (userId: string) => Promise<string | null>) {}
  setReconciliationKeyVerifier(_fn: (userId: string, publicKey: string) => Promise<boolean>) {}
  setDeviceRevocationVerifier(_fn: (binding: DeviceBinding, jwt: string, current: () => boolean) => Promise<DeviceRevocation>) {}
  setDeviceIdentityRetirer(_fn: (binding: DeviceBinding, current: () => boolean) => Promise<boolean>) {}
  private identityResetter: ((binding: DeviceBinding, current: () => boolean) => Promise<void>) | null = null
  private identityResetTarget: DeviceBinding | null = null
  setDeviceIdentityResetter(reset: (binding: DeviceBinding, current: () => boolean) => Promise<void>) { this.identityResetter = reset }
  setDeviceRevoker(capture: (seal: CloudSeal, binding: DeviceBinding | null) => CapturedRevoker | null) { this.deviceRevoker = capture }
  setBindingRestorer(restore: (seal: CloudSeal, binding: DeviceBinding) => Promise<boolean>) { this.bindingRestorer = restore }
  get sessionEpoch() { return this.epoch }
  private advanceEpoch() { this.epoch++; this.emit('epoch', this.epoch); return this.epoch }
  private fencePresentation() {
    this.locallyFenced = true; this.cachedToken = null
    this.state = { ...this.state, user: null }
    return this.advanceEpoch()
  }
  reconciliationState(): ReconciliationState { return { kind: this.transient || this.localResetPending ? 'session_unverified' : 'none' } }
  deviceIdentityScope() {
    const c = this.config
    return c.kind === 'configured' ? { origin: c.cloudOrigin, issuer: c.issuer, deploymentId: c.deploymentId, manifestHash: c.manifestHash } : null
  }
  private get storage() { return new AccountStorage(app.getPath('userData'), this.namespace, sealed => {
    if (!this.encryptionAvailable()) throw new KeychainBlocked('unavailable')
    this.validateStoredPlaintext(this.recoveryDecrypt(sealed))
  }) }
  private get passwordStore() { return new PasswordAttemptStore(app.getPath('userData'), this.namespace) }
  private get namespace() {
    const c = this.config
    return c.kind === 'configured' ? JSON.stringify([c.deploymentId, c.issuer, c.cloudOrigin, c.frontendOrigin, c.proxyOrigin, c.manifestHash]) : 'unavailable'
  }
  private current(epoch: number) { if (epoch !== this.epoch) throw new Error('Account operation superseded.') }
  private encryptionAvailable() { return safeStorage.isEncryptionAvailable() && !(process.platform === 'linux' && safeStorage.getSelectedStorageBackend() === 'basic_text') }
  private get activeSession(): ClerkSession | null {
    const matches = this.client?.sessions?.filter(s => s.id === this.client?.last_active_session_id) ?? []
    return matches.length === 1 && matches[0].status === 'active' && typeof matches[0].user?.id === 'string' && !!matches[0].user.id ? matches[0] : null
  }
  private set(change: Partial<AccountState>) {
    const next = { ...this.state, ...change }
    const user = this.activeSession?.user
    const confirmed = next.ready && !next.busy && !this.restoring && !this.locallyFenced && !this.transient && !this.localResetPending && !!this.deviceToken && !this.secondFactor && !this.signUpAttempt && !this.emailSignIn && !this.newPassword
    const email = user?.email_addresses?.find(e => e.id === user.primary_email_address_id)?.email_address ?? user?.email_addresses?.[0]?.email_address ?? null
    const pending: AccountPendingStep | null = this.secondFactor?.epoch === this.epoch ? { kind: 'second_factor', strategies: [...this.secondFactor.strategies] }
      : this.signUpAttempt?.epoch === this.epoch ? { kind: 'email_code', purpose: 'sign_up', resendAt: this.signUpAttempt.resendAt }
      : this.emailSignIn?.epoch === this.epoch ? { kind: 'email_code', purpose: this.emailSignIn.step === 'first' ? 'sign_in' : this.emailSignIn.step === 'reset' ? 'reset_password' : 'device_trust', resendAt: this.emailSignIn.resendAt }
      : this.newPassword?.epoch === this.epoch && this.state.passwordReset ? { kind: 'new_password', minPasswordLength: this.state.passwordReset.minPasswordLength } : null
    this.state = { ...next, pending, waitlist: next.error === waitlistCopy, user: confirmed && user ? { userId: user.id, email, avatarURL: user.image_url?.startsWith('https://') ? user.image_url : null } : null }
    this.emit('change')
  }
  captureCloudSession(): CloudSeal | null { return this.restoring || !this.state.ready || this.state.busy ? null : this.issueCloudSession() }
  private issueCloudSession(): CloudSeal | null {
    const session = this.activeSession
    if (!this.base || this.locallyFenced || this.transient || this.localResetPending || !session?.user || !this.deviceToken) return null
    const seal = Object.freeze({ epoch: this.epoch, sessionId: session.id, userId: session.user.id })
    this.seals.add(seal); return seal
  }
  isCloudSessionCurrent(seal: CloudSeal): boolean {
    return this.seals.has(seal) && !this.locallyFenced && !this.transient && !this.localResetPending && seal.epoch === this.epoch && this.activeSession?.id === seal.sessionId && this.activeSession.user?.id === seal.userId
  }
  async tokenForCloud(seal: CloudSeal): Promise<string> {
    if (!this.isCloudSessionCurrent(seal)) throw new Error('Session superseded.')
    const token = await this.sessionToken()
    if (!this.isCloudSessionCurrent(seal)) throw new Error('Session superseded.')
    return token
  }
  verifiedDeviceBinding(seal: CloudSeal): DeviceBinding | null {
    return this.isCloudSessionCurrent(seal) && this.deviceBinding && this.bindingMatches(this.deviceBinding, seal) ? this.deviceBinding : null
  }
  private bindingMatches(binding: DeviceBinding, seal: CloudSeal): boolean {
    const c = this.config
    return c.kind === 'configured' && binding.version === 1 && binding.sessionId === seal.sessionId && binding.userId === seal.userId &&
      binding.origin === c.cloudOrigin && binding.issuer === c.issuer && binding.deploymentId === c.deploymentId && binding.manifestHash === c.manifestHash &&
      /^dev_[A-Za-z0-9_-]+$/.test(binding.deviceId) && /^[A-Za-z0-9_-]{43}$/.test(binding.deviceKey)
  }
  async saveDeviceBinding(seal: CloudSeal, deviceId: string, deviceKey: string): Promise<DeviceBinding | null> {
    if (!this.isCloudSessionCurrent(seal) || !this.deviceToken || this.config.kind !== 'configured') return null
    const c = this.config
    const binding: DeviceBinding = { version: 1, sessionId: seal.sessionId, userId: seal.userId, origin: c.cloudOrigin, issuer: c.issuer, deploymentId: c.deploymentId, manifestHash: c.manifestHash, deviceId, deviceKey }
    if (!this.bindingMatches(binding, seal)) return null
    const token = this.deviceToken
    const work = this.storageQueue.catch(() => {}).then(async () => {
      if (!this.isCloudSessionCurrent(seal) || this.deviceToken !== token || !this.encryptionAvailable()) throw new Error('Session superseded.')
      const sealed = safeStorage.encryptString(JSON.stringify({ version: 1, token, binding, principal: { userId: seal.userId, sessionId: seal.sessionId } }))
      const current = () => { if (!this.isCloudSessionCurrent(seal) || this.deviceToken !== token) throw new Error('Session superseded.') }
      await this.storage.retain(sealed, 'active', current)
    })
    this.storageQueue = work
    try { await work } catch (error) { if (seal.epoch === this.epoch) await this.failStorage(); throw error }
    if (!this.isCloudSessionCurrent(seal) || this.deviceToken !== token) return null
    this.deviceBinding = binding
    return binding
  }

  private passwordSender(event: IpcMainInvokeEvent, windows: BrowserWindow[]): boolean {
    const frame = event.senderFrame
    if (!frame || !windows.some(w => !w.isDestroyed() && w.webContents === event.sender && w.webContents.mainFrame === frame) || event.sender.isDestroyed()) return false
    try {
      const current = new URL(frame.url)
      const dev = !app.isPackaged && process.env.ELECTRON_RENDERER_URL
      const expected = dev ? new URL(`${dev}/?window=chat`) : new URL(`../renderer/index.html?window=chat`, `file://${__dirname}/`)
      if (dev && !(['http:', 'https:'].includes(expected.protocol) && ['localhost', '127.0.0.1', '[::1]'].includes(expected.hostname))) return false
      if (!dev && (expected.protocol !== 'file:' || expected.host)) return false
      current.hash = ''; expected.hash = ''
      return current.href === expected.href
    } catch { return false }
  }

  register(windows: () => BrowserWindow[]) {
    ipcMain.handle('account:state', () => this.state)
    ipcMain.handle('account:signIn', (_e, provider: SignInProvider) => this.signIn(provider))
    ipcMain.handle('account:signInWithPassword', (event, ...args: unknown[]) => this.passwordSender(event, windows()) && args.length === 2 && typeof args[0] === 'string' && typeof args[1] === 'string' ? this.signInWithPassword(args[0], args[1]) : 'error')
    ipcMain.handle('account:reconcileSignInWithPassword', (event, ...args: unknown[]) => this.passwordSender(event, windows()) && args.length === 2 && typeof args[0] === 'string' && typeof args[1] === 'string' ? this.reconcileSignInWithPassword(args[0], args[1]) : { kind: 'unavailable' })
    ipcMain.handle('account:attemptSecondFactor', (event, ...args: unknown[]) => this.passwordSender(event, windows()) && args.length === 2 && typeof args[1] === 'string' ? this.attemptSecondFactor(args[0], args[1]) : 'error')
    ipcMain.handle('account:signUpWithPassword', (event, ...args: unknown[]) => this.passwordSender(event, windows()) && args.length === 2 && typeof args[0] === 'string' && typeof args[1] === 'string' ? this.signUpWithPassword(args[0], args[1]) : 'error')
    ipcMain.handle('account:signInWithEmailCode', (event, ...args: unknown[]) => this.passwordSender(event, windows()) && args.length === 1 && typeof args[0] === 'string' ? this.signInWithEmailCode(args[0]) : 'error')
    ipcMain.handle('account:verifyEmailCode', (event, ...args: unknown[]) => this.passwordSender(event, windows()) && args.length === 1 && typeof args[0] === 'string' ? this.verifyEmailCode(args[0]) : 'error')
    ipcMain.handle('account:forgotPassword', (event, ...args: unknown[]) => this.passwordSender(event, windows()) && args.length === 1 && typeof args[0] === 'string' ? this.forgotPassword(args[0]) : 'error')
    ipcMain.handle('account:resetPassword', (event, ...args: unknown[]) => this.passwordSender(event, windows()) && args.length === 1 && typeof args[0] === 'string' ? this.resetPassword(args[0]) : 'error')
    ipcMain.handle('account:resendEmailCode', (event): Promise<ResendEmailCodeResult> => this.passwordSender(event, windows()) ? this.resendEmailCode() : Promise.resolve({ kind: 'error', code: 'refused' }))
    ipcMain.handle('account:cancelPending', (event) => this.passwordSender(event, windows()) ? this.cancelPending() : undefined)
    ipcMain.handle('account:openWaitlist', (event) => this.passwordSender(event, windows()) ? openWaitlist() : false)
    ipcMain.handle('account:signOut', () => this.signOut())
    ipcMain.handle('account:reconciliationState', () => this.reconciliationState())
    ipcMain.handle('account:reconcile', (_e, action: unknown) => this.reconcile(action))
    ipcMain.handle('account:reconcileSignIn', (_e, provider: unknown) => this.reconcileSignIn(provider))
    this.on('change', () => {
      for (const w of windows()) w.webContents.send('account:change', this.state)
    })
    registerAccountProfile(this, windows) // E6b: Ajustes → Conta (profile.ts).
    void this.restore()
    void this.loadSocialProviders()
  }

  /** E6b (DON-864): o ambiente público lido por último (regras de senha em Ajustes → Conta). */
  environment: unknown = null
  isAccountWindow(event: IpcMainInvokeEvent, windows: BrowserWindow[]) { return this.passwordSender(event, windows) }
  get profileSessionId() { return this.state.user ? this.activeSession?.id ?? null : null }
  /** E6b: só `/v1/me` e a reverificação desta sessão, com a conta confirmada e sem outra ação em curso. */
  async profileCall<T>(method: 'GET' | 'POST' | 'PATCH' | 'DELETE', path: string, body?: Record<string, string | boolean>): Promise<T> {
    const session = this.state.user ? this.activeSession : null
    if (!session || this.state.busy || this.restoring || !(/^\/v1\/me(\/|$)/.test(path) || path.startsWith(`/v1/client/sessions/${encodeURIComponent(session.id)}/verify`))) throw new Error('Sign in again to reach your account.')
    const query: Record<string, string> = { _clerk_session_id: session.id }
    if (method === 'PATCH') query._method = 'PATCH'
    const value = await this.request<T>(method === 'PATCH' ? 'POST' : method, path, body, query, this.epoch)
    if (this.activeSession?.id !== session.id) throw new Error('Session superseded.')
    return value
  }

  /**
   * Fail-closed: until the instance's public environment is read, no social sign-in is offered
   * or accepted (e-mail and password are unaffected). A failed read retries later.
   */
  async loadSocialProviders(): Promise<void> {
    const base = this.base
    if (!base) return
    try {
      const url = new URL('/v1/environment', base)
      if (url.origin !== base) throw new Error('Invalid account origin.')
      url.searchParams.set('_is_native', 'true')
      const res = await fetch(url, { method: 'GET', headers: { 'x-device-type': process.platform === 'darwin' ? 'macos' : process.platform, 'x-app-version': app.getVersion(), 'x-bundle-id': identity.appId }, redirect: 'error', credentials: 'omit', signal: AbortSignal.timeout(8_000) })
      if (!res.ok) throw new Error('Environment unavailable.')
      const json = await res.json() as { response?: unknown }
      this.environment = json.response ?? json
      this.set({ socialProviders: enabledSocialProviders(json.response ?? json), signUp: signUpRules(json.response ?? json), signUpMode: signUpMode(json.response ?? json), emailCode: emailCodeSignIn(json.response ?? json), passwordReset: passwordResetRules(json.response ?? json) })
    } catch {
      setTimeout(() => void this.loadSocialProviders(), ENVIRONMENT_RETRY_MS).unref?.()
    }
  }

  private recoveryCallback(returned: string, expected: string): URL {
    diag('callback_validate', 'begin')
    try { const callback = accountCallback(returned, expected); diag('callback_validate', 'succeeded'); return callback }
    catch (error) { diag('callback_validate', 'invalid'); throw error }
  }

  private recoveryEncrypt(value: string): Buffer {
    diag('secure_encrypt', 'begin')
    try { const encrypted = safeStorage.encryptString(value); diag('secure_encrypt', 'succeeded'); return encrypted }
    catch (error) { diag('secure_encrypt', 'failed'); throw error }
  }

  private recoveryDecrypt(sealed: Buffer): string {
    diag('account_decrypt', 'begin')
    try { const value = safeStorage.decryptString(sealed); diag('account_decrypt', 'succeeded'); return value }
    catch (error) { diag('account_decrypt', 'failed'); throw error }
  }

  /** Validate before destructive cleanup too: successful CBC decryption is not integrity. */
  private validateStoredPlaintext(plaintext: string): void {
    let value: { version?: unknown; token?: unknown; credential?: unknown; binding?: DeviceBinding; principal?: { userId?: unknown; sessionId?: unknown } }
    try { value = JSON.parse(plaintext) }
    catch {
      if (!/^[\s]*[\[{]/.test(plaintext) && validNativeCredential(plaintext)) return // legacy opaque credential
      throw new InvalidStoredAnchor()
    }
    if (!value || typeof value !== 'object' || Array.isArray(value) || (value.version !== 1 && value.version !== 2 && value.version !== 3)) throw new InvalidStoredAnchor()
    if ('token' in value ? value.token !== null && !validNativeCredential(value.token) : !validNativeCredential(value.credential)) throw new InvalidStoredAnchor()
    const p = value.principal, b = value.binding
    if (p && (typeof p !== 'object' || typeof p.userId !== 'string' || !p.userId || typeof p.sessionId !== 'string' || !p.sessionId)) throw new InvalidStoredAnchor()
    if (b && !this.bindingMatches(b, { epoch: this.epoch, userId: b.userId, sessionId: b.sessionId })) throw new InvalidStoredAnchor()
    if (b && p && (b.userId !== p.userId || b.sessionId !== p.sessionId)) throw new InvalidStoredAnchor()
  }

  private async persist(token: string | null, epoch: number, _unknown = false, binding = this.deviceBinding ?? this.unverifiedBinding): Promise<void> {
    const work = this.storageQueue.catch(() => {}).then(async () => {
      this.current(epoch)
      if (!token) { await this.storage.clear(); this.current(epoch); return }
      if (!this.encryptionAvailable()) throw new KeychainBlocked('unavailable')
      const session = this.activeSession
      const principal = session?.user ? { userId: session.user.id, sessionId: session.id } : this.unverifiedPrincipal
      const data = this.recoveryEncrypt(JSON.stringify({ version: 1, token, binding, principal }))
      await this.storage.retain(data, 'active', () => this.current(epoch)); this.current(epoch)
    })
    this.storageQueue = work; return work
  }
  private async savePasswordAttempt(record: PasswordAttempt, epoch: number) {
    const work = this.storageQueue.catch(() => {}).then(async () => {
      this.current(epoch); await this.passwordStore.save(record, () => this.current(epoch)); this.current(epoch); this.passwordAttempt = record
    })
    this.storageQueue = work; await work
  }
  private async clearPasswordAttempt(record: PasswordAttempt, epoch: number) {
    const work = this.storageQueue.catch(() => {}).then(async () => {
      this.current(epoch)
      if (this.passwordAttempt !== record) throw new Error('Attempt superseded.')
      await this.passwordStore.clear(() => this.current(epoch)); this.current(epoch); this.passwordAttempt = null
    })
    this.storageQueue = work; await work
  }
  private retryLater(cofre?: 'unavailable' | 'mismatch' | 'unsupported') {
    this.locallyFenced = true; this.transient = true; this.cachedToken = null
    this.set({ user: null, ready: true, busy: false, error: cofre ? `keychain.${cofre}` : 'Não foi possível verificar sua sessão agora' })
  }
  private targets(credential = this.deviceToken): ExitTarget[] {
    const who = this.activeSession ?? this.unverifiedPrincipal ?? this.unverifiedBinding
    const targets: ExitTarget[] = []
    if (credential && validNativeCredential(credential)) targets.push({ credential, sessionId: who ? ('sessionId' in who ? who.sessionId : who.id) : undefined })
    const record = this.passwordAttempt
    if (record?.credential) targets.push({ credential: record.credential, attemptId: record.id ?? undefined })
    return targets
  }
  /** Captured transport only. Never calls request/persist/applyClient or retries. */
  private backgroundExit(targets: ExitTarget[], revoke: CapturedRevoker | null = null, cached: string | null = null) {
    const base = this.base
    if (!base) return
    void (async () => {
      const seen = new Set<string>()
      const references = new Set<string>()
      for (const target of targets.slice(0, 256)) {
        const reference = JSON.stringify(target)
        if (references.has(reference)) continue
        references.add(reference)
        if (!validNativeCredential(target.credential)) continue
        let credential = target.credential
        const call = async (path: string, method: 'GET' | 'POST' | 'DELETE') => {
          const url = new URL(path, base); url.searchParams.set('_is_native', 'true')
          const response = await fetch(url, { method, headers: { Authorization: credential }, redirect: 'error', credentials: 'omit', signal: AbortSignal.timeout(8_000) })
          const raw = response.headers?.get('authorization')
          if (raw != null) credential = nativeAuthorization(raw) ?? credential
          if (!response.ok) throw new Error('Remote exit unavailable.')
          return response
        }
        try {
          let sessionId = target.sessionId
          if (!sessionId && target.attemptId) {
            const body = await (await call(`/v1/client/sign_ins/${encodeURIComponent(target.attemptId)}`, 'GET')).json() as { response?: SignInResource }
            if (body.response?.id !== target.attemptId) throw new Error('Attempt unavailable.')
            sessionId = body.response.created_session_id ?? undefined
            if (!sessionId) continue
          }
          // An old opaque client without an anchor: only close its selected session.
          if (!sessionId && !target.all) {
            const body = await (await call('/v1/client', 'GET')).json() as { response?: ClerkClient }
            const client = body.response
            const matches = client?.sessions?.filter(s => s.id === client.last_active_session_id) ?? []
            sessionId = matches.length === 1 ? matches[0].id : undefined
            if (!sessionId) continue
          }
          const key = JSON.stringify([credential, target.all ? 'all' : sessionId])
          if (seen.has(key)) continue
          seen.add(key)
          // Cloud revocation is independent of Clerk and cannot block the local exit.
          if (revoke && sessionId) {
            const captured = revoke; revoke = null
            try {
              let jwt = cached
              if (!jwt) {
                const body = await (await call(`/v1/client/sessions/${encodeURIComponent(sessionId)}/tokens`, 'POST')).json() as { response?: { jwt?: string }; jwt?: string }
                jwt = body.response?.jwt ?? body.jwt ?? null
              }
              if (!jwt || (await captured(jwt, () => true)).kind === 'unknown') diag('background_exit', 'failed')
            } catch { diag('background_exit', 'failed') }
          }
          await call(target.all ? '/v1/client/sessions' : `/v1/client/sessions/${encodeURIComponent(sessionId!)}/remove`, target.all ? 'DELETE' : 'POST')
          diag('background_exit', 'succeeded')
        } catch { diag('background_exit', 'failed') }
      }
    })().catch(() => diag('background_exit', 'failed'))
  }
  private legacyTargets(value: unknown, depth = 0): ExitTarget[] {
    if (depth > 3 || !value || typeof value !== 'object') return []
    const v = value as Partial<PendingExit> & { pending?: PendingExit; orphans?: PendingExit[]; orphan?: PendingExit; token?: unknown; principal?: { sessionId?: string }; socialAttemptId?: string }
    const result: ExitTarget[] = []
    if (validNativeCredential(v.token)) result.push({ credential: v.token, sessionId: v.principal?.sessionId ?? v.binding?.sessionId, attemptId: v.socialAttemptId })
    if (validNativeCredential(v.credential)) result.push({ credential: v.credential, sessionId: v.sessionId })
    if (v.restricted && validNativeCredential(v.restricted.credential)) result.push({ credential: v.restricted.credential, sessionId: v.restricted.sessionId })
    if (v.restrictedAttempt && validNativeCredential(v.restrictedAttempt.credential)) result.push({ credential: v.restrictedAttempt.credential, attemptId: v.restrictedAttempt.signInId })
    if (v.pending) result.push(...this.legacyTargets(v.pending, depth + 1))
    for (const item of (Array.isArray(v.orphans) ? v.orphans.slice(0, 128) : v.orphan ? [v.orphan] : [])) result.push(...this.legacyTargets(item, depth + 1))
    return result
  }
  /** Epoch changes before the first await; queued old writes cannot survive the clear. */
  private async resetLocal(message: string | null, all = false, explicitExit = false): Promise<void> {
    const seal = this.captureCloudSession()
    const binding = this.identityResetTarget ?? this.deviceBinding ?? this.unverifiedBinding
    let storageReadable = false
    let identityResetSucceeded = !this.identityResetter
    let revoke: CapturedRevoker | null = null
    try { revoke = seal ? this.deviceRevoker?.(seal, binding) ?? null : null } catch { diag('background_exit', 'failed') }
    const cached = this.cachedToken?.jwt ?? null
    const targets = this.targets().map(t => ({ ...t, all }))
    const epoch = this.fencePresentation()
    if (binding && this.identityResetter) this.identityResetTarget = Object.freeze({ ...binding })
    this.localResetPending = true; this.transient = false
    this.restoring = false; this.restoreEpoch = null
    this.client = null; this.deviceToken = null; this.cachedToken = null; this.passwordAttempt = null
    this.secondFactor = null; this.signUpAttempt = null; this.emailSignIn = null; this.newPassword = null
    this.deviceBinding = null; this.unverifiedBinding = null; this.unverifiedPrincipal = null
    this.set({ user: null, busy: false, ready: true, error: message })
    const work = this.storageQueue.catch(() => {}).then(async () => {
      this.current(epoch)
      // Recover targets from disk if sign-out happened before restore's first read.
      // Local failure never delays cleanup with network I/O.
      try {
        const saved = await this.storage.load(); this.current(epoch)
        if (saved.kind !== 'empty' && saved.sealed) {
          if (!this.encryptionAvailable()) throw new KeychainBlocked('unavailable')
          const plain = this.recoveryDecrypt(saved.sealed)
          this.validateStoredPlaintext(plain)
          try {
            const decoded = JSON.parse(plain) as { binding?: DeviceBinding; pending?: { binding?: DeviceBinding } }
            const candidate = decoded.binding ?? decoded.pending?.binding
            if (!this.identityResetTarget && this.identityResetter && candidate && typeof candidate.userId === 'string' && !!candidate.userId && typeof candidate.sessionId === 'string' && !!candidate.sessionId && this.bindingMatches(candidate, { epoch, userId: candidate.userId, sessionId: candidate.sessionId })) this.identityResetTarget = Object.freeze({ ...candidate })
            targets.push(...this.legacyTargets(decoded).map(t => ({ ...t, all })))
          }
          catch { if (validNativeCredential(plain)) targets.push({ credential: plain, all }) }
        }
      } catch { diag('account_load', 'failed'); throw new KeychainBlocked('unavailable') }
      try {
        const attempt = await (explicitExit ? this.passwordStore.loadForExit() : this.passwordStore.load()); this.current(epoch)
        if (attempt?.credential) targets.push({ credential: attempt.credential, attemptId: attempt.id ?? undefined, all })
      } catch { diag('password_load', 'failed'); throw new KeychainBlocked('unavailable') }
      storageReadable = true
      // Identity cleanup is allowed only after both stores were readable.
      const resetTarget = this.identityResetTarget
      const resetIdentity = async () => {
        if (!resetTarget || !this.identityResetter) return
        await this.identityResetter(resetTarget, () => epoch === this.epoch)
        this.current(epoch)
        if (this.identityResetTarget === resetTarget) this.identityResetTarget = null
        identityResetSucceeded = true
      }
      // Local identity I/O joins the existing cleanup barrier. Remote completion
      // never invokes this callback or touches a later generation.
      const results = await Promise.allSettled([this.storage.clear('blocked', explicitExit), this.passwordStore.clear(() => this.current(epoch), explicitExit)])
      this.current(epoch)
      if (results.some(r => r.status === 'rejected')) throw new Error('Local cleanup failed.')
      await resetIdentity()
    })
    this.storageQueue = work
    try { await work; this.current(epoch); this.localResetPending = false; this.set({ error: message }) }
    catch { if (epoch === this.epoch) { this.localResetPending = true; this.set({ error: 'Não foi possível limpar a sessão local. Tente de novo ou entre novamente.' }) } }
    finally { if (storageReadable) this.backgroundExit(targets, identityResetSucceeded ? revoke : null, cached) }
  }
  private async expire() { await this.resetLocal('Sua sessão expirou. Entre novamente.') }
  async signOut() { await this.resetLocal(null, false, true) }
  async signOutAll() { await this.resetLocal(null, true, true) }
  async reconcile(action: unknown): Promise<ReconciliationState> {
    diag('check_entry', 'entered')
    if (action !== 'check' || this.state.busy || this.restoring || !this.state.ready) return { kind: 'unavailable' }
    if (this.localResetPending) { await this.resetLocal(null); return this.reconciliationState() }
    if (!this.transient) return this.reconciliationState()
    await this.restore()
    return this.reconciliationState()
  }
  async reconcileSignIn(_provider: unknown): Promise<ReconciliationState> { return { kind: 'unavailable' } }
  async reconcileSignInWithPassword(_identifier: string, _password: string): Promise<ReconciliationState> { return { kind: 'unavailable' } }
  private async restore() {
    if (!this.base || this.restoring || this.localResetPending) return
    const epoch = this.epoch
    this.restoring = true; this.restoreEpoch = epoch; this.locallyFenced = true
    this.set({ ready: false, busy: false, error: null }); diag('restore', 'begin')
    try {
      await this.storageQueue.catch(() => {}); this.current(epoch)
      const saved = await this.storage.load(); this.current(epoch)
      let attempt: PasswordAttempt | null
      try { attempt = await this.passwordStore.load(); this.current(epoch) }
      catch (error) { this.current(epoch); this.retryLater(error instanceof KeychainBlocked ? error.reason : undefined); return }
      diag('account_load', saved.kind)
      if (saved.kind === 'empty') {
        // A journal alone never silently publishes an old attempt after restart.
        if (attempt) { this.passwordAttempt = attempt; this.retryLater(); return }
        this.deviceToken = null; this.client = null; this.transient = false; this.locallyFenced = false; return
      }
      let available = false
      try { available = this.encryptionAvailable() } catch { diag('secure_availability', 'failed') }
      if (!available) { this.retryLater('unavailable'); return }
      if (!saved.sealed) { this.retryLater(); return }
      let plaintext: string
      try { plaintext = this.recoveryDecrypt(saved.sealed); this.validateStoredPlaintext(plaintext) } catch { this.retryLater('mismatch'); return }
      if (saved.kind !== 'active' || saved.exitRequested) { this.retryLater(); return }
      let token = plaintext
      let binding: DeviceBinding | null = null
      let principal: { userId: string; sessionId: string } | null = null
      try {
        const v = JSON.parse(plaintext) as { version?: number; token?: unknown; binding?: DeviceBinding; principal?: { userId: string; sessionId: string }; socialRetry?: boolean }
        if (![1, 2, 3].includes(v.version ?? 0)) throw new InvalidStoredAnchor()
        // Old confirmations alone are bookkeeping, never a pending login (F9).
        if (v.token === null && !v.binding && !v.principal) {
          await this.resetLocal(null); return
        }
        if (v.socialRetry || !validNativeCredential(v.token)) throw new InvalidStoredAnchor()
        token = v.token
        binding = v.binding ?? null; principal = v.principal ?? null
        if (principal && (typeof principal !== 'object' || typeof principal.userId !== 'string' || !principal.userId || typeof principal.sessionId !== 'string' || !principal.sessionId)) throw new InvalidStoredAnchor()
        if (binding && !this.bindingMatches(binding, { epoch, sessionId: binding.sessionId, userId: binding.userId })) throw new InvalidStoredAnchor()
        if (binding && principal && (binding.sessionId !== principal.sessionId || binding.userId !== principal.userId)) throw new InvalidStoredAnchor()
      } catch (error) { if (error instanceof InvalidStoredAnchor || /^[\s]*[\[{]/.test(plaintext)) { this.retryLater(); return } }
      if (!validNativeCredential(token)) { this.retryLater(); return }
      this.deviceToken = token; this.unverifiedBinding = binding; this.unverifiedPrincipal = principal
      await this.refreshClient(epoch, undefined, 'restore_client_get'); this.current(epoch)
      const session = this.activeSession
      if (!this.deviceToken || !session?.user || (principal && (principal.sessionId !== session.id || principal.userId !== session.user.id)) || (binding && !this.bindingMatches(binding, { epoch, sessionId: session.id, userId: session.user.id }))) {
        const expected = principal?.sessionId ?? binding?.sessionId
        if (expected && terminalSession(this.client, expected)) await this.expire()
        else this.retryLater()
        return
      }
      this.transient = false; this.locallyFenced = false
      if (binding) {
        const seal = this.issueCloudSession()
        if (seal && await this.bindingRestorer?.(seal, binding)) { this.current(epoch); this.deviceBinding = binding }
      }
      this.current(epoch)
      // A journal after account publication may refer to this very session (F10).
      // Drop it locally; verification never performs remote journal cleanup.
      const clear = this.storageQueue.catch(() => {}).then(async () => { this.current(epoch); await this.passwordStore.clear(() => this.current(epoch)) })
      this.storageQueue = clear; await clear; this.current(epoch)
      this.passwordAttempt = null
      await this.persist(this.deviceToken, epoch, false, binding); this.current(epoch)
      this.set({ error: null })
    } catch (error) {
      if (epoch !== this.epoch) return
      // Missing/invalid/denied responses do not prove revocation of this stored session.
      this.retryLater()
    } finally {
      if (this.restoreEpoch === epoch) { this.restoring = false; this.restoreEpoch = null; this.set({ ready: true, busy: false }) }
    }
  }
  private async failStorage() { this.fencePresentation(); this.retryLater() }
  private async request<T>(method: 'GET' | 'POST' | 'DELETE', path: string, body?: Record<string, string | boolean>, query?: Record<string, string>, epoch = this.epoch, apply = true, diagnosticStage?: RecoveryStage): Promise<T> {
    this.current(epoch)
    if (!this.base) throw new ClerkError(0, "Sign-in isn't configured in this build.")
    const url = new URL(`${this.base}${path}`)
    if (url.origin !== this.base) throw new Error('Invalid account origin.')
    url.searchParams.set('_is_native', 'true')
    for (const [k, v] of Object.entries(query ?? {})) url.searchParams.set(k, v)
    const headers: Record<string, string> = { 'x-device-type': process.platform === 'darwin' ? 'macos' : process.platform, 'x-app-version': app.getVersion(), 'x-bundle-id': identity.appId }
    // Clerk derives azp from Origin. Its explicit build pin is distinct from the FAPI;
    // never weaken the Worker's authorizedParties check for native tokens.
    if (method === 'POST' && /^\/v1\/client\/sessions\/[^/]+\/tokens$/.test(path)) {
      if (this.config.kind !== 'configured') throw new Error('Invalid account configuration.')
      headers.Origin = this.config.tokenOrigin
    }
    if (this.deviceToken) headers.Authorization = this.deviceToken
    let payload: string | undefined
    if (body) {
      headers['Content-Type'] = 'application/x-www-form-urlencoded'
      payload = new URLSearchParams(Object.entries(body).map(([k, v]) => [k, String(v)])).toString()
    }
    if (diagnosticStage) diag(diagnosticStage, 'begin')
    let res: Response
    try { res = await fetch(url, { method, headers, body: payload, redirect: 'error', credentials: 'omit', signal: AbortSignal.timeout(20_000) }) }
    catch (error) { if (diagnosticStage) diag(diagnosticStage, 'transport_failed'); throw error }
    if (diagnosticStage) diag(diagnosticStage, res.ok ? 'response' : 'http_refused', recoveryStatus(res))
    if (epoch !== this.epoch) {
      try { const credential = nativeAuthorization(res.headers.get('authorization') ?? headers.Authorization ?? ''); if (credential) this.backgroundExit([{ credential }]) } catch {}
      this.current(epoch)
    }
    const auth = res.headers.get('authorization') ?? undefined
    if (auth !== undefined) {
      let token: string | null
      try { token = nativeAuthorization(auth) } catch (error) { if (diagnosticStage) diag(diagnosticStage, 'credential_format'); throw error }
      if (diagnosticStage && token === null) diag(diagnosticStage, 'credential_cleared')
      try { await this.persist(token, epoch) } catch (error) {
        if (epoch === this.epoch) await this.failStorage()
        throw error
      }
      this.current(epoch)
      this.deviceToken = token
    }
    const json = (await res.json().catch(() => { if (diagnosticStage) diag(diagnosticStage, 'invalid'); return {} })) as { response?: T; client?: ClerkClient | null; errors?: { code?: unknown; long_message?: string; message?: string }[] } & T
    this.current(epoch)
    if (diagnosticStage && method === 'GET') {
      if (diagnosticStage === 'social_attempt_get') classifyAttempt(json.response ?? json, 'attempt_social')
      else classifyClient(json, json.response ?? json)
    }
    if (!res.ok) throw new ClerkError(res.status, 'Clerk request rejected.', Array.isArray(json.errors) && json.errors.length === 1 && typeof json.errors[0]?.code === 'string' ? json.errors[0].code : null)
    if (apply && 'client' in json) this.applyClient(json.client ?? null)
    return (json.response ?? json) as T
  }

  private applyClient(client: ClerkClient | null) {
    const previousClient = this.client
    const previous = this.activeSession
    this.client = client
    const next = this.activeSession
    if (previous && (previous.id !== next?.id || previous.user?.id !== next?.user?.id)) {
      this.client = previousClient
      this.cachedToken = null
      if (terminalSession(client, previous.id)) void this.expire()
      else this.retryLater()
    }
    this.set({})
  }
  private async refreshClient(epoch = this.epoch, expectedSession?: string, diagnosticStage?: RecoveryStage) {
    const client = await this.request<ClerkClient | null>('GET', '/v1/client', undefined, undefined, epoch, false, diagnosticStage)
    this.current(epoch)
    if (!client || !Array.isArray(client.sessions) || client.sessions.some(s => !s || typeof s.id !== 'string' || typeof s.status !== 'string') || (client.last_active_session_id != null && typeof client.last_active_session_id !== 'string')) throw new InvalidStoredAnchor()
    if (expectedSession && (client?.last_active_session_id !== expectedSession || !client.sessions?.some((session) => session.id === expectedSession && session.status === 'active'))) throw new Error('Session could not be confirmed.')
    this.applyClient(client)
  }

  private async passwordCall(record: PasswordAttempt, epoch: number, path: string, method: 'GET' | 'POST', body?: Record<string, string>): Promise<{ ok: boolean; status: number; value: SignInResource; codes: string[]; credential: string | null }> {
    if (!this.base) throw new Error('Unavailable.')
    this.current(epoch)
    const url = new URL(path, this.base)
    if (url.origin !== this.base) throw new Error('Invalid origin.')
    url.searchParams.set('_is_native', 'true')
    const headers: Record<string, string> = { 'x-device-type': process.platform === 'darwin' ? 'macos' : process.platform, 'x-app-version': app.getVersion(), 'x-bundle-id': identity.appId }
    if (record.credential) headers.Authorization = record.credential
    if (body) headers['Content-Type'] = 'application/x-www-form-urlencoded'
    if (method === 'GET') diag('password_attempt_get', 'begin')
    let res: Response
    try { res = await fetch(url, { method, headers, body: body ? new URLSearchParams(body).toString() : undefined, redirect: 'error', credentials: 'omit', signal: AbortSignal.timeout(20_000) }) }
    catch (error) { if (method === 'GET') diag('password_attempt_get', 'transport_failed'); throw error }
    if (method === 'GET') diag('password_attempt_get', res.ok ? 'response' : 'http_refused', recoveryStatus(res))
    if (epoch !== this.epoch) {
      const raw = res.headers.get('authorization')
      let credential = record.credential
      try { if (raw != null) credential = nativeAuthorization(raw) } catch {}
      if (credential) this.backgroundExit([{ credential, attemptId: record.id ?? undefined }])
      this.current(epoch)
    }
    const raw = res.headers.get('authorization')
    let responseCredential = record.credential
    if (raw !== null) {
      const credential = nativeAuthorization(raw)
      if (!credential) throw new Error('Credential unavailable.')
      if (record.id) {
        record = { ...record, credential }
        await this.savePasswordAttempt(record, epoch)
      }
      responseCredential = credential
    }
    const json = await res.json() as { response?: SignInResource; errors?: { code?: unknown }[] } & SignInResource
    this.current(epoch)
    if (method === 'GET') classifyAttempt(json.response ?? json, 'attempt_signin')
    return { ok: res.ok, status: res.status, value: json.response ?? json, codes: Array.isArray(json.errors) ? json.errors.map(e => e.code).filter((code): code is string => typeof code === 'string') : [], credential: responseCredential }
  }

  private async passwordFinish(record: PasswordAttempt, epoch: number, sessionId: string): Promise<boolean> {
    if (!record.credential || record.scope !== 'normal' || record.exitRequested) return false
    this.deviceToken = record.credential
    await this.refreshClient(epoch, sessionId, 'password_client_get'); this.current(epoch)
    if (!this.activeSession?.user || this.activeSession.id !== sessionId) return false
    this.unverifiedPrincipal = { userId: this.activeSession.user.id, sessionId }
    await this.persist(this.deviceToken, epoch); this.current(epoch)
    await this.clearPasswordAttempt(this.passwordAttempt ?? record, epoch); this.current(epoch)
    this.transient = false; this.locallyFenced = false; this.set({ error: null })
    return true
  }
  private async passwordSignIn(identifier: unknown, password: unknown): Promise<SignInFailure | null> {
    if (typeof identifier !== 'string' || !identifier.trim() || identifier.length > 320 || typeof password !== 'string' || !password || password.length > 4096 || this.restoring || !this.state.ready || this.state.busy || !this.base) return 'error'
    if (this.transient || this.localResetPending || this.deviceToken || this.activeSession || this.secondFactor || this.signUpAttempt || this.emailSignIn || this.newPassword) return 'error'
    let record = this.passwordAttempt
    if (record && (record.scope !== 'normal' || record.exitRequested || !['ready', 'rejected'].includes(record.phase))) return 'error'
    const epoch = this.fencePresentation()
    this.set({ busy: true, error: null })
    try {
      if (!this.encryptionAvailable()) throw new Error('Storage unavailable.')
      if (!record) {
        record = { version: 2, scope: 'normal', owner: null, phase: 'starting', factorSent: false, exitRequested: false, credential: null, id: null }
        await this.savePasswordAttempt(record, epoch)
        const created = await this.passwordCall(record, epoch, '/v1/client/sign_ins', 'POST', { identifier })
        if (!created.ok || !created.value.id || !created.credential || created.value.status !== 'needs_first_factor' || created.value.created_session_id) throw new Error('Attempt unavailable.')
        record = { ...record, credential: created.credential, id: created.value.id, phase: 'ready' }
        await this.savePasswordAttempt(record, epoch)
      } else {
        const checked = await this.passwordCall(record, epoch, `/v1/client/sign_ins/${encodeURIComponent(record.id!)}`, 'GET')
        record = this.passwordAttempt ?? record
        if (!checked.ok || checked.value.id !== record.id || checked.value.status !== 'needs_first_factor' || checked.value.created_session_id || checked.value.identifier !== identifier) throw new Error('Attempt unavailable.')
      }
      record = { ...record, phase: 'sent', factorSent: true }; await this.savePasswordAttempt(record, epoch)
      const attempted = await this.passwordCall(record, epoch, `/v1/client/sign_ins/${encodeURIComponent(record.id!)}/attempt_first_factor`, 'POST', { strategy: 'password', password })
      record = this.passwordAttempt ?? record
      if (!attempted.ok && attempted.status === 422 && attempted.codes.length === 1 && ['form_password_incorrect', 'form_password_or_identifier_incorrect'].includes(attempted.codes[0]!)) {
        const checked = await this.passwordCall(record, epoch, `/v1/client/sign_ins/${encodeURIComponent(record.id!)}`, 'GET')
        record = this.passwordAttempt ?? record
        if (!checked.ok || checked.value.id !== record.id || checked.value.status !== 'needs_first_factor' || checked.value.created_session_id) throw new Error('Attempt unconfirmed.')
        // No factor/session remains pending: return an actually usable login screen.
        await this.clearPasswordAttempt(record, epoch); this.current(epoch)
        if (record.credential && record.id) this.backgroundExit([{ credential: record.credential, attemptId: record.id }])
        this.locallyFenced = false; this.set({ error: 'Não foi possível entrar. Confira os dados ou escolha outro método.' })
        return 'error'
      }
      if (attempted.ok && attempted.value.id === record.id && attempted.value.status === 'needs_second_factor' && !attempted.value.created_session_id) {
        // E6a: the password was right; no session exists until the second factor is accepted.
        const strategies = secondFactorStrategies(attempted.value)
        if (strategies.length) { this.secondFactor = { via: 'password', attemptId: record.id!, epoch, strategies }; return 'verification' }
      }
      if (attempted.ok && attempted.value.id === record.id && (attempted.value.status === 'needs_client_trust' || attempted.value.status === 'needs_second_factor') && !attempted.value.created_session_id) {
        // Device Trust (on in production): a password on a new device needs an e-mail code first.
        const factor = emailCodeFactor(attempted.value, 'second')
        if (!factor) { await this.resetLocal(codeCopy.unsupported); return 'verification' }
        const prepared = await this.passwordCall(record, epoch, `/v1/client/sign_ins/${encodeURIComponent(record.id!)}/prepare_second_factor`, 'POST', { strategy: 'email_code', email_address_id: factor })
        if (!prepared.ok) throw new Error('Code not sent.')
        this.emailSignIn = { attemptId: record.id!, emailAddressId: factor, epoch, resendAt: Date.now() + EMAIL_CODE_RESEND_MS, step: 'second' }
        return 'verification'
      }
      const result = classifyPasswordResponse(attempted.value, record)
      if (attempted.ok && result.kind === 'complete' && await this.passwordFinish(record, epoch, result.sessionId)) return null
      throw new Error('Attempt not complete.')
    } catch {
      if (epoch === this.epoch) this.retryLater()
      return 'error'
    } finally { if (epoch === this.epoch) this.set({ busy: false, ready: true }) }
  }
  async signInWithPassword(identifier: string, password: string): Promise<SignInFailure | null> { return this.passwordSignIn(identifier, password) }
  /** Why it ended without an account (`sign_in_failed`); null once signed in. */
  async signIn(provider: SignInProvider): Promise<SignInFailure | null> {
    if (!socialStrategy(provider) || !this.state.socialProviders.includes(provider) || !this.state.ready || this.state.busy || this.restoring || this.transient || this.localResetPending || this.deviceToken || this.passwordAttempt || this.secondFactor || this.signUpAttempt || this.emailSignIn || this.newPassword || !this.base) return 'error'
    const epoch = this.fencePresentation()
    this.deviceBinding = null; this.unverifiedBinding = null; this.unverifiedPrincipal = null
    this.client = null
    const name = provider === 'apple' ? 'Apple' : provider === 'google' ? 'Google' : 'X'
    let failure: SignInFailure | null = null
    let attemptId: string | null = null
    let stage: 'secure_storage' | 'create' | 'prepare' | 'browser' | 'callback' | 'verify' | 'transfer' | 'session' | 'publish' = 'secure_storage'
    this.set({ busy: true, error: null })
    try {
      if (!this.encryptionAvailable()) throw new KeychainBlocked('unavailable')
      await this.persist(null, epoch); this.current(epoch)
      const callbackURL = `${REDIRECT}?attempt=${randomBytes(24).toString('base64url')}`
      const strategy = socialStrategy(provider)!
      stage = 'create'
      const created = await this.request<SignInResource>('POST', '/v1/client/sign_ins', { strategy, redirect_url: callbackURL }, undefined, epoch, false)
      if (typeof created.id !== 'string' || !created.id) throw new Error('Missing sign-in attempt.')
      attemptId = created.id
      stage = 'prepare'
      const prepared = await this.request<SignInResource>('POST', `/v1/client/sign_ins/${encodeURIComponent(created.id)}/prepare_first_factor`, { strategy, redirect_url: callbackURL }, undefined, epoch, false)
      if (prepared.id !== created.id) throw new Error('Different sign-in attempt.')
      const redirect = prepared.first_factor_verification?.external_verification_redirect_url
      if (!redirect) throw new ClerkError(0, 'Redirect URL is missing. Unable to start the sign-in.')
      stage = 'browser'
      const returned = await authenticate(this.browserURL(redirect), SCHEME, callbackURL)
      stage = 'callback'
      const callback = this.recoveryCallback(returned, callbackURL)
      this.current(epoch)
      const nonce = callback.searchParams.get('rotating_token_nonce')
      stage = 'verify'
      const signIn = await this.request<SignInResource>('GET', `/v1/client/sign_ins/${encodeURIComponent(created.id)}`, undefined, nonce ? { rotating_token_nonce: nonce } : undefined, epoch, false)
      if (signIn.id !== created.id) throw new Error('Different sign-in attempt.')
      const transferable = signIn.first_factor_verification?.status === 'transferable' || signIn.second_factor_verification?.status === 'transferable'
      let completedSession = signIn.status === 'complete' ? signIn.created_session_id : null
      if (!nonce && transferable) {
        // A new account: Clerk's transfer flow turns the sign-in into a sign-up.
        stage = 'transfer'
        const signUp = await this.request<{ status: string; created_session_id?: string | null }>('POST', '/v1/client/sign_ups', { transfer: true }, undefined, epoch, false)
        completedSession = signUp.status === 'complete' ? signUp.created_session_id : null
        if (signUp.status !== 'complete') {
          failure = 'verification'
          this.set({ error: `Para criar a conta com ${name}, faltam informações. Você ainda não entrou.` })
        }
      } else if (signIn.status === 'needs_second_factor' && !signIn.created_session_id && secondFactorStrategies(signIn).length) {
        // E6a: Google passed; the account asks for its second factor before any session exists.
        this.secondFactor = { via: 'social', attemptId: created.id, epoch, strategies: secondFactorStrategies(signIn) }
        failure = 'verification'
      } else if (signIn.status !== 'complete') {
        const error = signIn.first_factor_verification?.error
        if (error) throw new ClerkError(0, error.long_message ?? error.message ?? 'Sign-in failed.', typeof error.code === 'string' ? error.code : null)
        failure = 'verification'
        this.set({ error: `O login com ${name} pede mais uma verificação. Você ainda não entrou.` })
      }
      this.current(epoch)
      if (!failure && completedSession) {
        stage = 'session'
        await this.refreshClient(epoch, completedSession)
        if (this.activeSession?.id !== completedSession) { this.applyClient(null); throw new Error('Session could not be confirmed.') }
        stage = 'publish'
        if (this.deviceToken) {
          await this.persist(this.deviceToken, epoch); this.current(epoch)
          this.locallyFenced = false

        }
      } else if (!failure) { throw new Error('Completed session missing.') }
    } catch (error) {
      if (epoch !== this.epoch) return 'cancelled'
      if (error instanceof AuthenticationEnded) {
        failure = error.reason
        const target = this.deviceToken && attemptId ? { credential: this.deviceToken, attemptId } : null
        const cleanupEpoch = this.fencePresentation()
        this.deviceToken = null; this.client = null
        try {
          await this.persist(null, cleanupEpoch); this.current(cleanupEpoch)
          this.transient = false; this.locallyFenced = false
          this.set({ busy: false, ready: true, error: failure === 'cancelled' ? 'Entrada cancelada. Escolha um método para entrar.' : 'O tempo para entrar terminou. Tente de novo ou escolha outro método.' })
        } catch { if (cleanupEpoch === this.epoch) this.retryLater() }
        finally { if (target) this.backgroundExit([target]) }
      } else if (error instanceof ClerkError && waitlistRefusal(error.code)) {
        // E7c: Clerk closed sign-ups (waitlist). No session exists: clear the attempt and point to the list.
        failure = 'verification'
        await this.resetLocal(waitlistCopy)
      } else {
        failure = 'error'
        // Fixed local stages and HTTP status only: no error object, URL, callback,
        // response, provider message, account identifier or credential goes to logs.
        try { console.warn('account:sign-in-failed', { stage, status: error instanceof ClerkError ? error.status : null, reason: error instanceof InvalidNativeCredential ? 'native_credential_format' : error instanceof ClerkError ? 'clerk_rejected' : 'unconfirmed' }) } catch { /* Diagnostic only. */ }
        this.set({ error: traduzir('pt-BR', 'marca.conta.naoConfirmou') })
      }
    } finally {
      if (epoch === this.epoch) {
        if (failure && this.secondFactor?.epoch === epoch) { /* Waits for the code on this screen, still fenced. */ }
        else if (failure && (this.deviceToken || stage === 'secure_storage')) this.retryLater()
        else if (failure) this.locallyFenced = false
        this.set({ busy: false, ready: true })
      }
    }
    return failure
  }
  /**
   * E6a: the authenticator (TOTP) or a recovery code for a sign-in waiting on its second factor. Sent
   * only by the user's click; the code is never stored or logged. Wrong code keeps the step; an ended
   * attempt returns to the methods; an unknown answer follows D185 (retry or sign in again).
   */
  async attemptSecondFactor(strategy: unknown, code: string): Promise<SignInFailure | null> {
    const pending = this.secondFactor
    if (!pending || pending.epoch !== this.epoch || this.state.busy || this.restoring || !this.base || (strategy !== 'totp' && strategy !== 'backup_code') || !pending.strategies.includes(strategy)) return 'error'
    const clean = normalizeCode(strategy, code)
    if (!clean) { this.set({ error: codeCopy[strategy] }); return 'error' }
    const epoch = pending.epoch
    const path = `/v1/client/sign_ins/${encodeURIComponent(pending.attemptId)}/attempt_second_factor`
    this.set({ busy: true, error: null })
    try {
      let result: FapiResult<SignInResource>
      let record = this.passwordAttempt
      if (pending.via === 'password') {
        if (!record || record.id !== pending.attemptId || record.scope !== 'normal' || record.exitRequested) throw new Error('Attempt unavailable.')
        result = await this.passwordCall(record, epoch, path, 'POST', { strategy, code: clean })
        record = this.passwordAttempt ?? record
      } else {
        try { result = { ok: true, status: 200, value: await this.request<SignInResource>('POST', path, { strategy, code: clean }, undefined, epoch, false), codes: [], credential: this.deviceToken } }
        catch (error) { if (!(error instanceof ClerkError) || error.status < 400 || error.status >= 500) throw error; result = { ok: false, status: error.status, value: { id: '', status: '' }, codes: error.code ? [error.code] : [], credential: this.deviceToken } }
      }
      this.current(epoch)
      if (!result.ok) {
        if (result.status === 422 && result.codes.length === 1 && ['form_code_incorrect', 'form_param_format_invalid'].includes(result.codes[0]!)) { this.set({ error: codeCopy[strategy] }); return 'verification' }
        if (result.status === 429) { this.set({ error: codeCopy.tooMany }); return 'verification' }
        // Expired or ended attempt: no session was created, so the methods come back.
        await this.resetLocal(codeCopy.expired); return 'verification'
      }
      const sessionId = result.value.id === pending.attemptId && result.value.status === 'complete' && typeof result.value.created_session_id === 'string' && result.value.created_session_id ? result.value.created_session_id : null
      if (!sessionId) throw new Error('Attempt not complete.')
      this.secondFactor = null
      if (pending.via === 'password') {
        if (!await this.passwordFinish(record!, epoch, sessionId)) throw new Error('Session could not be confirmed.')
      } else {
        await this.refreshClient(epoch, sessionId)
        if (this.activeSession?.id !== sessionId || !this.deviceToken) throw new Error('Session could not be confirmed.')
        await this.persist(this.deviceToken, epoch); this.current(epoch)
        this.transient = false; this.locallyFenced = false; this.set({ error: null })
      }
      return null
    } catch {
      if (epoch === this.epoch) { this.secondFactor = null; this.retryLater() }
      return 'error'
    } finally { if (epoch === this.epoch) this.set({ busy: false, ready: true }) }
  }

  /** E6a: drops a waiting code step locally (no session exists yet) and returns to the methods. */
  async cancelPending(): Promise<void> {
    if (this.state.busy || (!this.secondFactor && !this.signUpAttempt && !this.emailSignIn && !this.newPassword)) return
    await this.resetLocal(null)
  }

  /** The e-mail code step on screen, for a sign-up or a sign-in. */
  async verifyEmailCode(code: string): Promise<SignInFailure | null> {
    return this.emailSignIn ? this.attemptEmailSignInCode(code) : this.verifySignUp(code)
  }
  async resendEmailCode(): Promise<ResendEmailCodeResult> {
    return this.emailSignIn ? this.resendEmailSignInCode() : this.resendSignUpCode()
  }

  /**
   * E6a: sign in without a password, with a code sent by e-mail as the first factor. Offered only
   * when the instance environment lists `email_code` as a first factor; the Main refuses otherwise.
   * The attempt uses the encrypted password journal (no identifier or code in it). A second factor,
   * when on, is still required after the code.
   */
  async signInWithEmailCode(identifier: string): Promise<SignInFailure | null> {
    return this.state.emailCode ? this.startEmailCode(identifier, 'first') : 'error'
  }
  /** E6a: "Esqueci minha senha", Clerk's native reset_password_email_code, then a new password. */
  async forgotPassword(identifier: string): Promise<SignInFailure | null> {
    return this.state.passwordReset ? this.startEmailCode(identifier, 'reset') : 'error'
  }
  private async startEmailCode(identifier: string, step: 'first' | 'reset'): Promise<SignInFailure | null> {
    const strategy = step === 'reset' ? 'reset_password_email_code' : 'email_code'
    const address = identifier.trim()
    if (!address || address.length > 320 || this.restoring || !this.state.ready || this.state.busy || !this.base) return 'error'
    if (this.transient || this.localResetPending || this.deviceToken || this.activeSession || this.passwordAttempt || this.secondFactor || this.signUpAttempt || this.emailSignIn || this.newPassword) return 'error'
    const epoch = this.fencePresentation()
    this.set({ busy: true, error: null })
    try {
      if (!this.encryptionAvailable()) throw new Error('Storage unavailable.')
      let record: PasswordAttempt = { version: 2, scope: 'normal', owner: null, phase: 'starting', factorSent: false, exitRequested: false, credential: null, id: null }
      await this.savePasswordAttempt(record, epoch)
      const created = await this.passwordCall(record, epoch, '/v1/client/sign_ins', 'POST', { identifier: address })
      if (!created.ok && created.status >= 400 && created.status < 500 && created.status !== 408 && created.status !== 429) {
        // E7c: with sign-ups closed, an address Clerk doesn't know gets the waitlist (Clerk already said so).
        const closed = this.state.signUpMode === 'waitlist' && created.codes.length === 1 && created.codes[0] === 'form_identifier_not_found'
        await this.resetLocal(closed ? waitlistCopy : 'Não foi possível enviar o código. Confira o e-mail ou crie uma conta.'); return 'error'
      }
      const factor = emailCodeFactor(created.value, 'first', strategy)
      if (!created.ok || !created.value.id || !created.credential || created.value.status !== 'needs_first_factor' || created.value.created_session_id) throw new Error('Attempt unavailable.')
      record = { ...record, credential: created.credential, id: created.value.id, phase: 'ready' }
      await this.savePasswordAttempt(record, epoch)
      if (!factor) { await this.resetLocal(step === 'reset' ? 'Esta conta não pode trocar a senha por e-mail. Entre com outro método.' : 'Este e-mail não pode entrar com código. Use a senha ou outro método.'); return 'error' }
      record = { ...record, phase: 'sent', factorSent: true }; await this.savePasswordAttempt(record, epoch)
      const prepared = await this.passwordCall(record, epoch, `/v1/client/sign_ins/${encodeURIComponent(record.id!)}/prepare_first_factor`, 'POST', { strategy, email_address_id: factor })
      if (!prepared.ok) throw new Error('Code not sent.')
      this.emailSignIn = { attemptId: record.id!, emailAddressId: factor, epoch, resendAt: Date.now() + EMAIL_CODE_RESEND_MS, step }
      return 'verification'
    } catch {
      // No code was accepted, so no session can exist: clean locally and say so.
      if (epoch === this.epoch) await this.resetLocal('Não foi possível enviar o código agora. Confira a internet e tente de novo.')
      return 'error'
    } finally { if (epoch === this.epoch) this.set({ busy: false, ready: true }) }
  }

  private async resendEmailSignInCode(): Promise<ResendEmailCodeResult> {
    const pending = this.emailSignIn
    const record = this.passwordAttempt
    if (!pending || pending.epoch !== this.epoch || !record || record.id !== pending.attemptId) return { kind: 'error', code: 'not_pending' }
    if (this.state.busy) return { kind: 'error', code: 'busy' }
    if (Date.now() < pending.resendAt) return { kind: 'limit', retryAt: pending.resendAt }
    const epoch = pending.epoch
    this.set({ busy: true, error: null })
    try {
      const prepared = await this.passwordCall(record, epoch, `/v1/client/sign_ins/${encodeURIComponent(pending.attemptId)}/prepare_${pending.step === 'second' ? 'second' : 'first'}_factor`, 'POST', { strategy: pending.step === 'reset' ? 'reset_password_email_code' : 'email_code', email_address_id: pending.emailAddressId })
      pending.resendAt = Date.now() + EMAIL_CODE_RESEND_MS
      if (!prepared.ok) this.set({ error: prepared.status === 429 ? codeCopy.tooMany : codeCopy.offline })
      return prepared.ok ? { kind: 'ok' } : prepared.status === 429 ? { kind: 'limit', retryAt: pending.resendAt } : { kind: 'error', code: 'network' }
    } catch { if (epoch === this.epoch) this.set({ error: codeCopy.offline }); return { kind: 'error', code: 'network' } }
    finally { if (epoch === this.epoch) this.set({ busy: false, ready: true }) }
  }

  private async attemptEmailSignInCode(code: string): Promise<SignInFailure | null> {
    const pending = this.emailSignIn
    let record = this.passwordAttempt
    if (!pending || pending.epoch !== this.epoch || this.state.busy || this.restoring || !this.base || !record || record.id !== pending.attemptId || record.scope !== 'normal' || record.exitRequested) return 'error'
    const clean = normalizeCode('email_code', code)
    if (!clean) { this.set({ error: codeCopy.email_code }); return 'error' }
    const epoch = pending.epoch
    this.set({ busy: true, error: null })
    try {
      const result = await this.passwordCall(record, epoch, `/v1/client/sign_ins/${encodeURIComponent(pending.attemptId)}/attempt_${pending.step === 'second' ? 'second' : 'first'}_factor`, 'POST', { strategy: pending.step === 'reset' ? 'reset_password_email_code' : 'email_code', code: clean })
      record = this.passwordAttempt ?? record
      if (!result.ok) {
        const only = result.codes.length === 1 ? result.codes[0] : null
        if (result.status === 422 && (only === 'form_code_incorrect' || only === 'form_param_format_invalid')) { this.set({ error: codeCopy.email_code }); return 'verification' }
        if (result.status === 422 && only === 'verification_expired') { this.set({ error: codeCopy.email_expired }); return 'verification' }
        if (result.status === 429) { this.set({ error: codeCopy.tooMany }); return 'verification' }
        await this.resetLocal(codeCopy.expired); return 'verification'
      }
      if (pending.step === 'reset' && result.value.id === pending.attemptId && result.value.status === 'needs_new_password' && !result.value.created_session_id) {
        this.emailSignIn = null
        this.newPassword = { attemptId: pending.attemptId, epoch }
        return 'verification'
      }
      if (pending.step === 'first' && result.value.id === pending.attemptId && result.value.status === 'needs_second_factor' && !result.value.created_session_id) {
        // The code replaces the password, never the second factor.
        this.emailSignIn = null
        const strategies = secondFactorStrategies(result.value)
        if (!strategies.length) { await this.resetLocal(codeCopy.unsupported); return 'verification' }
        this.secondFactor = { via: 'password', attemptId: pending.attemptId, epoch, strategies }
        return 'verification'
      }
      const sessionId = result.value.id === pending.attemptId && result.value.status === 'complete' && typeof result.value.created_session_id === 'string' && result.value.created_session_id ? result.value.created_session_id : null
      if (!sessionId) throw new Error('Attempt not complete.')
      this.emailSignIn = null
      if (!await this.passwordFinish(record, epoch, sessionId)) throw new Error('Session could not be confirmed.')
      return null
    } catch {
      if (epoch === this.epoch) { this.emailSignIn = null; this.retryLater() }
      return 'error'
    } finally { if (epoch === this.epoch) this.set({ busy: false, ready: true }) }
  }

  /**
   * E6a: the new password after an accepted reset code. Clerk checks it (minimum, HIBP); a refusal keeps
   * this step with the rule explained. With 2FA on, the second factor still comes next.
   */
  async resetPassword(password: string): Promise<SignInFailure | null> {
    const pending = this.newPassword
    const rules = this.state.passwordReset
    let record = this.passwordAttempt
    if (!pending || !rules || pending.epoch !== this.epoch || this.state.busy || this.restoring || !this.base || !record || record.id !== pending.attemptId || record.scope !== 'normal' || record.exitRequested || !password || password.length > 4096) return 'error'
    if (password.length < rules.minPasswordLength) { this.set({ error: `A senha precisa ter pelo menos ${rules.minPasswordLength} caracteres.` }); return 'error' }
    const epoch = pending.epoch
    this.set({ busy: true, error: null })
    try {
      const result = await this.passwordCall(record, epoch, `/v1/client/sign_ins/${encodeURIComponent(pending.attemptId)}/reset_password`, 'POST', { password })
      record = this.passwordAttempt ?? record
      if (!result.ok && result.status === 422 && result.codes.length >= 1 && result.codes.every(c => c.startsWith('form_password'))) { this.set({ error: signUpRefusal(result.codes, rules.minPasswordLength) }); return 'verification' }
      if (!result.ok && result.status === 429) { this.set({ error: codeCopy.tooMany }); return 'verification' }
      if (!result.ok && result.status >= 400 && result.status < 500) { await this.resetLocal(codeCopy.expired); return 'verification' }
      if (result.ok && result.value.id === pending.attemptId && result.value.status === 'needs_second_factor' && !result.value.created_session_id) {
        this.newPassword = null
        const strategies = secondFactorStrategies(result.value)
        if (!strategies.length) { await this.resetLocal(codeCopy.unsupported); return 'verification' }
        this.secondFactor = { via: 'password', attemptId: pending.attemptId, epoch, strategies }
        return 'verification'
      }
      const sessionId = result.ok && result.value.id === pending.attemptId && result.value.status === 'complete' && typeof result.value.created_session_id === 'string' && result.value.created_session_id ? result.value.created_session_id : null
      if (!sessionId) throw new Error('Attempt not complete.')
      this.newPassword = null
      if (!await this.passwordFinish(record, epoch, sessionId)) throw new Error('Session could not be confirmed.')
      return null
    } catch {
      if (epoch === this.epoch) { this.newPassword = null; this.retryLater() }
      return 'error'
    } finally { if (epoch === this.epoch) this.set({ busy: false, ready: true }) }
  }

  /** A FAPI call for an unverified sign-up, with its own client credential. Never logs the body. */
  private async signUpCall(credential: string | null, epoch: number, path: string, body: Record<string, string>): Promise<FapiResult<SignUpResource>> {
    if (!this.base) throw new Error('Unavailable.')
    this.current(epoch)
    const url = new URL(path, this.base)
    if (url.origin !== this.base) throw new Error('Invalid origin.')
    url.searchParams.set('_is_native', 'true')
    const headers: Record<string, string> = { 'x-device-type': process.platform === 'darwin' ? 'macos' : process.platform, 'x-app-version': app.getVersion(), 'x-bundle-id': identity.appId, 'Content-Type': 'application/x-www-form-urlencoded' }
    if (credential) headers.Authorization = credential
    const res = await fetch(url, { method: 'POST', headers, body: new URLSearchParams(body).toString(), redirect: 'error', credentials: 'omit', signal: AbortSignal.timeout(20_000) })
    const raw = res.headers.get('authorization')
    let next = credential
    if (raw !== null) { const value = nativeAuthorization(raw); if (!value) throw new Error('Credential unavailable.'); next = value }
    if (epoch !== this.epoch) { if (next) this.backgroundExit([{ credential: next }]); this.current(epoch) }
    const json = await res.json().catch(() => ({})) as { response?: SignUpResource; errors?: { code?: unknown }[] } & SignUpResource
    this.current(epoch)
    return { ok: res.ok, status: res.status, value: json.response ?? json, codes: Array.isArray(json.errors) ? json.errors.map(e => e.code).filter((c): c is string => typeof c === 'string') : [], credential: next }
  }

  /** E6a: e-mail and password sign-up, then an e-mail code. Rules come from the instance environment. */
  async signUpWithPassword(email: string, password: string): Promise<SignInFailure | null> {
    const rules = this.state.signUp
    const address = email.trim()
    if (!rules || !address || address.length > 320 || !password || password.length > 4096 || this.restoring || !this.state.ready || this.state.busy || !this.base) return 'error'
    if (this.transient || this.localResetPending || this.deviceToken || this.activeSession || this.passwordAttempt || this.secondFactor || this.signUpAttempt || this.emailSignIn || this.newPassword) return 'error'
    if (password.length < rules.minPasswordLength) { this.set({ error: `A senha precisa ter pelo menos ${rules.minPasswordLength} caracteres.` }); return 'error' }
    const epoch = this.fencePresentation()
    this.client = null
    this.set({ busy: true, error: null })
    try {
      if (!this.encryptionAvailable()) throw new Error('Storage unavailable.')
      const created = await this.signUpCall(null, epoch, '/v1/client/sign_ups', { email_address: address, password })
      if (!created.ok && created.status >= 400 && created.status < 500) { this.locallyFenced = false; this.set({ error: signUpRefusal(created.codes, rules.minPasswordLength) }); return 'error' }
      const id = created.value.id
      if (!created.ok || typeof id !== 'string' || !id || !created.credential || created.value.status !== 'missing_requirements' || created.value.created_session_id) throw new Error('Sign-up unavailable.')
      const prepared = await this.signUpCall(created.credential, epoch, `/v1/client/sign_ups/${encodeURIComponent(id)}/prepare_verification`, { strategy: 'email_code' })
      if (!prepared.ok || !prepared.credential) throw new Error('Code not sent.')
      this.signUpAttempt = { id, credential: prepared.credential, epoch, resendAt: Date.now() + EMAIL_CODE_RESEND_MS }
      return 'verification'
    } catch {
      // No account exists before the e-mail code: nothing to retry or clean remotely.
      if (epoch === this.epoch) { this.signUpAttempt = null; this.locallyFenced = false; this.set({ error: 'Não foi possível criar a conta agora. Confira a internet e tente de novo.' }) }
      return 'error'
    } finally { if (epoch === this.epoch) this.set({ busy: false, ready: true }) }
  }

  private async resendSignUpCode(): Promise<ResendEmailCodeResult> {
    const pending = this.signUpAttempt
    if (!pending || pending.epoch !== this.epoch) return { kind: 'error', code: 'not_pending' }
    if (this.state.busy) return { kind: 'error', code: 'busy' }
    if (Date.now() < pending.resendAt) return { kind: 'limit', retryAt: pending.resendAt }
    const epoch = pending.epoch
    this.set({ busy: true, error: null })
    try {
      const prepared = await this.signUpCall(pending.credential, epoch, `/v1/client/sign_ups/${encodeURIComponent(pending.id)}/prepare_verification`, { strategy: 'email_code' })
      if (prepared.credential) pending.credential = prepared.credential
      pending.resendAt = Date.now() + EMAIL_CODE_RESEND_MS
      if (!prepared.ok) this.set({ error: prepared.status === 429 ? codeCopy.tooMany : codeCopy.offline })
      return prepared.ok ? { kind: 'ok' } : prepared.status === 429 ? { kind: 'limit', retryAt: pending.resendAt } : { kind: 'error', code: 'network' }
    } catch { if (epoch === this.epoch) this.set({ error: codeCopy.offline }); return { kind: 'error', code: 'network' } }
    finally { if (epoch === this.epoch) this.set({ busy: false, ready: true }) }
  }

  /** E6a: the e-mail code. Only a `complete` sign-up with its session, read back and stored, opens the app. */
  private async verifySignUp(code: string): Promise<SignInFailure | null> {
    const pending = this.signUpAttempt
    if (!pending || pending.epoch !== this.epoch || this.state.busy || this.restoring || !this.base) return 'error'
    const clean = normalizeCode('email_code', code)
    if (!clean) { this.set({ error: codeCopy.email_code }); return 'error' }
    const epoch = pending.epoch
    this.set({ busy: true, error: null })
    let completed = false
    try {
      let result: FapiResult<SignUpResource>
      try { result = await this.signUpCall(pending.credential, epoch, `/v1/client/sign_ups/${encodeURIComponent(pending.id)}/attempt_verification`, { strategy: 'email_code', code: clean }) }
      catch (error) { if (epoch === this.epoch) this.set({ error: codeCopy.offline }); throw error }
      if (result.credential) pending.credential = result.credential
      if (!result.ok) {
        if (result.status === 422 && result.codes.length === 1 && ['form_code_incorrect', 'form_param_format_invalid'].includes(result.codes[0]!)) { this.set({ error: codeCopy.email_code }); return 'verification' }
        if (result.status === 429) { this.set({ error: codeCopy.tooMany }); return 'verification' }
        await this.resetLocal('O código expirou. Crie a conta de novo.'); return 'verification'
      }
      const sessionId = result.value.id === pending.id && result.value.status === 'complete' && typeof result.value.created_session_id === 'string' && result.value.created_session_id ? result.value.created_session_id : null
      if (!sessionId) { await this.resetLocal('A conta pede informações que o app ainda não coleta. Você ainda não entrou.'); return 'verification' }
      completed = true
      this.signUpAttempt = null
      this.deviceToken = pending.credential
      await this.refreshClient(epoch, sessionId, 'password_client_get'); this.current(epoch)
      if (this.activeSession?.id !== sessionId || !this.activeSession.user) throw new Error('Session could not be confirmed.')
      this.unverifiedPrincipal = { userId: this.activeSession.user.id, sessionId }
      await this.persist(this.deviceToken, epoch); this.current(epoch)
      this.transient = false; this.locallyFenced = false; this.set({ error: null })
      return null
    } catch {
      // Before an answer the step stays for another explicit try; after a created session, D185.
      if (epoch === this.epoch && completed) this.retryLater()
      return 'error'
    } finally { if (epoch === this.epoch) this.set({ busy: false, ready: true }) }
  }

  async sessionToken(): Promise<string> {
    if (this.locallyFenced) throw new Error('Sign in again to reach your account.')
    const epoch = this.epoch
    const session = this.activeSession
    if (!session) throw new Error('Sign in again to reach your account.')
    if (this.cachedToken && this.cachedToken.sessionId === session.id && this.cachedToken.until > Date.now()) return this.cachedToken.jwt
    const res = await this.request<{ jwt?: string } | null>('POST', `/v1/client/sessions/${encodeURIComponent(session.id)}/tokens`, undefined, undefined, epoch)
    this.current(epoch)
    if (this.locallyFenced || !res?.jwt || this.activeSession?.id !== session.id) throw new Error('Sign in again to reach your account.')
    this.cachedToken = { sessionId: session.id, jwt: res.jwt, until: Date.now() + 50_000 }
    return res.jwt
  }
}

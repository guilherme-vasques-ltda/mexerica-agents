import type { SignInProvider } from '@shared/analytics'
import type { AccountPendingStep, AccountReconciliationState, ResendEmailCodeResult, SecondFactorStrategy } from '@shared/ipc'
import { Observable, useModel } from '../lib/observable'
import { analytics } from './analytics'

export interface AccountInfo { userId: string; email: string | null; avatarURL: string | null }
export type ExitKind = AccountReconciliationState['kind']
export type ExitAction = 'check' | 'signOut'
export type ExitIntent = 'check'
export interface ExitFlags { registrationRequired?: boolean; cleanupRequired?: boolean }
export function exitUnresolved(kind: ExitKind) { return kind !== 'none' && kind !== 'revoked' }
export function exitActions(kind: ExitKind, _hasSession: boolean, _flags: ExitFlags = {}): ExitAction[] {
  return exitUnresolved(kind) ? ['check', 'signOut'] : []
}

/** D185: renderer sends retry/re-login intents; all authority stays in Main. */
export class AccountSession extends Observable {
  isBusy = false
  isReady = false
  isConfigured = false
  errorMessage: string | null = null
  user: AccountInfo | null = null
  cloudURL: string | null = null
  /** Social sign-ins the configured Clerk instance has on (Main reads it; empty = only e-mail). */
  socialProviders: SignInProvider[] = []
  /** Criar conta por e-mail e senha, se a instância oferece (null = não aparece). */
  signUp: { minPasswordLength: number } | null = null
  /** E7c: modo de cadastro lido pelo Main; só `waitlist` mostra "Entrar na lista de espera". */
  signUpMode: 'public' | 'waitlist' | null = null
  /** E7c: a última tentativa foi recusada porque o cadastro está fechado (lista de espera). */
  waitlist = false
  /** Entrar com código por e-mail, se a instância tem o fator ligado. */
  emailCode = false
  /** Esqueci minha senha por código no e-mail, se a instância permite. */
  passwordReset: { minPasswordLength: number } | null = null
  /** Um código pedido nesta tela (segundo fator ou e-mail do cadastro); a sessão ainda não existe. */
  pending: AccountPendingStep | null = null
  sessionRevision = 0
  provider: SignInProvider | null = null
  exit: ExitKind = 'none'
  exitAction: ExitIntent | null = null
  exitOutcome: { action: ExitIntent; kind: ExitKind; uncertain?: boolean } | null = null
  isSigningOut = false
  /** Web: o aviso "este navegador vai esquecer o vínculo" está aberto, antes de qualquer saída. */
  confirmingSignOut = false
  private turn = 0
  get isSignedIn() { return this.user !== null }
  get isExitUnresolved() { return exitUnresolved(this.exit) }
  get exitChoices() { return exitActions(this.exit, this.isSignedIn) }
  /** Só a web (a conta dela expõe `proximoAcessoExigePareamento`): o Sair apaga o vínculo do navegador. */
  get signOutForgetsBrowser() { return typeof window.codync.account.proximoAcessoExigePareamento === 'function' }
  get canAct() { return this.isReady && !this.isBusy && !this.isSigningOut && !this.exitAction }
  private get scope() { return JSON.stringify([this.isConfigured, this.cloudURL, this.user?.userId ?? null]) }
  async start() {
    this.apply(await window.codync.account.state())
    window.codync.account.onChange(s => { this.apply(s); void this.readExit() })
    await this.readExit()
  }
  private apply(s: Awaited<ReturnType<typeof window.codync.account.state>>) {
    if (this.scope !== JSON.stringify([s.configured, s.cloudURL, s.user?.userId ?? null])) { this.sessionRevision++; this.confirmingSignOut = false }
    this.isReady = s.ready; this.isConfigured = s.configured; this.isBusy = s.busy
    this.errorMessage = s.error; this.user = s.user; this.cloudURL = s.cloudURL
    this.socialProviders = s.socialProviders ?? []
    this.signUp = s.signUp ?? null; this.signUpMode = s.signUpMode === 'public' || s.signUpMode === 'waitlist' ? s.signUpMode : null; this.waitlist = s.waitlist === true; this.emailCode = s.emailCode === true; this.passwordReset = s.passwordReset ?? null; this.pending = s.pending ?? null
    this.changed()
  }
  async signIn(provider: SignInProvider) {
    if (this.isExitUnresolved || !this.canAct) return
    this.provider = provider
    analytics.track({ event: 'sign_in_started', provider })
    const reason = await window.codync.account.signIn(provider)
    if (reason) analytics.track({ event: 'sign_in_failed', provider, reason })
  }
  async signInWithPassword(identifier: string, password: string) {
    if (this.isExitUnresolved || !this.canAct) return
    await window.codync.account.signInWithPassword(identifier, password)
  }
  // Códigos e senha vão direto ao Main; nada fica neste objeto.
  async attemptSecondFactor(strategy: SecondFactorStrategy, code: string) {
    if (!this.canAct || this.pending?.kind !== 'second_factor') return
    await window.codync.account.attemptSecondFactor(strategy, code)
  }
  async signUpWithPassword(email: string, password: string) {
    if (this.isExitUnresolved || !this.canAct || !this.signUp) return
    await window.codync.account.signUpWithPassword(email, password)
  }
  async signInWithEmailCode(email: string) {
    if (this.isExitUnresolved || !this.canAct || !this.emailCode) return
    await window.codync.account.signInWithEmailCode(email)
  }
  async forgotPassword(email: string) {
    if (this.isExitUnresolved || !this.canAct || !this.passwordReset) return
    await window.codync.account.forgotPassword(email)
  }
  async resetPassword(password: string) {
    if (!this.canAct || this.pending?.kind !== 'new_password') return
    await window.codync.account.resetPassword(password)
  }
  async verifyEmailCode(code: string) {
    if (!this.canAct || this.pending?.kind !== 'email_code') return
    await window.codync.account.verifyEmailCode(code)
  }
  /** Mesma tela do desktop: o Main já mostra o erro/limite no estado; o resultado só informa quem chamou. */
  async resendEmailCode(): Promise<ResendEmailCodeResult | null> {
    if (!this.canAct || this.pending?.kind !== 'email_code') return null
    return window.codync.account.resendEmailCode()
  }
  /** Só a intenção: o Main abre a página fixa da lista de espera. */
  async openWaitlist() { await window.codync.account.openWaitlist() }
  async cancelPending() {
    if (!this.pending || this.isBusy) return
    await window.codync.account.cancelPending()
  }
  /** O gesto de "Sair" das telas: na web mostra o aviso antes; nos demais lugares sai direto. */
  async requestSignOut() {
    if (!this.signOutForgetsBrowser) return this.signOut()
    if (this.isSigningOut || this.confirmingSignOut) return
    this.confirmingSignOut = true; this.changed()
  }
  cancelSignOut() {
    if (!this.confirmingSignOut) return
    this.confirmingSignOut = false; this.changed()
  }
  async signOut() {
    if (this.isSigningOut) return
    // Sair also supersedes an in-flight retry. Its late answer cannot overwrite this intent.
    ++this.turn; this.exitAction = null; this.exitOutcome = null
    this.isSigningOut = true; this.changed()
    try { await window.codync.account.signOut() }
    finally { this.isSigningOut = false; this.changed(); await this.readExit() }
  }
  async checkExit() {
    if (!this.canAct || !this.isExitUnresolved) return
    const turn = ++this.turn
    this.exitAction = 'check'; this.exitOutcome = null; this.changed()
    let kind: ExitKind = 'session_unverified'
    try { kind = (await window.codync.account.reconcile('check')).kind }
    catch { /* Retain the retry UI on IPC failure. */ }
    if (turn !== this.turn) return
    this.exitAction = null
    if (kind !== 'unavailable') this.exit = kind
    this.exitOutcome = { action: 'check', kind }
    this.changed(); await this.readExit()
  }
  private async readExit() {
    if (this.exitAction || this.isSigningOut) return
    const turn = ++this.turn; const scope = this.scope
    try {
      const answer = await window.codync.account.reconciliationState()
      if (turn !== this.turn || scope !== this.scope || this.exitAction || this.isSigningOut) return
      this.exit = answer.kind === 'none' ? 'none' : 'session_unverified'
      this.changed()
    } catch { /* Keep the last projection. */ }
  }
}
export const account = new AccountSession()
export function useAccount() { return useModel(account) }

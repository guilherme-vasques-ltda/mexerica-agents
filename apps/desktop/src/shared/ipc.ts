import type { CoresControles } from './janela-faixa'
import type { AppLanguage } from './fork-idioma'
import type { ForkCloudIntent, ForkCloudResult, ForkCloudCallResult, ForkCloudEventsBody, ForkCloudStreamError } from './fork-cloud'
import type { VozBridge } from './fork-voz'
import type { AccountProfileBridge } from './fork-account-profile'
import type { SignInFailure, SignInProvider } from './analytics'
import type { SharedFile } from './models'
import type { SSHBridge } from './ssh'
import type { ScreenAgentState } from './computer-access'
import type { IslandRect, IslandScreen, IslandSummary } from './island'
import type { ErrorReport } from './error-report'
import type { OriginReport } from './guard-hub'
import type { AppIconId } from './app-icons'
import type { PonteCofreChat } from './fork-cofre-seguro'

// The bridge between the main process (host lifecycle, loopback HTTP, tray, windows)
// and the renderer (stores and UI). Exposed as `window.codync` by the preload script.

export type HostState =
  | { kind: 'missingBinary' }
  | { kind: 'notInstalled' }
  | { kind: 'starting' }
  | { kind: 'running' }
  | { kind: 'failed'; message: string }
  /** D76: este computador não roda host (`forkFeatures.localHost`); os bots ficam no Hub, por SSH. */
  | { kind: 'disabled' }

/** This computer's own host, once it answered. */
export interface LocalHost {
  computerId: string
  baseURL: string
  token: string
}

export interface HostSnapshot {
  state: HostState
  local: LocalHost | null
  /** Started against a manually run host (`DONNABOT_PORT`). */
  dev: boolean
  logPath: string
}

export interface CallError {
  status: number
  message: string
  code?: string
}

/** What the tray menu shows; the renderer owns the stores and sends this on every change. */
export interface TraySummary {
  status: string
  canPair: boolean
  bots: { ref: string; name: string; detail: string; working: boolean; avatar: string | null }[]
  approval: string | null
  usage: { name: string; icon: string | null; lines: { title: string; bar: string | null }[] }[]
  screen: {
    enabled: boolean
    subtitle: string
    error: string | null
  } | null
  version: string | null
  needsAttention: boolean
  textSize: number
  usageIconStyle: 'character' | 'original'
}

/** Things the tray menu (or the app menu) asks the window to do. */
export type WindowCommand =
  | { kind: 'openBot'; ref: string }
  | { kind: 'stopBot'; ref: string }
  | { kind: 'reviewApprovals' }
  | { kind: 'confirmReset' }
  | { kind: 'setTextSize'; size: number }
  | { kind: 'stepTextSize'; up: boolean | null }
  | { kind: 'setUsageIconStyle'; style: 'character' | 'original' }
  | { kind: 'setRemoteScreen'; on: boolean }
  | { kind: 'computerAccess' }
  | { kind: 'newBot' }
  | { kind: 'search' }
  | { kind: 'toggleSidebar' }
  /** ⌘, ("Ajustes…" no menu do app): abre a tela de Ajustes (ajustes-tela-01). */
  | { kind: 'openSettings' }
  | { kind: 'toggleDetails' }
  | { kind: 'refresh' }
  | { kind: 'respondPermission'; ref: string; entryId: string; optionId: string }
  /** `replyId`: a Ilha espera o resultado (aceito ou barrado pelo aviso de senha colada) por `island.sendResult`. */
  | { kind: 'sendMessage'; ref: string; text: string; contentOrigin?: OriginReport; replyId?: string; nonce?: string }
  /** The chat window was shown again after being hidden (`window_opened`). */
  | { kind: 'windowOpened' }
  /** MX-122 v3: a janela entrou ou saiu da tela cheia (a faixa do topo tira a reserva dos botões do Mac). */
  | { kind: 'fullScreen'; on: boolean }

/** O que a Ilha recebe de volta ao enviar uma mensagem. */
/** `sem-confirmacao`: a conversa não respondeu (prazo, destino ausente, resposta inválida) — NÃO é aceite. */
export type ResultadoEnvioIlha = 'aceito' | 'bloqueado' | 'sem-confirmacao'

/** The app's own updates (electron-updater, release builds only). */
export interface UpdateState {
  supported: boolean
  checking: boolean
  availableVersion: string | null
  /** Downloading: percent done (0–100); null when nothing is downloading. */
  downloadPercent: number | null
  /** Downloaded and ready: installs on restart. */
  staged: boolean
  /** Restarting into the staged release (the host stops first). */
  installing: boolean
  autoCheck: boolean
  autoDownload: boolean
  lastCheck: number | null
  error: string | null
}

export type AccountReconciliationState = { kind: 'none' | 'pending' | 'checking' | 'active' | 'revoked' | 'needs_authentication' | 'session_unverified' | 'target_unavailable' | 'unavailable' | 'unknown'; cleanupRequired?: boolean; registrationRequired?: boolean; cleanupCount?: number; cleanupCursor?: number }

export interface AccountState {
  /** The persisted session has been restored (or there was none). */
  ready: boolean
  /** This build has a Clerk key and a cloud. */
  configured: boolean
  busy: boolean
  saidaPendente?: 'tentar' | 'semSuporte' | null
  error: string | null
  user: { userId: string; email: string | null; avatarURL: string | null } | null
  cloudURL: string | null
  /** Social sign-ins this Clerk instance has on; empty until its environment is read (fail-closed). */
  socialProviders: SignInProvider[]
  /** Sign-up by e-mail and password, read from the instance's public environment; null = not offered (fail-closed). */
  signUp: { minPasswordLength: number } | null
  /** E7c: the instance's sign-up mode (public environment). null = unknown or other: neither "Criar conta" nor the waitlist. */
  signUpMode: 'public' | 'waitlist' | null
  /** E7c: the last attempt was refused because sign-ups are closed (waitlist copy in `error`). */
  waitlist: boolean
  /** Sign-in by a code sent by e-mail (no password), when the instance lists it as a first factor. */
  emailCode: boolean
  /** "Esqueci minha senha" by an e-mail code, when password and e-mail codes are on; null = hidden. */
  passwordReset: { minPasswordLength: number } | null
  /** A sign-in or sign-up waiting for a code typed on this screen. The session is not confirmed yet. */
  pending: AccountPendingStep | null
}

export type SecondFactorStrategy = 'totp' | 'backup_code'
export type AccountPendingStep =
  | { kind: 'second_factor'; strategies: SecondFactorStrategy[] }
  /** `resendAt`: epoch ms when the Main accepts another e-mail code. */
  /** `device_trust`: Clerk's check of a password on a new device (`needs_client_trust`). */
  | { kind: 'email_code'; purpose: 'sign_up' | 'sign_in' | 'device_trust' | 'reset_password'; resendAt: number }
  /** The reset code was accepted; a new password typed here finishes the sign-in. */
  | { kind: 'new_password'; minPasswordLength: number }

export type SpeechEvent =
  | { type: 'partial' | 'final'; text: string }
  | { type: 'level'; value: number }
  | { type: 'started' }
  | { type: 'stopped' }
  | { type: 'error'; message: string }

/**
 * Resultado do reenvio do código de e-mail. `ok` = o servidor aceitou o envio; `limit` = limite de reenvio (ou prazo ainda
 * não vencido): tente de novo em `retryAt` (ms epoch); `error` = não enviou (códigos sem frase, a UI traduz).
 */
export type ResendEmailCodeResult =
  | { kind: 'ok' }
  | { kind: 'limit'; retryAt: number }
  | { kind: 'error'; code: 'not_pending' | 'busy' | 'refused' | 'network' | 'cancelled' }

export interface CodyncBridge {
  /** MX-200: presente só na ponte web (src/web/platform.ts); o Electron nunca o preenche. */
  web?: true
  forkCloud: {
    invoke(intent: ForkCloudIntent): Promise<ForkCloudResult>
    onChange(cb: (result: ForkCloudResult) => void): () => void
    call(connectionId: string, method: string, body: unknown, timeoutMs: number): Promise<ForkCloudCallResult>
    subscribeEvents(connectionId: string, body: ForkCloudEventsBody, onEvent: (event: unknown) => void, onEnd: (error: ForkCloudStreamError | null) => void): () => void
  }
  /** MX-209: o chat só pede a janela do cartão seguro e ouve avisos sem valor (shared/fork-cofre-seguro.ts). */
  forkCofre: PonteCofreChat
  /** voz-04: a chave de voz no cofre deste aparelho; só escrita, nunca devolve a chave. */
  voz: VozBridge
  /** E6b: Ajustes → Conta (nome, senha, contas ligadas, verificação em duas etapas) pelo Main. */
  accountProfile: AccountProfileBridge
  platform: 'darwin' | 'linux' | 'win32'
  appName: string
  appScheme: string
  hostPort: number
  appVersion: string
  /** The computer's name ("Kevin's MacBook Pro"). */
  computerName: string
  /** DON-903: o idioma do app sugerido pelos idiomas preferidos do sistema, lido localmente no main. */
  suggestedLanguage: AppLanguage | null
  /** This launch was started by the login item (Open at login). */
  openedAtLogin: boolean
  /** Screenshot/UI checks: CODYNC_DEBUG_OPEN=newBot | newGroup | plugins | computers | <bot name>. */
  debugOpen: string | null
  host: {
    snapshot(): Promise<HostSnapshot>
    onChange(cb: (s: HostSnapshot) => void): () => void
    install(): void
    restart(): void
    /** POST /api/<method> on a loopback host. Rejects with a `CallError`. */
    call(baseURL: string, token: string, method: string, body: unknown, timeoutMs: number): Promise<unknown>
    /** SSE on a loopback host: each `data:` line, then `onEnd` (with an error when it failed). */
    stream(url: string, token: string, onData: (line: string) => void, onEnd: (error: CallError | null) => void): () => void
  }
  account: {
    /** Web: ausência de semente significa que a conta precisa parear novamente. */
    proximoAcessoExigePareamento?(userId: string): Promise<boolean>
    tentarSaidaNovamente?(): Promise<void>
    state(): Promise<AccountState>
    onChange(cb: (s: AccountState) => void): () => void
    /** Why it ended without an account; null once signed in (or while another sign-in runs). */
    signIn(provider: SignInProvider): Promise<SignInFailure | null>
    signInWithPassword(identifier: string, password: string): Promise<SignInFailure | null>
    reconcileSignInWithPassword(identifier: string, password: string): Promise<AccountReconciliationState>
    /** The authenticator or recovery code after `pending.kind === 'second_factor'`. */
    attemptSecondFactor(strategy: SecondFactorStrategy, code: string): Promise<SignInFailure | null>
    signUpWithPassword(email: string, password: string): Promise<SignInFailure | null>
    /** Sign in without a password: Clerk e-mails a code (first factor). */
    signInWithEmailCode(email: string): Promise<SignInFailure | null>
    /** The e-mail code on screen, for the sign-up or the e-mail sign-in. */
    verifyEmailCode(code: string): Promise<SignInFailure | null>
    /** Reenvia o código da tela; o resultado é EXPLÍCITO (a UI confirma o envio só por `ok`, nunca por prazo ou ausência de erro). */
    resendEmailCode(): Promise<ResendEmailCodeResult>
    /** "Esqueci minha senha": Clerk e-mails a reset code to this address. */
    forgotPassword(email: string): Promise<SignInFailure | null>
    /** The new password after the reset code. */
    resetPassword(password: string): Promise<SignInFailure | null>
    /** Drops the pending code step locally and returns to the sign-in methods. */
    cancelPending(): Promise<void>
    /** E7c: the Main opens its fixed waitlist page in the browser; the renderer never sends a URL. */
    openWaitlist(): Promise<boolean>
    signOut(): Promise<void>
    reconciliationState(): Promise<AccountReconciliationState>
    reconcile(action: 'check' | 'revoke' | 'cleanup' | 'cleanup_next' | 'register'): Promise<AccountReconciliationState>
    reconcileSignIn(provider: SignInProvider): Promise<AccountReconciliationState>
  }
  /** On-device speech recognition (macOS: SFSpeechRecognizer in a helper; unavailable on Linux). */
  speech: {
    available: boolean
    /** The languages it can transcribe, as locale identifiers. */
    locales(): string[]
    /** `locale` null: the computer's language. */
    start(locale: string | null): void
    stop(): void
    onEvent(cb: (e: SpeechEvent) => void): () => void
  }
  ssh: SSHBridge
  /** The notch island (shared/island.ts): its setting, the chat window's summary, and the island window's side. */
  island: {
    /** False where a window can't place itself at the top of the screen (Linux on Wayland). */
    supported: boolean
    enabled(): Promise<boolean>
    setEnabled(on: boolean): Promise<boolean>
    setSummary(summary: IslandSummary): void
    state(): Promise<{ screen: IslandScreen; summary: IslandSummary }>
    onSummary(cb: (summary: IslandSummary) => void): () => void
    onScreen(cb: (screen: IslandScreen) => void): () => void
    /** The pointer reached or left the shape. */
    onHover(cb: (inside: boolean) => void): () => void
    setShape(rect: IslandRect): void
    /** The message field takes the keyboard (the island never does otherwise). */
    setFocusable(on: boolean): void
    /** ⌥Y / ⌥N answer the approval on screen, from any app, while it shows. */
    setKeys(on: boolean): void
    onKey(cb: (answer: 'allow' | 'deny') => void): () => void
    respond(ref: string, entryId: string, optionId: string): void
    /** Resolve `bloqueado` quando a conversa recusa o envio (aviso de senha aberto): a Ilha mantém o texto. */
    send(ref: string, text: string, contentOrigin: OriginReport | undefined, nonce: string): Promise<ResultadoEnvioIlha>
    /** A janela da conversa responde a um `sendMessage` com `replyId`: só `aceito` ou `bloqueado`. */
    sendResult(replyId: string, resultado: 'aceito' | 'bloqueado'): void
    openBot(ref: string): void
    newBot(): void
    openApp(): void
  }
  /** The Codync cloud's `/v1` API with the signed-in session (and the device key for `signed`). */
  cloud: {
    request(method: string, path: string, body: unknown, signed: boolean): Promise<{ ok: true; value: unknown } | { ok: false; error: { status: number; code: string; message: string | null } }>
  }
  updates: {
    state(): Promise<UpdateState>
    onChange(cb: (s: UpdateState) => void): () => void
    check(): void
    /** Downloads the available release if needed, then restarts into it. */
    install(): void
    setAutoCheck(on: boolean): void
    setAutoDownload(on: boolean): void
  }
  files: {
    begin(file: SharedFile): Promise<string | null>
    write(id: string, offset: number, data: Uint8Array): Promise<void>
    finish(id: string): Promise<void>
    cancel(id: string): Promise<void>
  }
  /** MX-9: o ícone do app é uma escolha local por `id` do manifesto; o renderer nunca manda caminho nem bytes. */
  appIcon: {
    selected(): Promise<AppIconId>
    set(id: AppIconId): Promise<AppIconId>
  }
  app: {
    /** Error tracking: an uncaught error in this window (shared/error-report.ts). */
    reportError(report: ErrorReport): void
    openExternal(url: string): void
    /** A long-form memory note in the user's editor; only paths inside a memory folder open. */
    openMemoryFile(path: string): void
    copy(text: string): void
    /** Files picked in the system dialog: name and bytes. */
    pickFiles(): Promise<{ name: string; data: Uint8Array }[]>
    readClipboardFiles(): Promise<{ name: string; data: Uint8Array }[]>
    setTraySummary(summary: TraySummary): void
    /** D454/D495: o idioma efetivo do app, para o menu do Mac e a barra de menus (processo principal). */
    setLanguage(idioma: AppLanguage): void
    onCommand(cb: (command: WindowCommand) => void): () => void
    /** MX-122 v3: se a janela já está em tela cheia (a faixa do topo, ao montar). */
    isFullScreen(): Promise<boolean>
    /** MX-122 v3: as cores dos controles do sistema na faixa do topo (Windows/Linux; no Mac, nada). */
    setWindowControls(cores: CoresControles): void
    resetAllData(): Promise<void>
    setLaunchAtLogin(on: boolean): Promise<boolean>
    showInDock(): Promise<boolean>
    setShowInDock(on: boolean): Promise<boolean>
    launchAtLogin(): Promise<boolean>
    openSettings(url: string): void
    /**
     * Opens `url` in the system browser and resolves with the URL it returns to on `scheme`
     * (`codync://…`), like ASWebAuthenticationSession. Rejects when cancelled or timed out.
     */
    authenticate(url: string, scheme: string): Promise<string>
    /**
     * Login de conector com o Hub em outro computador (túnel): o app ouve o retorno do navegador em 127.0.0.1, numa
     * porta fixa, só durante o login (main/fork-oauth-loopback.ts). `abrir` devolve a porta (ou `null`, e o login
     * segue pelo caminho de antes); `esperar` abre a página de login e devolve a URL de retorno; `responder` diz ao
     * navegador como terminou; `fechar` desliga o ouvinte.
     */
    loginRetorno: {
      /** A porta e o número da tentativa; as demais chamadas levam esse número (uma tentativa antiga nunca mexe na atual). */
      abrir(): Promise<{ porta: number; tentativa: number } | null>
      esperar(tentativa: number, url: string): Promise<string>
      responder(tentativa: number, resultado: { ok: boolean; nome?: string; detalhe?: string }): void
      fechar(tentativa: number): void
    }
    /** macOS: registers or removes the Codync Screen agent (SMAppService). */
    screenAgentState(): Promise<ScreenAgentState>
    setScreenAgent(on: boolean): Promise<{ needsApproval: boolean }>
    syncScreenAgent(enabled: boolean): Promise<{ needsApproval: boolean }>
  }
}

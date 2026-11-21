// MX-200 (trilha B): o CodyncBridge da web. Mesmo contrato do preload do Electron
// (src/shared/ipc.ts): o renderer não muda de import. O que o navegador não tem vira stub
// explícito — Promise rejeita com o código webUnavailable, void é no-op, onX devolve unsubscribe.
// Partes reais entram por `parts` (account da trilha C, forkCloud da MX-202, accountProfile).

import type {
  AccountReconciliationState,
  AccountState,
  CallError,
  CodyncBridge,
  HostSnapshot,
  UpdateState,
} from '../shared/ipc.ts'
import type { SignInFailure } from '../shared/analytics.ts'
import { APP_LANGUAGES, sugerirIdioma, type AppLanguage } from '../shared/fork-idioma.ts'
import type { EstadoChaves, ResultadoChave, VozBridge } from '../shared/fork-voz.ts'
import type { AccountProfileBridge, ProfileResult } from '../shared/fork-account-profile.ts'
import type { SSHBridge, SSHState } from '../shared/ssh.ts'
import { APP_ICON_DEFAULT_ID, type AppIconId } from '../shared/app-icons.ts'

// Código de erro (não é texto de interface): quem trata a falha mostra a própria mensagem do catálogo.
export const WEB_UNAVAILABLE = 'webUnavailable'

const unavailable = () => Promise.reject(new Error(WEB_UNAVAILABLE))
const none = () => () => {}

// vite.web.config.ts injeta a versão do package.json; sem o define (node --test) cai no fallback.
declare const __APP_VERSION__: string | undefined
const appVersion = typeof __APP_VERSION__ === 'string' ? __APP_VERSION__ : '0.0.0'

export interface WebBridgeParts {
  account?: CodyncBridge['account']
  accountProfile?: CodyncBridge['accountProfile']
  forkCloud?: CodyncBridge['forkCloud']
}

function defaultAccount(): CodyncBridge['account'] {
  const state: AccountState = {
    ready: true,
    configured: false,
    busy: false,
    error: null,
    user: null,
    cloudURL: null,
    socialProviders: [],
    signUp: null,
    signUpMode: null,
    waitlist: false,
    emailCode: false,
    passwordReset: null,
    pending: null,
  }
  const fail: SignInFailure = 'error'
  const reconcile = async (): Promise<AccountReconciliationState> => ({ kind: 'unavailable' })
  return {
    state: async () => state,
    onChange: none,
    signIn: async () => fail,
    signInWithPassword: async () => fail,
    reconcileSignInWithPassword: reconcile,
    attemptSecondFactor: async () => fail,
    signUpWithPassword: async () => fail,
    signInWithEmailCode: async () => fail,
    verifyEmailCode: async () => fail,
    resendEmailCode: async () => ({ kind: 'error' as const, code: 'refused' as const }),
    forgotPassword: async () => fail,
    resetPassword: async () => fail,
    cancelPending: async () => {},
    openWaitlist: async () => false,
    signOut: async () => {},
    reconciliationState: async () => ({ kind: 'unavailable' }),
    reconcile,
    reconcileSignIn: reconcile,
  }
}

function defaultForkCloud(): CodyncBridge['forkCloud'] {
  return {
    invoke: async () => ({ registration: 'unavailable', state: { kind: 'disabled', epoch: 0, attemptId: '' }, computers: [] }),
    onChange: none,
    call: async () => ({ kind: 'notSent', reason: 'notReady' }),
    subscribeEvents: none,
    subscribeFrames: (_connectionId, _onFrame, onEnd) => { queueMicrotask(() => onEnd({ status: 0, message: '' })); return { cancel: none, consumed: none, opened: Promise.resolve(false) } },
  }
}

function defaultVoz(): VozBridge {
  const estado = (): EstadoChaves => ({
    cofre: 'sem-cofre',
    chaves: {
      openai: { situacao: 'ausente', reserva: false },
      gemini: { situacao: 'ausente', reserva: false },
      xai: { situacao: 'ausente', reserva: false },
    },
  })
  const chave = (): Promise<ResultadoChave> => Promise.resolve({ ok: false, categoria: 'sem-cofre', estado: estado() })
  return {
    catalogoXai: async () => ({ ok: false, categoria: 'sem-chave' }),
    credencialXai: async () => ({ ok: false, categoria: 'sem-chave' }),
    conferirSessaoXai: async () => ({ ok: false, categoria: 'sem-chave' }),
    fecharSessaoXai: async () => ({ ok: true }),
    estado: async () => estado(),
    guardar: chave,
    limpar: chave,
    testar: chave,
    usarReserva: chave,
    credencial: async () => ({ ok: false, categoria: 'sem-chave' }),
    definirAtalho: async () => ({ ok: false, motivo: 'indisponivel' }),
    onChange: none,
  }
}

function defaultAccountProfile(): AccountProfileBridge {
  const falha = <T>(): Promise<ProfileResult<T>> =>
    Promise.resolve({ ok: false, failure: 'unavailable', message: WEB_UNAVAILABLE })
  return {
    load: falha,
    saveName: falha,
    setPassword: falha,
    link: falha,
    unlink: falha,
    startTotp: falha,
    confirmTotp: falha,
    disableTotp: falha,
    regenerateCodes: falha,
    saveCodes: async () => 'error',
    startReverify: falha,
    sendReverifyCode: falha,
    attemptReverify: falha,
  }
}

function defaultHost(): CodyncBridge['host'] {
  const snapshot: HostSnapshot = { state: { kind: 'disabled' }, local: null, dev: false, logPath: '' }
  return {
    snapshot: async () => snapshot,
    onChange: none,
    install() {},
    restart() {},
    call: unavailable as CodyncBridge['host']['call'],
    stream: (_url, _token, _onData, onEnd) => {
      const timer = setTimeout(() => onEnd({ status: 503, message: WEB_UNAVAILABLE } satisfies CallError), 0)
      return () => clearTimeout(timer)
    },
  }
}

function defaultSsh(): SSHBridge {
  const state: SSHState = { profiles: [], status: {}, attachments: [] }
  return {
    state: async () => state,
    onChange: none,
    // O editor de SSH não trata rejeição: devolve o motivo "indisponível" do catálogo.
    save: async () => ({ chave: 'ajustes.ssh.demo' }),
    remove() {},
    connect() {},
    disconnect() {},
    trustHostKey() {},
    chooseKey: async () => null,
  }
}

function defaultUpdates(): CodyncBridge['updates'] {
  const state: UpdateState = {
    supported: false, checking: false, availableVersion: null, downloadPercent: null,
    staged: false, installing: false, autoCheck: false, autoDownload: false,
    lastCheck: null, error: null,
  }
  return { state: async () => state, onChange: none, check() {}, install() {}, setAutoCheck() {}, setAutoDownload() {} }
}

/** O primeiro idioma do navegador que o app tem (mesma regra do main: shared/fork-idioma). */
export function suggestedLanguageOf(languages: readonly string[]): AppLanguage | null {
  return sugerirIdioma(languages)
}

export function createWebBridge(parts: WebBridgeParts = {}): CodyncBridge {
  let appIcon: AppIconId = APP_ICON_DEFAULT_ID
  return {
    web: true,
    forkCloud: parts.forkCloud ?? defaultForkCloud(),
    // MX-209: a janela segura é só do app desktop; na web o cartão recusa como "sem suporte".
    forkCofre: { abrir: async () => ({ ok: false, motivo: 'semSuporte' }), abrirNovo: async () => ({ ok: false, motivo: 'semSuporte' }), onEstado: none },
    voz: defaultVoz(),
    accountProfile: parts.accountProfile ?? defaultAccountProfile(),
    platform: 'linux',
    appName: 'Mexerica',
    appScheme: 'codync',
    hostPort: 19422,
    appVersion,
    computerName: 'Navegador',
    suggestedLanguage: suggestedLanguageOf(typeof navigator === 'undefined' ? [] : navigator.languages),
    openedAtLogin: false,
    debugOpen: null,
    host: defaultHost(),
    account: parts.account ?? defaultAccount(),
    speech: { available: false, locales: () => [], start() {}, stop() {}, onEvent: none },
    ssh: defaultSsh(),
    island: {
      supported: false,
      enabled: async () => false,
      setEnabled: async () => false,
      setSummary() {},
      state: async () => ({ screen: { notch: null }, summary: { bots: [], permissions: [], usage: [] } }),
      onSummary: none,
      onScreen: none,
      onHover: none,
      setShape() {},
      setFocusable() {},
      setKeys() {},
      onKey: none,
      respond() {},
      // A Ilha não existe na web: nenhum envio sai por aqui e o resultado é sempre "não confirmado" (falha fechada).
      send: async () => 'sem-confirmacao' as const,
      sendResult() {},
      openBot() {},
      newBot() {},
      openApp() {},
    },
    cloud: {
      request: async () => ({ ok: false, error: { status: 503, code: 'web', message: WEB_UNAVAILABLE } }),
    },
    updates: defaultUpdates(),
    files: {
      begin: unavailable as CodyncBridge['files']['begin'],
      write: unavailable as CodyncBridge['files']['write'],
      finish: unavailable as CodyncBridge['files']['finish'],
      cancel: unavailable as CodyncBridge['files']['cancel'],
    },
    appIcon: {
      selected: async () => appIcon,
      set: async (id) => { appIcon = id; return appIcon },
    },
    app: {
      reportError: (report) => console.error('[codync]', report),
      openExternal(url) {
        // Só https: abre em nova aba sem opener; o resto é recusado (fail-closed). O aviso é genérico e NUNCA traz
        // a URL (nem o erro): um link de bot pode carregar token, e o console é copiado em relatórios de bug.
        try {
          if (new URL(url).protocol !== 'https:') { console.warn('[codync] openExternal recusado'); return }
          window.open(url, '_blank', 'noopener,noreferrer')
        } catch { console.warn('[codync] openExternal recusado') }
      },
      openMemoryFile() {},
      copy(text) {
        void navigator.clipboard?.writeText(text).catch(() => {})
      },
      pickFiles: () => pickFilesWeb(),
      readClipboardFiles: async () => [],
      setTraySummary() {},
      setLanguage() {},
      onCommand: none,
      isFullScreen: async () => false,
      setWindowControls() {},
      resetAllData: async () => {},
      setLaunchAtLogin: async () => false,
      showInDock: async () => true,
      setShowInDock: async () => true,
      launchAtLogin: async () => false,
      openSettings() {},
      authenticate: unavailable as CodyncBridge['app']['authenticate'],
      loginRetorno: { abrir: async () => null, esperar: unavailable as CodyncBridge['app']['loginRetorno']['esperar'], responder() {}, fechar() {} },
      screenAgentState: async () => ({ available: false, needsApproval: false, registered: false }),
      setScreenAgent: unavailable as CodyncBridge['app']['setScreenAgent'],
      syncScreenAgent: async () => ({ needsApproval: false }),
    },
  }
}

/** Diálogo de arquivos do navegador: <input type="file">; devolve nome e bytes de cada um. */
function pickFilesWeb(): Promise<{ name: string; data: Uint8Array }[]> {
  return new Promise((resolve) => {
    const input = document.createElement('input')
    input.type = 'file'
    input.multiple = true
    input.onchange = () => {
      const files = [...(input.files ?? [])]
      void Promise.all(files.map(async (f) => ({ name: f.name, data: new Uint8Array(await f.arrayBuffer()) })))
        .then(resolve, () => resolve([]))
    }
    // Cancelar o diálogo não dispara change; sem isso a promessa nunca resolveria.
    input.addEventListener('cancel', () => resolve([]))
    input.click()
  })
}

// APP_LANGUAGES reexportado para quem quiser conferir a lista sem importar shared/fork-idioma.
export { APP_LANGUAGES }

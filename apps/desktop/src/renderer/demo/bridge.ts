import { demoVozBridge } from './fork-voz'
import { demoAccountProfileBridge } from './fork-account-profile'
import type { AccountState, CallError, CodyncBridge, HostSnapshot, UpdateState } from '@shared/ipc'
import { appIconAsset, type AppIconId } from '@shared/app-icons'
import { DEMO_UNAVAILABLE, FakeHost } from './fake-host'
import { COMPUTER_ID, HOST_NAME, usage as storyUsage } from './story'
import { version } from '../../../package.json'
import { sugerirIdioma, type AppLanguage } from '../../shared/fork-idioma'

// `window.codync` for the browser: a running local host served by `FakeHost`, everything else
// signed out, unavailable or a no-op. Imported before the renderer so module-level reads see it.

const host = new FakeHost(version)
let offline = false
const activeStreams = new Set<(error: CallError | null) => void>()
// Opt-in explícito, exclusivo da demo; indisponível por padrão e sem controle no site/produto.
Object.defineProperty(window, 'donnabotWorkplacesDemo', { value: {
  ativar() { host.workplaces.enabled = true },
  desativar() { host.workplaces.enabled = false },
  cenario(value: 'ready' | 'api' | 'disconnected' | 'error' | 'slow') { host.workplaces.scenario = value },
} })
// Apenas o fake-host: não existe no preload nem no app instalado.
Object.defineProperty(window, 'donnabotEditorDemo', { value: {
  host,
  setOffline(value: boolean) {
    offline = value
    if (value) for (const end of activeStreams) end({ status: 503, message: 'Hub demo desconectado' })
  },
} })
// Só o demo: controles do catálogo D166 sintético e os links que a tela tentou abrir (deve ficar vazio).
const externos: string[] = []
Object.defineProperty(window, 'donnabotSkillsDemo', { value: {
  skills: host.skills,
  externos,
  /** Outro Hub (outro `hostId` e outro catálogo): derruba e reconecta o stream. */
  trocarHub() {
    host.skills.hub = host.skills.hub === 'a' ? 'b' : 'a'
    host.hostId = host.skills.hub === 'a' ? 'demo-host' : 'demo-host-b'
    offline = true
    for (const end of activeStreams) end({ status: 503, message: 'Hub demo trocado' })
    setTimeout(() => { offline = false }, 50)
  },
} })
const none = () => () => {}
const unavailable = () => Promise.reject(new Error(DEMO_UNAVAILABLE))

// MX-217, só a demo: `?ssh=queda` troca o Hub deste computador por um Hub SSH fictício (o mesmo FakeHost atrás de
// um "túnel"), para conferir a queda curta pelo caminho real dos anexos SSH. `donnabotSshDemo.cair()` derruba o túnel
// (o Main fictício segue tentando), `voltar()` abre um túnel novo e `falhar()` simula falha definitiva.
const sshQueda = new URLSearchParams(location.search).get('ssh') === 'queda'
const SSH_PERFIL = 'demo-ssh'
type DemoSSHState = Awaited<ReturnType<CodyncBridge['ssh']['state']>>
let tunel = 1
const sshPerfil = { id: SSH_PERFIL, host: 'hub-demo', port: null, user: null, identityFile: null, remotePort: 19422, computerId: COMPUTER_ID, name: HOST_NAME }
const sshAnexo = () => ({ profileId: SSH_PERFIL, computer: { id: COMPUTER_ID, name: HOST_NAME, signKey: '', boxKey: null }, baseURL: `http://ssh-${tunel}.demo.invalid`, token: `demo-${tunel}` })
let sshEstado: DemoSSHState = sshQueda
  ? { profiles: [sshPerfil], status: { [SSH_PERFIL]: { kind: 'connected', localPort: 40000 + tunel } }, attachments: [sshAnexo()] }
  : { profiles: [], status: {}, attachments: [] }
const sshOuvintes = new Set<(s: DemoSSHState) => void>()
const publicarSSH = (next: DemoSSHState) => { sshEstado = next; for (const ouvir of sshOuvintes) ouvir(next) }
if (sshQueda) Object.defineProperty(window, 'donnabotSshDemo', { value: {
  cair() {
    offline = true
    for (const end of activeStreams) end({ status: 503, message: 'Hub demo desconectado' })
    publicarSSH({ ...sshEstado, status: { [SSH_PERFIL]: { kind: 'retrying', message: 'ssh tunnel ended' } }, attachments: [] })
  },
  voltar() {
    offline = false
    tunel++
    publicarSSH({ ...sshEstado, status: { [SSH_PERFIL]: { kind: 'connected', localPort: 40000 + tunel } }, attachments: [sshAnexo()] })
  },
  falhar() {
    publicarSSH({ ...sshEstado, status: { [SSH_PERFIL]: { kind: 'failed', message: 'Permission denied' } }, attachments: [] })
  },
} })

const snapshot: HostSnapshot = sshQueda
  ? { state: { kind: 'disabled' }, local: null, dev: false, logPath: '' }
  : {
    state: { kind: 'running' },
    local: { computerId: COMPUTER_ID, baseURL: 'http://demo.invalid', token: 'demo' },
    dev: false,
    logPath: '',
  }
// Identidade apenas em memória para a prévia; não representa sessão nem credencial real.
// Só para conferir a entrada (E6a): `?entrada=sair|2fa|email|email-entrar|aparelho|esqueci|nova-senha|sem-internet` mostra a tela sem conta.
// E7c: `?entrada=lista` (cadastro em lista de espera) e `?entrada=lista-google` (Google recusado pela lista).
const entrada = new URLSearchParams(location.search).get('entrada')
const waitlistMode = entrada === 'lista' || entrada === 'lista-google'
const account: AccountState = {
  ready: true, configured: true, busy: false, error: entrada === 'sem-internet' ? 'Não foi possível verificar sua sessão agora' : entrada === 'lista-google' ? 'A Mexerica está em acesso antecipado. Entre na lista de espera e avisamos quando seu convite sair.' : null,
  user: entrada ? null : { userId: 'demo', email: null, avatarURL: null }, cloudURL: null, socialProviders: entrada ? ['google'] : ['apple', 'google', 'x'],
  signUp: waitlistMode ? null : { minPasswordLength: 15 }, signUpMode: waitlistMode ? 'waitlist' : 'public', waitlist: entrada === 'lista-google', emailCode: true, passwordReset: { minPasswordLength: 15 },
  pending: entrada === '2fa' ? { kind: 'second_factor', strategies: ['totp', 'backup_code'] } : entrada === 'nova-senha' ? { kind: 'new_password', minPasswordLength: 15 }
    : entrada === 'email' || entrada === 'email-entrar' || entrada === 'aparelho' || entrada === 'esqueci' ? { kind: 'email_code', purpose: ({ email: 'sign_up', 'email-entrar': 'sign_in', aparelho: 'device_trust', esqueci: 'reset_password' } as const)[entrada], resendAt: Date.now() + 30_000 } : null,
}
const updates: UpdateState = {
  supported: false, checking: false, availableVersion: null, downloadPercent: null, staged: false, installing: false,
  autoCheck: false, autoDownload: false, lastCheck: null, error: null,
}
let appIcon: AppIconId = 'padrao'

const bridge: CodyncBridge = {
  // MX-209: a demo não tem janela segura.
  forkCofre: { abrir: async () => ({ ok: false, motivo: 'semSuporte' }), abrirNovo: async () => ({ ok: false, motivo: 'semSuporte' }), onEstado: () => () => {} },
  forkCloud: {
    invoke: async () => ({ registration: 'unavailable', state: { kind: 'disabled', epoch: 0, attemptId: '' }, computers: [] }),
    onChange: () => () => {},
    call: async () => ({ kind: 'notSent', reason: 'notReady' }),
    subscribeEvents: (_connectionId, _body, _onEvent, _onEnd) => () => {},
  },
  voz: demoVozBridge(),
  accountProfile: demoAccountProfileBridge(),
  platform: 'darwin',
  appName: 'Mexerica',
  appScheme: 'codync',
  hostPort: 19222,
  appVersion: version,
  computerName: HOST_NAME,
  // DON-903: `?idioma=en|pt-BR|nenhum` simula o idioma do sistema; sem ele, o do navegador.
  suggestedLanguage: demoSuggestedLanguage(),
  openedAtLogin: false,
  // Opens the group at launch, like the website's screenshot.
  debugOpen: 'Lançamento',
  host: {
    snapshot: async () => snapshot,
    onChange: none,
    install() {},
    restart() {},
    async call(_baseURL, _token, method, body) {
      if (offline) throw { status: 503, message: 'Hub demo desconectado' } satisfies CallError
      await new Promise((r) => setTimeout(r, 30))
      try {
        return await host.call(method, (body ?? {}) as Record<string, unknown>)
      } catch (e) {
        throw (e as CallError).status ? e : ({ status: 500, message: String(e) } satisfies CallError)
      }
    },
    stream(url, _token, onData, onEnd) {
      if (offline) { const timer = setTimeout(() => onEnd({ status: 503, message: 'Hub demo desconectado' }), 0); return () => clearTimeout(timer) }
      const query = new URL(url).searchParams
      const since = Number(query.get('since') ?? 0)
      let close = () => {}
      const end = (error: CallError | null) => { activeStreams.delete(end); close(); onEnd(error) }
      activeStreams.add(end)
      close = host.subscribe(since, onData, query.get('spaceId') ?? undefined)
      return () => { activeStreams.delete(end); close() }
    },
  },
  account: {
    state: async () => account,
    onChange: none,
    signIn: unavailable,
    signInWithPassword: unavailable,
    reconcileSignInWithPassword: async () => ({ kind: 'unavailable' }),
    signOut: async () => {},
    reconciliationState: async () => ({ kind: entrada === 'sem-internet' ? 'session_unverified' : 'none' }),
    attemptSecondFactor: unavailable,
    signUpWithPassword: unavailable,
    signInWithEmailCode: unavailable,
    verifyEmailCode: unavailable,
    resendEmailCode: async () => ({ kind: 'ok' as const }),
    forgotPassword: unavailable,
    resetPassword: unavailable,
    cancelPending: async () => {},
    openWaitlist: async () => false,
    reconcile: async () => ({ kind: 'unavailable' }),
    reconcileSignIn: async () => ({ kind: 'unavailable' }),
  },
  speech: { available: false, locales: () => [], start() {}, stop() {}, onEvent: none },
  ssh: {
    state: async () => sshEstado,
    onChange: (ouvir) => { sshOuvintes.add(ouvir); return () => void sshOuvintes.delete(ouvir) },
    save: async () => ({ chave: 'ajustes.ssh.demo' }),
    remove() {},
    connect() {},
    disconnect() {},
    trustHostKey() {},
    chooseKey: async () => null,
  },
  island: {
    supported: false,
    enabled: async () => false,
    setEnabled: async () => false,
    setSummary() {},
    // MX-245, só `?uso=longo`: a mesma história de uso da tela de Ajustes, para a janela da ilha isolada
    // (`?window=island`) provar o cartão de Uso com o pior caso de comprimento.
    state: async () => ({ screen: { notch: null }, summary: { bots: [], permissions: [], usage: new URLSearchParams(location.search).get('uso') === 'longo' ? storyUsage(Date.now()).providers : [] } }),
    onSummary: none,
    onScreen: none,
    onHover: none,
    setShape() {},
    setFocusable() {},
    setKeys() {},
    onKey: none,
    respond() {},
    send: async () => 'aceito' as const,
    sendResult() {},
    openBot() {},
    newBot() {},
    openApp() {},
  },
  cloud: { request: async () => ({ ok: false, error: { status: 503, code: 'demo', message: DEMO_UNAVAILABLE } }) },
  updates: { state: async () => updates, onChange: none, check() {}, install() {}, setAutoCheck() {}, setAutoDownload() {} },
  files: { begin: unavailable, write: async () => {}, finish: async () => {}, cancel: async () => {} },
  appIcon: {
    selected: async () => appIcon,
    set: async (id) => { appIcon = appIconAsset(id)?.id ?? appIcon; return appIcon },
  },
  app: {
    reportError() {},
    openExternal: (url) => { externos.push(url); void window.open(url, '_blank', 'noopener') },
    openMemoryFile: () => {},
    copy: (text) => void navigator.clipboard?.writeText(text).catch(() => {}),
    pickFiles: async () => [],
    readClipboardFiles: async () => [],
    setTraySummary() {},
    setLanguage() {},
    onCommand: none,
    isFullScreen: async () => false,
    setWindowControls: () => {},
    resetAllData: async () => {},
    setLaunchAtLogin: async () => false,
    showInDock: async () => true,
    setShowInDock: async () => true,
    launchAtLogin: async () => false,
    openSettings() {},
    authenticate: unavailable,
    loginRetorno: { abrir: async () => null, esperar: unavailable, responder() {}, fechar() {} },
    screenAgentState: async () => ({ available: false, needsApproval: false, registered: false }),
    setScreenAgent: unavailable,
    syncScreenAgent: async () => ({ needsApproval: false }),
  },
}

// Every visit starts from the story: no mirror cache or drafts from a previous visit, onboarding done,
// no GitHub star ask over the demo.
try {
  for (const key of Object.keys(localStorage)) if (key.startsWith('codync-')) localStorage.removeItem(key)
  localStorage.setItem('macAccountOnboardingCompleted', 'true')
  localStorage.setItem('computerAccessSetup', JSON.stringify('later'))
} catch {}

// The dark palette everywhere (vite.demo.config.ts does the same for the CSS).
const matchMedia = window.matchMedia.bind(window)
window.matchMedia = (query) => matchMedia(query.includes('prefers-color-scheme: dark') ? 'all' : query)

window.codync = bridge

/** DON-903: a sugestão da demo, sem rede: `?idioma=` ou, sem ele, navigator.languages (a reserva fora do Electron). */
function demoSuggestedLanguage(): AppLanguage | null {
  const pedido = new URLSearchParams(location.search).get('idioma')
  if (pedido === 'nenhum') return null
  return sugerirIdioma(pedido ? [pedido] : navigator.languages)
}

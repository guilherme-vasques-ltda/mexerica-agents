// MX-9 (dono, 10/11): o ícone principal continua o do pacote; Ajustes › Ícone do app guarda uma escolha local por `id`
// do manifesto e o main aplica o PNG pronto (macOS `app.dock.setIcon`, demais `BrowserWindow.setIcon`). Sem escolha ou com
// `padrao` gravado não chama setIcon; voltar ao padrão depois de uma escolha reseta o Dock com imagem vazia no mac
// (o ícone do bundle/Assets.car volta) e reaplica o icon.png nas janelas. Um `app-icon-v1.json` antigo com uma variante do "d" segue no disco sem ser lido (a D457 o ignorava e
// o MX-9 não o revive). Os testes de ciclo de vida do Dock, do login e da ilha (R10–R14, antes em logo-editor/app-icon-state.test.ts)
// seguem aqui, com o main/index.ts real.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import * as fs from 'node:fs/promises'
import { createRequire } from 'node:module'
import { tmpdir } from 'node:os'
import * as path from 'node:path'
import * as nodeURL from 'node:url'
import * as navigationPolicy from './navigation-policy.ts'
import * as islandShared from './island.ts'
import * as surfaces from './fork-surfaces.ts'
import { deferred, loadMainModule, settle } from './test-support/main-module.ts'

interface DockModule { applyDockVisibility(): void; setShowInDock(on: boolean): boolean }

/** Uma variante antiga do "d" salva antes da D457: o app não lê mais este arquivo. */
const musgo = { style: 'bot', palette: 'musgo' }

interface AuthModule {
  registerSchemes(keepDockHidden?: () => void): void
  handleURL(url: string): void
  authenticate(url: string, scheme: string): Promise<string>
}
/** O main/auth.ts real: esquemas de teste, abrir o navegador só registrado, timers do login sem segurar o processo. */
function loadAuthModule(app: unknown, ipcMain: unknown, browserOpens: string[]) {
  const timer = (run: () => void, ms: number) => { const handle = setTimeout(run, ms); handle.unref(); return handle }
  return loadMainModule<AuthModule>('auth', {
    electron: { app, ipcMain }, './environment': { identity: { port: 0, scheme: 'donnabot', appId: 'app.donnabot.mac' } },
    './navigation': { openExternalURL: async (url: string) => { browserOpens.push(url); return true } },
    '../shared/navigation-policy': navigationPolicy,
  }, { process: { platform: 'darwin', env: {} }, setTimeout: timer, clearTimeout })
}

/**
 * O rig R10–R13. `agent`: o pacote do Mac com `LSUIElement` (S13), que o LaunchServices roda fora do Dock e não põe no
 * Dock ao abrir (fixture nativa R13); sem ele, o pacote comum (o app sem empacotar), que volta para regular a cada
 * abertura (R10/R11). O primeiro plano é modelado (o app ativo, oculto, quem está à frente) só com os fatos medidos nas
 * fixtures e lidos no Electron fixado (`browser_mac.mm`/`native_window_mac.mm` 44.5.1), indicados em cada ponto.
 */
async function appRig({ saved = musgo as unknown, raw = null as string | null, showInDock = false, agent = false, showIsland = false, appIcon = undefined as string | undefined, env = {} as Record<string, string>, dialogo = {} as { showMessageBox?: (janela: unknown, opcoes: Electron.MessageBoxOptions) => Promise<{ response: number }> } } = {}) {
  const dir = await fs.mkdtemp(path.join(tmpdir(), 'd169-r10-'))
  const file = path.join(dir, 'app-icon-v1.json')
  // D457: a preferência antiga do seletor (uma variante do "d", um arquivo inválido ou nenhum) fica no disco, intocada.
  const initial = raw ?? (saved === null ? null : JSON.stringify({ version: 1, selection: saved }) + '\n')
  if (initial !== null) await fs.writeFile(file, initial, { mode: 0o600 })
  const startup: string[] = []
  const log: string[] = []
  // R13: um relógio só (timers e `Date.now()` do dock.ts) e o primeiro plano. O pacote comum nasce regular e ativo; o
  // agente nasce fora do Dock e o LaunchServices não o ativa ao lançar (a janela o ativa quando aparece; fixture R13).
  const clock = { now: 1_000_000 }
  const fg = { active: !agent, hidden: false, front: agent ? 'outro' : 'donnabot', ever: !agent, refused: false, handOff: null as null | { cancelled: boolean } }
  const pendingShows: (() => void)[] = []
  const dock = {
    visible: !agent, tile: 'bundle', failHide: false, ignoreHide: false, holdShow: false, lastShow: -Infinity,
    setIcon(image: { png: string }) { log.push('setIcon'); if (dock.visible) dock.tile = image.png },
    hide() {
      log.push('hide')
      if (dock.failHide) throw new Error('hide falso falhou')
      // DockHide (browser_mac.mm 441–461): nada até 1 s depois de um show; senão marca as janelas canHide:NO e sai de regular.
      if (dock.ignoreHide || clock.now - dock.lastShow < 1000) return
      for (const window of chats) window.canHide = false
      const regular = dock.visible
      dock.visible = false; dock.tile = 'bundle'
      // Fixtures R12/R13: sair de regular com o app ativo passa a ativação ao app anterior. Como no AppKit, a passagem
      // chega depois (o `isActive()` só muda com ela) e um pedido de ativação feito antes dela a vence (R14).
      if (regular && fg.active) {
        const handOff = { cancelled: false }
        fg.handOff = handOff
        queueMicrotask(() => { if (!handOff.cancelled) { fg.handOff = null; deactivate() } })
      }
    },
    show() {
      log.push('show')
      // DockShow (browser_mac.mm 470–503): marca as janelas canHide:YES; o item aparece quando a promessa resolve.
      dock.lastShow = clock.now
      for (const window of chats) window.canHide = true
      const appear = () => { if (!dock.visible) { dock.visible = true; dock.tile = 'bundle' } }
      if (dock.holdShow) return new Promise<void>((resolve) => pendingShows.push(() => { appear(); resolve() }))
      return new Promise<void>((resolve) => queueMicrotask(() => { appear(); resolve() }))
    },
    failVisible: false,
    isVisible: () => { if (dock.failVisible) throw new Error('isVisible falso falhou'); return dock.visible },
  }
  const activate = () => {
    // Ativar desfaz o oculto do app (as janelas ocultadas voltam), como reabrir, o menu e mostrar/focar uma janela.
    if (fg.handOff) { fg.handOff.cancelled = true; fg.handOff = null }
    fg.ever = true
    fg.hidden = false
    for (const window of chats) window.hiddenByApp = false
    if (!fg.active) { fg.active = true; fg.front = 'donnabot'; emit('did-become-active') }
  }
  /** O app perde a ativação: o próximo app da fila (o outro) fica à frente, e o Electron avisa `did-resign-active`. */
  const deactivate = () => { if (fg.active) { fg.active = false; fg.front = 'outro'; emit('did-resign-active') } }
  /** O fato medido nas fixtures R10/R11/R13: o LaunchServices ativa o app ao abri-lo (reabrir, um link); o pacote comum volta ao Dock antes do evento. */
  const launchServicesOpen = () => { log.push('ls-open'); if (!agent && !dock.visible) { dock.visible = true; dock.tile = 'bundle' } fg.refused = false; activate() }
  const prefEntries: [string, unknown][] = [['showInDock', showInDock], ['showIsland', showIsland]]
  if (appIcon !== undefined) prefEntries.push(['appIcon', appIcon])
  const prefs = new Map<string, unknown>(prefEntries)
  const prefWrites: unknown[][] = []
  const protocolClients: string[] = []
  const listeners = new Map<string, ((...args: unknown[]) => unknown)[]>()
  const ready = deferred()
  const app = {
    dock, isPackaged: true, commandLine: { appendSwitch() {} }, setAsDefaultProtocolClient: (scheme: string) => { protocolClients.push(scheme); return true },
    getPath: () => dir, getName: () => 'Donnabot', getVersion: () => '0.0.0',
    requestSingleInstanceLock: () => true, quit() {}, exit() {}, relaunch() {},
    // Browser::Focus = activateIgnoringOtherApps:steal. Fixture R14: sem `steal` não ativa com outro app à frente.
    focus(options?: { steal?: boolean }) { log.push(options?.steal ? 'app:focus:steal' : 'app:focus'); if (options?.steal || fg.front !== 'outro') activate() },
    failIsActive: false, failHide: false,
    isActive() { if (app.failIsActive) throw new Error('isActive falso falhou'); return fg.active },
    isHidden: () => fg.hidden,
    // Browser::Hide = [NSApp hide:] (Apple: oculta as janelas e ativa o próximo app da fila); as canHide:NO ficam.
    hide() {
      log.push('app:hide')
      if (app.failHide) throw new Error('app.hide falso falhou')
      fg.hidden = true
      for (const window of chats) if (window.visible && window.canHide) window.hiddenByApp = true
      deactivate()
    },
    on(event: string, listener: (...args: unknown[]) => unknown) { listeners.set(event, [...(listeners.get(event) ?? []), listener]); return app },
    removeListener(event: string, listener: (...args: unknown[]) => unknown) { listeners.set(event, (listeners.get(event) ?? []).filter((l) => l !== listener)); return app },
    whenReady: () => ready.promise,
  }
  const emit = (event: string, ...args: unknown[]) => {
    const fired = { defaultPrevented: false, preventDefault() { fired.defaultPrevented = true } }
    for (const listener of listeners.get(event) ?? []) listener(fired, ...args)
    return fired
  }
  const handles = new Map<string, (event?: unknown, ...args: unknown[]) => unknown>()
  const ipcMain = { handle: (channel: string, handler: (event?: unknown, ...args: unknown[]) => unknown) => handles.set(channel, handler), on: (channel: string, handler: (event?: unknown, ...args: unknown[]) => unknown) => handles.set(channel, handler) }
  const changes = new Set<() => void>()
  const chats: ChatWindow[] = []
  class ChatWindow {
    static getAllWindows = () => chats
    /** A janela de chat do index.ts ou o painel da ilha (`type: 'panel'`, island.ts real). */
    kind: 'chat' | 'island'
    ipcAtLoad: boolean | null = null
    visible = false
    /** NSWindow.canHide (padrão sim; o DockHide/DockShow do Electron o mudam) e se o oculto do app a tirou da tela. */
    canHide = true
    hiddenByApp = false
    sent: unknown[][] = []
    events = new Map<string, () => void>()
    webContents = {
      mainFrame: { url: '', send: (...args: unknown[]) => { this.sent.push(args); for (const listener of [...changes]) listener() } },
      isLoading: () => false, send: () => {},
    }
    constructor(options: { type?: string } = {}) { this.kind = options.type === 'panel' ? 'island' : 'chat'; log.push('create:' + this.kind); chats.push(this) }
    on(event: string, listener: () => void) { this.events.set(event, listener); return this }
    once(event: string, listener: () => void) { return this.on(event, listener) }
    // O preload pede `island:supported` por sendSync ao carregar: o IPC precisa existir antes do loadURL do chat.
    async loadURL(url: string) { if (this.kind === 'chat') this.ipcAtLoad = handles.has('island:supported'); this.webContents.mainFrame.url = url }
    isVisible() { return this.visible && !this.hiddenByApp }
    isDestroyed() { return false }
    // NativeWindowMac::Show/Focus (native_window_mac.mm 447–491): ativam o app; Focus não age numa janela invisível.
    // Fixtures R13/R14 e revisão R13: no pacote agente com o item no Dock, um painel mostrado sem ativar antes da
    // primeira ativação faz o pedido de ativação do show() do chat não ativar (a ativação do LaunchServices volta a valer).
    show() { log.push('window:show'); this.visible = true; if (fg.refused) log.push('activation-refused'); else activate() }
    showInactive() { this.visible = true; if (this.kind === 'island' && agent && dock.visible && !fg.active && !fg.ever) fg.refused = true }
    setAlwaysOnTop() {}
    setIgnoreMouseEvents() {}
    setVisibleOnAllWorkspaces() {}
    setShape() {}
    setBounds() {}
    getBounds() { return { x: 0, y: 0, width: islandShared.PANEL.width, height: islandShared.PANEL.height } }
    setFocusable() {}
    isFocused() { return false }
    blur() {}
    moveTop() {}
    destroy() { this.visible = false }
    focus() { log.push('window:focus'); if (this.isVisible()) activate() }
    hide() { this.visible = false }
    setBackgroundColor() {}
  }
  const platform = { process: { platform: 'darwin', resourcesPath: dir, env: { ...env } } }
  // R12/R13: os timers e o relógio do dock.ts são do rig (`advance` anda o tempo e roda o que venceu).
  const dockTimers = new Map<number, { due: number, run: () => void }>()
  let timerSeq = 0
  const dockClock = {
    setTimeout: (run: () => void, ms = 0) => { const id = ++timerSeq; dockTimers.set(id, { due: clock.now + ms, run }); return id },
    clearTimeout: (id: number) => { dockTimers.delete(id) },
    Date: { now: () => clock.now },
  }
  const advance = (ms: number) => {
    clock.now += ms
    for (const [id, timer] of [...dockTimers].sort((a, b) => a[1].due - b[1].due)) {
      if (timer.due <= clock.now && dockTimers.delete(id)) timer.run()
    }
  }
  const dockModule = loadMainModule<DockModule & Record<string, unknown>>('dock', { electron: { app }, './host-controller': { prefs: { get: (key: string) => prefs.get(key), set: (key: string, value: unknown) => { prefWrites.push([key, value]); prefs.set(key, value) } } } }, { ...platform, ...dockClock })
  // R11: o auth.ts real (esquemas de teste, navegador neutralizado; o timer do login não segura o processo).
  const browserOpens: string[] = []
  const authModule = loadAuthModule(app, ipcMain, browserOpens)
  // Só fronteiras: tudo o que o main/index.ts importa além do Dock, do ícone e do login é inerte aqui.
  class Inert { state = 'ready'; local = null; logPath = ''; on() { return this } start() {} register() { startup.push('account') } hostChanged() {} update() {} closeToBackground() {} beginQuit() {} }
  // R13: o menu da barra é inerte, mas guarda o `openChat` real que o index.ts lhe passa (o item que abre a janela).
  let menuOpen: (() => unknown) | null = null
  class MenuBar extends Inert { constructor(options: { openChat(): unknown }) { super(); menuOpen = options.openChat } }
  const none = () => {}
  // R14: a island.ts real (registra os IPC síncronos e, ligada, abre o painel). Tela e ponteiro fixos; o laço do ponteiro
  // não corre sozinho (timers guardados, não executados).
  const islandModule = loadMainModule('island', {
    'node:child_process': { execFile: (_file: string, _args: string[], _options: unknown, done: (error: unknown, stdout: string) => void) => done(null, '[]') },
    electron: {
      BrowserWindow: ChatWindow, ipcMain, globalShortcut: { register: () => true, unregister() {}, isRegistered: () => false },
      screen: { on() {}, getPrimaryDisplay: () => ({ bounds: { x: 0, y: 0, width: 1280, height: 900 } }), getAllDisplays: () => [{ bounds: { x: 0, y: 0, width: 1280, height: 900 } }], getCursorScreenPoint: () => ({ x: -10000, y: -10000 }) },
    },
    '../shared/island': islandShared, './host-controller': { prefs: { get: (key: string) => prefs.get(key), set: (key: string, value: unknown) => { prefWrites.push([key, value]); prefs.set(key, value) } } },
  }, { ...platform, console, setTimeout: () => 0, clearTimeout: () => {} })
  loadMainModule('index', {
    './fork-keychain/generation': { prepareKeychain: () => { startup.push('keychain') } },
    './environment': { identity: { port: 0, scheme: 'donnabot' } },
    './fork-voz/registrar': { registerVozIPC: () => { startup.push('voice') } },
    '../shared/fork-features': { forkFeatures: { cloud: false, cloudAccess: false } }, '../shared/fork-surfaces': surfaces,
    './fork-cloud/register': { registerForkCloud: () => assert.fail('cloud fixture is disabled') },
    './fork-cloud/service': { registerForkCloudService: () => assert.fail('cloud fixture is disabled') },
    'node:child_process': { execFileSync: () => '' }, 'node:os': { hostname: () => 'teste' }, 'node:path': path, 'node:url': nodeURL,
    electron: {
      app, autoUpdater: {}, BrowserWindow: ChatWindow, clipboard: { writeText() {} }, dialog: dialogo, ipcMain, shell: {},
      Menu: { buildFromTemplate: (template: unknown) => template, setApplicationMenu() {} },
      nativeImage: { createEmpty: () => ({ png: 'reset', isEmpty: () => true, getSize: () => ({ width: 0, height: 0 }) }), createFromPath: () => ({ png: 'icone-escolhido', isEmpty: () => false, getSize: () => ({ width: 256, height: 256 }) }) },
      nativeTheme: { shouldUseDarkColors: false, on() {} }, session: { defaultSession: { setPermissionRequestHandler() {}, setPermissionCheckHandler() {}, setDisplayMediaRequestHandler() {} } },
    },
    './host-controller': { devPort: null, HostController: Inert, prefs: { get: (key: string) => prefs.get(key), set: (key: string, value: unknown) => { prefWrites.push([key, value]); prefs.set(key, value) } } }, './file-saves': { registerFileSaves: none }, './host-proxy': { registerHostProxy: none },
    './files': { readClipboardFiles: none, readFiles: none }, './tray': { Tray: MenuBar }, './dock': dockModule, './account': { AccountService: Inert },
    './updates': { Updates: Inert, yieldsToInstall: () => false },
    './auth': authModule,
    './fork-oauth-loopback-ipc': { registerOAuthLoopbackIPC: none },
    './speech': { registerSpeech: none }, './screen': { registerScreenIPC: none }, './cloud': { registerCloud: () => { startup.push('cloud') } }, './ssh': { registerSSH: none }, './fork-cofre-seguro/janela': { registrarCofreSeguro: none },
    './quit-lifecycle': { QuitLifecycle: Inert }, './island': islandModule, './error-reports': { registerErrorReports: none },
    './analytics': { reportHostSetupFailure: none },
    './login-item': { launchAtLogin: none, openedAtLogin: none, setLaunchAtLogin: none, updateLoginItem: none },
    './navigation': { openExternalURL: none, openSystemSettingsURL: none, registerWindowNavigation: none },
  }, { ...platform, __dirname: '/app/out/main', URL })
  const idle = async () => { for (let i = 0; i < 12; i++) await settle() }
  /** O início do index.ts: visibilidade, IPC e, por último, a janela de chat (que aparece no ready-to-show). */
  const boot = async () => {
    ready.resolve()
    // Os IPC e a janela saem no mesmo trecho síncrono, depois do initialize e da visibilidade.
    const deadline = performance.now() + 60_000
    while (!handles.has('app:hostPort')) {
      assert.ok(performance.now() < deadline, 'o início deve registrar os IPC em até 60 s')
      await new Promise((resolve) => setTimeout(resolve, 5))
    }
    assert.equal(chats.filter((w) => w.kind === 'chat').length, 1, 'o início abre a janela de chat')
    // Medido (revisão R13, fixtures R13/R14): a ilha aparece antes do chat.
    for (const kind of ['island', 'chat'] as const) chats.find((w) => w.kind === kind)?.events.get('ready-to-show')?.()
    await idle()
    // A pessoa age segundos depois do início (o Electron ignora um hide até 1 s depois de um show).
    advance(5000)
  }
  const chatWindow = () => chats.find((w) => w.kind === 'chat')!
  const disk = async () => ({ raw: await fs.readFile(file, 'utf8').catch(() => null), mtime: (await fs.stat(file).catch(() => null))?.mtimeMs ?? null, files: await fs.readdir(dir) })
  return {
    startup, dock, dockModule, log, prefs, prefWrites, emit, boot, idle, launchServicesOpen, disk, initial, auth: authModule, browserOpens, protocolClients, app,
    /** R12, o fato medido (QA nativo 3; fixtures R12/R13): depois de sair de regular, o macOS às vezes passa a ativação ao app anterior de novo, sem gesto. */
    systemLateSwitch: () => { log.push('late-switch'); deactivate() },
    /** R13-D1 (contraprova da revisão R13): a pessoa volta à Mexerica, uma intenção modelada, sem o LaunchServices. */
    humanReturns: () => { log.push('human-return'); activate() },
    /** R13: a pessoa troca para outro app (um clique, Cmd+Tab): o app perde a ativação. */
    personSwitches: () => { log.push('person-switch'); deactivate() },
    /** R13: o item "abrir" do menu da barra, que chama o `openChat` real. */
    menuOpen: () => { log.push('menu-open'); menuOpen!() },
    /** R13: um painel próprio exibido sem ativar, como a ilha (`type: 'panel'`, `showInactive`). */
    panel: () => { const panel = new ChatWindow({ type: 'panel' }); panel.showInactive(); return panel },
    releaseShows: () => { for (const run of pendingShows.splice(0)) run() },
    advance,
    front: () => fg.front, active: () => fg.active, hidden: () => fg.hidden, windows: () => chats.length,
    /** A janela da retomada (e qualquer outro timer do dock.ts) expira agora. */
    expireDockTimers: () => { for (const [id, timer] of [...dockTimers]) { dockTimers.delete(id); timer.run() } },
    dockTimers: () => dockTimers.size,
    listeners: (event: string) => (listeners.get(event) ?? []).length,
    chat: chatWindow,
    island: () => chats.find((w) => w.kind === 'island'),
    /** O fato do item do Dock agora (visível ou oculto), como nas fixtures do Dock. */
    dockFact: () => (dock.visible ? 'visible' : 'hidden'),
    /** MX-9: sem escolha (ou com a padrão), o item mostra o ícone do pacote e nenhum setIcon desde o início. */
    packageOnly: () => dock.tile === 'bundle' && !log.includes('setIcon'),
    /** MX-9: com uma opção do manifesto, o main chama setIcon e o tile deixa de ser o do pacote. */
    customIcon: () => dock.tile === 'icone-escolhido' && log.includes('setIcon'),
    general: async () => handles.get('app:showInDock')!() as boolean,
    setShowInDock: async (on: boolean) => handles.get('app:setShowInDock')!({}, on) as boolean,
    /** MX-9: o handler IPC cru, para o caso de remetente não confiável (evento sem o frame da janela de chat). */
    handle: (channel: string) => handles.get(channel),
    /** MX-9: os IPC do seletor, chamados como o preload faz (evento do frame principal da janela de chat). */
    appIcon: {
      selected: () => handles.get('appIcon:selected')!({ sender: chatWindow().webContents, senderFrame: chatWindow().webContents.mainFrame }),
      set: (id: unknown) => handles.get('appIcon:set')!({ sender: chatWindow().webContents, senderFrame: chatWindow().webContents.mainFrame }, id),
    },
    since: (mark: number) => log.slice(mark),
    done: () => fs.rm(dir, { recursive: true, force: true }),
  }
}


test('R10 N3 controles: Dock ligado reaplica, segunda instância e activate sem a mudança do LaunchServices não alternam', async () => {
  const unhandled: unknown[] = []
  const onUnhandled = (reason: unknown) => { unhandled.push(reason) }
  process.on('unhandledRejection', onUnhandled)
  try {
    // Mostrar no Dock ligado: reabrir não oculta e a reaplicação do activate devolve a escolha ao item (F3, QA1 dock-12/14).
    const shown = await appRig({ showInDock: true })
    try {
      await shown.boot()
      assert.equal(shown.dock.visible, true)
      const mark = shown.log.length
      shown.launchServicesOpen()
      shown.emit('activate', true)
      await shown.idle()
      assert.equal(shown.dock.visible, true)
      assert.ok(shown.packageOnly(), 'D457: só o ícone do pacote, sem setIcon')
      assert.equal(shown.dockFact(), 'visible')
      assert.ok(!shown.since(mark).includes('hide') && !shown.since(mark).includes('show'), `reabrir com o Dock ligado não alterna: ${shown.since(mark)}`)
      assert.equal(await shown.general(), true)
    } finally { await shown.done() }

    // Desligado: segunda instância (não muda a política, medido) e um activate com o item já fora do Dock.
    const hidden = await appRig()
    try {
      await hidden.boot()
      assert.equal(hidden.dock.visible, false)
      const before = await hidden.disk()
      const mark = hidden.log.length
      hidden.emit('second-instance', ['/Applications/Donnabot.app'])
      await hidden.idle()
      hidden.emit('activate', true)
      await hidden.idle()
      assert.equal(hidden.dock.visible, false)
      assert.equal(hidden.dockFact(), 'hidden')
      assert.ok(!hidden.since(mark).includes('hide') && !hidden.since(mark).includes('show'), `sem a mudança do LaunchServices nada alterna: ${hidden.since(mark)}`)
      assert.ok(hidden.since(mark).includes('window:focus'), 'a janela abre')
      assert.deepEqual(await hidden.disk(), before)
      assert.deepEqual(hidden.prefWrites, [])
    } finally { await hidden.done() }
    assert.deepEqual(unhandled, [])
  } finally { process.off('unhandledRejection', onUnhandled) }
})

test('R10 N3: reabrir com Mostrar no Dock desligado mantém o app fora do Dock, Geral desligado e a tela honesta, sem gravar', async () => {
  const unhandled: unknown[] = []
  const onUnhandled = (reason: unknown) => { unhandled.push(reason) }
  process.on('unhandledRejection', onUnhandled)
  try {
    // Os dois caminhos do QA2: início oculto (relançamento 2) e desligar em Geral com o app aberto.
    for (const route of ['início oculto', 'desligado em Geral'] as const) {
      const r = await appRig({ showInDock: route === 'desligado em Geral' })
      try {
        await r.boot()
        if (route === 'desligado em Geral') { assert.equal(await r.setShowInDock(false), false); await r.idle() }
        assert.equal(r.dock.visible, false, `${route}: o item começa fora do Dock`)
        assert.equal(r.dockFact(), 'hidden')
        const before = await r.disk()
        const writes = r.prefWrites.length
        const mark = r.log.length

        r.launchServicesOpen()
        r.emit('activate', true)
        await r.idle()

        assert.equal(r.dock.visible, false, `${route}: reabrir com Mostrar no Dock desligado deixou a Mexerica no Dock (N3)`)
        const after = r.since(mark)
        assert.ok(!after.includes('show'), `${route}: a correção só oculta: ${after}`)
        assert.ok(after.includes('hide') && after.includes('window:show') && after.includes('window:focus'), `${route}: ${after}`)
        assert.ok(after.indexOf('hide') < after.indexOf('window:show') && after.indexOf('hide') < after.indexOf('window:focus'), `${route}: oculta antes de trazer a janela (ocultar depois desativaria o app): ${after}`)
        const state = { dock: r.dockFact(), error: null }
        assert.equal(state.dock, 'hidden')
        assert.equal(state.error, null)
        assert.equal(r.dockFact(), 'hidden')
        assert.equal(await r.general(), false, 'Geral continua desligado')
        assert.equal(r.prefWrites.length, writes, 'nenhuma gravação de preferência no activate')
        assert.deepEqual(await r.disk(), before, 'nem do arquivo do ícone')
        assert.ok(r.chat().sent.length === 0)
      } finally { await r.done() }
    }

    // Limites da reconciliação. Antes do primeiro pedido do início nada é ocultado (a ordem do início continua). O hide
    // que lança não sai do activate: a janela abre, e a reaplicação do mesmo activate observa o item de novo (visível,
    // como de fato está). O hide que o Electron ignora (menos de 1 s depois de um show) deixa o item visível, e o
    // estado diz isso, com a reaplicação do item visível. Nada disso grava preferência.
    const early = await appRig()
    try {
      const keep = early.dockModule.keepDockHidden as (() => void) | undefined
      assert.equal(typeof keep, 'function')
      keep!()
      assert.deepEqual(early.log, [], 'antes do início, nada')
      await early.boot()
      assert.equal(early.dock.visible, false)
    } finally { await early.done() }
    const failing = await appRig()
    try {
      await failing.boot()
      failing.dock.failHide = true
      failing.launchServicesOpen()
      failing.emit('activate', true)
      await failing.idle()
      assert.ok(failing.log.includes('window:focus'), 'a falha do hide não impede a janela')
      const state = { dock: failing.dockFact(), error: null }
      assert.equal(state.dock, 'visible', 'o fato observado depois da falha')
      assert.equal(await failing.general(), false)
      assert.deepEqual(failing.prefWrites, [])
    } finally { await failing.done() }
    const ignored = await appRig()
    try {
      await ignored.boot()
      ignored.dock.ignoreHide = true
      ignored.launchServicesOpen()
      ignored.emit('activate', true)
      await ignored.idle()
      const state = { dock: ignored.dockFact(), error: null }
      assert.equal(state.dock, 'visible', 'o fato observado prevalece')
      assert.ok(ignored.packageOnly(), 'D457: só o ícone do pacote, sem setIcon')
      assert.equal(await ignored.general(), false)
      assert.deepEqual(ignored.prefWrites, [])
    } finally { await ignored.done() }
    assert.deepEqual(unhandled, [])
  } finally { process.off('unhandledRejection', onUnhandled) }
})

// D169 R11 (o seam da R10): um link dos esquemas do app (`open-url`, o retorno de login pelo navegador) também chega pelo
// LaunchServices, que põe o app oculto no Dock e o ativa antes do evento, sem `activate` (fixture nativa R11). O rig R10
// carrega também o auth.ts real; o Dock falso acrescenta só esse fato medido (`launchServicesOpen` antes do open-url).
const back = 'donnabot://oauth?code=constante'

test('R11 open-url controles: Dock ligado, antes do início, outro esquema e sem callback não alternam nem mudam o login', async () => {
  const unhandled: unknown[] = []
  const onUnhandled = (reason: unknown) => { unhandled.push(reason) }
  process.on('unhandledRejection', onUnhandled)
  try {
    // Dock ligado: o link não alterna, o login resolve com o URL exato e a rota não mostra nem foca janela.
    const shown = await appRig({ showInDock: true })
    try {
      await shown.boot()
      assert.deepEqual(shown.protocolClients, ['donnabot', 'app.donnabot.mac'])
      const login = shown.auth.authenticate('https://example.test/entrar', 'donnabot')
      assert.deepEqual(shown.browserOpens, ['https://example.test/entrar'])
      const mark = shown.log.length
      shown.launchServicesOpen()
      const fired = shown.emit('open-url', back)
      assert.equal(fired.defaultPrevented, true)
      assert.equal(await login, back)
      await shown.idle()
      assert.equal(shown.dock.visible, true)
      assert.deepEqual(shown.since(mark), ['ls-open'], 'nada alterna e nenhuma janela abre')
      assert.equal(await shown.general(), true)
    } finally { await shown.done() }

    // Oculto e antes do início (o app lançado pelo próprio link): nada alterna; o início aplica a preferência.
    const cold = await appRig()
    try {
      assert.equal(cold.emit('open-url', back).defaultPrevented, true)
      assert.deepEqual(cold.log, [])
      await cold.boot()
      assert.equal(cold.dock.visible, false)
    } finally { await cold.done() }

    // Outro esquema não resolve o login pendente nem mexe no Dock; o esquema certo depois resolve.
    const other = await appRig()
    try {
      await other.boot()
      const login = other.auth.authenticate('https://example.test/entrar', 'donnabot')
      const mark = other.log.length
      other.emit('open-url', 'outro://oauth?code=constante')
      assert.equal(await Promise.race([login.then(() => 'resolveu'), other.idle().then(() => 'pendente')]), 'pendente')
      assert.deepEqual(other.since(mark), [])
      other.emit('open-url', 'DONNABOT://oauth?code=depois')
      assert.equal(await login, 'DONNABOT://oauth?code=depois')
    } finally { await other.done() }

    // registerSchemes() sem callback (como era): o tratador continua tratando o URL, sem exceção.
    const listeners: ((event: unknown, url: string) => void)[] = []
    const bare = loadAuthModule({ setAsDefaultProtocolClient: () => true, on: (_name: string, listener: (event: unknown, url: string) => void) => { listeners.push(listener) } }, { handle() {} }, [])
    bare.registerSchemes()
    const login = bare.authenticate('https://example.test/entrar', 'donnabot')
    for (const listener of listeners) listener({ preventDefault() {} }, back)
    assert.equal(await login, back)
    assert.deepEqual(unhandled, [])
  } finally { process.off('unhandledRejection', onUnhandled) }
})

test('R11 open-url: com Mostrar no Dock desligado o link resolve o login e o app sai do Dock de novo, sem janela nem gravação', async () => {
  const unhandled: unknown[] = []
  const onUnhandled = (reason: unknown) => { unhandled.push(reason) }
  process.on('unhandledRejection', onUnhandled)
  try {
    for (const route of ['início oculto', 'desligado em Geral'] as const) {
      const r = await appRig({ showInDock: route === 'desligado em Geral' })
      try {
        await r.boot()
        if (route === 'desligado em Geral') { assert.equal(await r.setShowInDock(false), false); await r.idle() }
        assert.equal(r.dock.visible, false, `${route}: o item começa fora do Dock`)
        assert.equal(r.dockFact(), 'hidden')
        const login = r.auth.authenticate('https://example.test/entrar', 'donnabot')
        const before = await r.disk()
        const writes = r.prefWrites.length
        const mark = r.log.length

        r.launchServicesOpen()
        const fired = r.emit('open-url', back)
        await r.idle()

        assert.equal(fired.defaultPrevented, true)
        assert.equal(await login, back, `${route}: o login resolve com o URL exato`)
        assert.equal(r.dock.visible, false, `${route}: o link deixou a Mexerica no Dock com Mostrar no Dock desligado (N3, open-url)`)
        assert.deepEqual(r.since(mark), ['ls-open', 'hide'], `${route}: só oculta, sem janela, show nem ícone`)
        const state = { dock: r.dockFact(), error: null }
        assert.equal(state.dock, 'hidden')
        assert.equal(state.error, null)
        assert.equal(await r.general(), false, 'Geral continua desligado')
        assert.equal(r.prefWrites.length, writes, 'nenhuma gravação de preferência')
        assert.deepEqual(await r.disk(), before, 'nem do arquivo do ícone')
        assert.ok(r.chat().sent.length === 0)
      } finally { await r.done() }
    }

    // Limites. isVisible que lança: nada oculta e nada novo é afirmado à tela; o login resolve. hide que lança: Dock
    // desconhecido (não há reaplicação nesta rota) e o login resolve. hide ignorado pelo Electron: o item segue visível e o
    // caminho de sempre reaplica a imagem. Callback que lança não impede o login nem sai do tratador. Nada grava preferência.
    for (const fault of ['isVisible', 'hide', 'ignorado'] as const) {
      const r = await appRig()
      try {
        await r.boot()
        const login = r.auth.authenticate('https://example.test/entrar', 'donnabot')
        const sent = r.chat().sent.length
        const mark = r.log.length
        if (fault === 'isVisible') r.dock.failVisible = true
        if (fault === 'hide') r.dock.failHide = true
        if (fault === 'ignorado') r.dock.ignoreHide = true
        r.launchServicesOpen()
        r.emit('open-url', back)
        await r.idle()
        assert.equal(await login, back, `${fault}: o login resolve`)
        r.dock.failVisible = false
        const state = { dock: r.dockFact(), error: null }
        if (fault === 'isVisible') {
          assert.deepEqual(r.since(mark), ['ls-open'], 'sem saber se está visível, não oculta')
          assert.equal(r.chat().sent.length, sent, 'nenhum aviso novo à tela')
        }
        // D457: sem o controller D169, o fato é o do item; o hide que lança o deixa visível.
        if (fault === 'hide') assert.equal(state.dock, 'visible')
        if (fault === 'ignorado') { assert.equal(state.dock, 'visible'); assert.ok(r.packageOnly(), 'D457: só o ícone do pacote, sem setIcon') }
        assert.ok(!r.since(mark).some((entry) => entry.startsWith('window:') || entry === 'show'), `${fault}: sem janela nem show: ${r.since(mark)}`)
        assert.equal(await r.general(), false)
        assert.deepEqual(r.prefWrites, [])
      } finally { await r.done() }
    }
    const listeners: ((event: unknown, url: string) => void)[] = []
    const auth = loadAuthModule({ setAsDefaultProtocolClient: () => true, on: (_name: string, listener: (event: unknown, url: string) => void) => { listeners.push(listener) } }, { handle() {} }, [])
    let called = 0
    auth.registerSchemes(() => { called++; throw new Error('reação do Dock falhou') })
    const login = auth.authenticate('https://example.test/entrar', 'donnabot')
    assert.doesNotThrow(() => { for (const listener of listeners) listener({ preventDefault() {} }, back) })
    assert.equal(await login, back)
    assert.equal(called, 1, 'a reconciliação corre uma vez por link, depois do login')
    assert.deepEqual(unhandled, [])
  } finally { process.off('unhandledRejection', onUnhandled) }
})

// D169 R13 (N4, R12-F1, S13): a segunda troca do macOS vem de sair de regular depois que o LaunchServices pôs o app no
// Dock (fixture R13: também ocultando com o app já inativo); trazer a janela de volta não distingue essa troca de um
// gesto da pessoa (R12-F1). O pacote do Mac passa a ser agente (`LSUIElement`): não volta ao Dock, não há o que ocultar
// nem o que retomar. Os focais R13 usam o rig real acima; o modelo do primeiro plano só tem fatos medidos ou lidos.
const unhandledGuard = async (run: () => Promise<void>) => {
  const unhandled: unknown[] = []
  const onUnhandled = (reason: unknown) => { unhandled.push(reason) }
  process.on('unhandledRejection', onUnhandled)
  try { await run(); assert.deepEqual(unhandled, []) } finally { process.off('unhandledRejection', onUnhandled) }
}
const windowCalls = (entries: string[]) => entries.filter((entry) => entry.startsWith('window:'))

test('R13 N4: o pacote do Mac é agente (LSUIElement) no YAML e nas configs efetivas do app deste Mac e do Dev', () => {
  const desktop = new URL('../../', import.meta.url)
  const require = createRequire(import.meta.url)
  const { parse } = require('yaml') as { parse(text: string): { mac: { extendInfo: Record<string, unknown> } } }
  const base = parse(readFileSync(new URL('electron-builder.yml', desktop), 'utf8'))
  // Os módulos reais que o electron-builder recebe em dist:mac:local e dist:mac:dev (só exigidos; nada é empacotado).
  const local = require(nodeURL.fileURLToPath(new URL('tools/local-builder.cjs', desktop))) as typeof base
  const dev = require(nodeURL.fileURLToPath(new URL('tools/dev-builder.cjs', desktop))) as typeof base
  for (const [name, config] of [['electron-builder.yml', base], ['local-builder.cjs', local], ['dev-builder.cjs', dev]] as const) {
    const info = config.mac.extendInfo
    assert.equal(info.LSUIElement, true, `${name}: o pacote é um app agente`)
    assert.equal(info.LSBackgroundOnly, undefined, `${name}: agente, não só de fundo`)
    assert.equal(info.LSUIPresentationMode, undefined, name)
    assert.equal(info.NSMicrophoneUsageDescription, 'Talk to your bots in a voice call.', `${name}: as outras chaves ficam`)
    assert.equal(info.NSSpeechRecognitionUsageDescription, 'Your words are transcribed on this Mac and sent to your bot as a message.', name)
  }
})

for (const agent of [false, true]) {
  for (const [after, who] of [[80, 'pessoa'], [100, 'pessoa'], [200, 'pessoa'], [401, 'pessoa'], [100, 'sistema']] as const) {
    const label = `${agent ? 'pacote agente' : 'pacote comum'}, ${who} a ${after} ms`
    test(`R13 R12-F1: depois de reabrir, a troca de app não é desfeita (${label})`, async () => {
      await unhandledGuard(async () => {
        const r = await appRig({ agent })
        try {
          await r.boot()
          const before = await r.disk()
          r.personSwitches(); await r.idle()
          assert.equal(r.front(), 'outro', `${label}: a reabertura parte de outro app`)
          const reopen = r.log.length
          r.launchServicesOpen(); r.emit('activate', true); await r.idle()
          assert.ok(windowCalls(r.since(reopen)).includes('window:focus'), `${label}: reabrir traz a janela`)
          assert.equal(r.front(), 'donnabot', `${label}: e o app fica à frente`)
          assert.equal(r.dock.visible, false, `${label}: fora do Dock`)
          r.advance(after)
          const mark = r.log.length
          if (who === 'pessoa') r.personSwitches(); else r.systemLateSwitch()
          await r.idle()
          r.advance(2000); await r.idle()
          assert.deepEqual(windowCalls(r.since(mark)), [], `${label}: nada traz a janela de volta depois da troca (R12-F1)`)
          assert.equal(r.front(), 'outro', `${label}: o outro app continua à frente`)
          assert.equal(r.listeners('did-resign-active'), 0, `${label}: nada fica ouvindo trocas de app`)
          assert.equal(r.dockTimers(), 0, `${label}: nenhum timer`)
          assert.deepEqual(r.prefWrites, [])
          assert.deepEqual(await r.disk(), before)
        } finally { await r.done() }
      })
    })
  }
}

test('R13 N4: pacote agente, início e reabrir com Mostrar no Dock desligado ou ligado, sem ocultar, mostrar ou retomar', async () => {
  await unhandledGuard(async () => {
    for (const showInDock of [false, true]) {
      const label = showInDock ? 'ligado' : 'desligado'
      const r = await appRig({ agent: true, showInDock })
      try {
        await r.boot()
        // Início: desligado, o item nunca aparece; ligado, o pedido de sempre o mostra e o ícone salvo vem junto.
        assert.equal(r.log.includes('show'), showInDock, `${label}: só o pedido do início mostra`)
        assert.equal(r.dock.visible, showInDock)
        assert.equal(r.dockFact(), showInDock ? 'visible' : 'hidden', `${label}: o fato observado`)
        if (showInDock) assert.ok(r.packageOnly(), 'D457: só o ícone do pacote, sem setIcon')
        assert.equal(r.front(), 'donnabot', `${label}: a janela do início traz o app`)
        assert.equal(await r.general(), showInDock)
        const before = await r.disk()
        for (const route of ['reabrir', 'segunda instância', 'menu da barra', 'janela fechada e reabrir'] as const) {
          r.personSwitches(); await r.idle()
          if (route === 'janela fechada e reabrir') r.chat().hide()
          const mark = r.log.length
          if (route === 'reabrir' || route === 'janela fechada e reabrir') { r.launchServicesOpen(); r.emit('activate', true) }
          if (route === 'segunda instância') r.emit('second-instance', ['/Applications/Donnabot.app'])
          if (route === 'menu da barra') r.menuOpen()
          await r.idle()
          const after = r.since(mark)
          assert.ok(!after.includes('hide') && !after.includes('show'), `${label}, ${route}: nada alterna no Dock: ${after}`)
          assert.ok(after.includes('window:show') && after.includes('window:focus'), `${label}, ${route}: a janela vem: ${after}`)
          assert.equal(r.front(), 'donnabot', `${label}, ${route}: o app à frente`)
          assert.equal(r.dock.visible, showInDock)
          assert.equal(r.windows(), 1, `${label}, ${route}: a mesma janela`)
          assert.equal(r.listeners('did-resign-active'), 0)
          assert.equal(r.dockTimers(), 0)
        }
        assert.equal(r.dockFact(), showInDock ? 'visible' : 'hidden')
        assert.equal(await r.general(), showInDock, `${label}: Geral não muda`)
        assert.deepEqual(r.prefWrites, [], `${label}: nenhuma gravação`)
        assert.deepEqual(await r.disk(), before)
      } finally { await r.done() }
    }
    // Quit com a janela destruída: reabrir não arma nada (nenhum ouvinte nem timer).
    const quit = await appRig({ agent: true })
    try {
      await quit.boot()
      quit.emit('before-quit'); quit.chat().events.get('closed')?.()
      quit.launchServicesOpen(); quit.emit('activate', true); await quit.idle()
      assert.equal(quit.listeners('did-resign-active'), 0)
      assert.equal(quit.dockTimers(), 0)
      assert.ok(!quit.log.includes('app:hide'))
    } finally { await quit.done() }
  })
})

test('R13 N4: pacote agente, o link resolve o login e devolve a ativação sem tirar janela ou painel da tela', async () => {
  await unhandledGuard(async () => {
    const r = await appRig({ agent: true })
    try {
      await r.boot()
      const panel = r.panel()
      const before = await r.disk()
      assert.equal(r.dockFact(), 'hidden')
      for (const round of [1, 2]) {
        const login = r.auth.authenticate('https://example.test/entrar', 'donnabot')
        r.personSwitches(); await r.idle() // a pessoa está no navegador
        const sent = r.chat().sent.length
        const mark = r.log.length
        r.launchServicesOpen()
        assert.equal(r.front(), 'donnabot', 'o LaunchServices ativa o app agente antes do evento (fixture R13)')
        const fired = r.emit('open-url', back)
        await r.idle()
        assert.equal(fired.defaultPrevented, true)
        assert.equal(await login, back, `rodada ${round}: o login resolve com o URL exato`)
        assert.deepEqual(r.since(mark), ['ls-open', 'hide', 'app:hide'], `rodada ${round}: o hide de sempre e então ceder`)
        assert.equal(r.front(), 'outro', `rodada ${round}: o navegador volta à frente`)
        assert.equal(r.hidden(), true)
        assert.equal(r.chat().isVisible(), true, `rodada ${round}: a janela de chat continua na tela (canHide:NO)`)
        assert.equal(panel.isVisible(), true, `rodada ${round}: o painel continua na tela`)
        assert.equal(r.windows(), 2, 'nenhuma janela nova')
        assert.equal(r.dock.visible, false)
        assert.ok(r.chat().sent.slice(sent).length === 0, 'nenhum aviso além do fato do Dock')
        // A próxima ativação desfaz o oculto: reabrir na primeira rodada, o menu da barra na segunda.
        const back2 = r.log.length
        if (round === 1) { r.launchServicesOpen(); r.emit('activate', true) } else r.menuOpen()
        await r.idle()
        assert.ok(r.since(back2).includes('window:focus'))
        assert.equal(r.front(), 'donnabot'); assert.equal(r.hidden(), false)
      }
      assert.equal(r.dockFact(), 'hidden')
      assert.equal(await r.general(), false)
      assert.deepEqual(r.prefWrites, [], 'nenhuma gravação de preferência')
      assert.deepEqual(await r.disk(), before, 'nem do arquivo do ícone')
      assert.equal(r.listeners('did-resign-active'), 0)
    } finally { await r.done() }
  })
})

for (const name of ['ligado', 'inativo', 'já ativo', 'ativado há mais de 1 s', 'show em curso', 'show há menos de 1 s', 'show há 1,2 s', 'isActive lança', 'app.hide lança', 'janela fechada', 'quit', 'pacote comum'] as const) {
  test(`R13 N4: o link só cede quando o hide vale e a ativação veio com ele (${name})`, async () => {
    await unhandledGuard(async () => {
      const r = await appRig({ agent: name !== 'pacote comum', showInDock: name === 'ligado' })
      try {
        await r.boot()
        const login = r.auth.authenticate('https://example.test/entrar', 'donnabot')
        if (name === 'show em curso') { r.dock.holdShow = true; await r.setShowInDock(true); await r.setShowInDock(false); r.advance(5000) }
        if (name === 'show há menos de 1 s') { await r.setShowInDock(true); await r.idle(); r.advance(300); await r.setShowInDock(false); await r.idle() }
        if (name === 'show há 1,2 s') { await r.setShowInDock(true); await r.idle(); r.advance(1100); await r.setShowInDock(false); await r.idle(); r.advance(100) }
        if (name === 'isActive lança') r.app.failIsActive = true
        if (name === 'app.hide lança') r.app.failHide = true
        if (name === 'janela fechada') r.chat().hide()
        if (name === 'quit') { r.emit('before-quit'); r.chat().hide(); r.chat().events.get('closed')?.() }
        // A pessoa está no navegador, menos em "já ativo" (estava na Mexerica quando o link chegou).
        if (name !== 'já ativo') { r.personSwitches(); await r.idle() }
        const writes = r.prefWrites.length
        const mark = r.log.length
        if (name !== 'inativo') r.launchServicesOpen()
        if (name === 'ativado há mais de 1 s') r.advance(1100)
        assert.doesNotThrow(() => r.emit('open-url', back), name)
        await r.idle()
        assert.equal(await login, back, `${name}: o login resolve`)
        const after = r.since(mark)
        assert.deepEqual(windowCalls(after), [], `${name}: o link não mostra nem foca janela`)
        assert.ok(!after.includes('show'), `${name}: nunca mostra o Dock`)
        assert.equal(r.windows(), 1, `${name}: nenhuma janela nova`)
        assert.equal(r.prefWrites.length, writes, `${name}: nenhuma gravação`)
        if (name === 'janela fechada' || name === 'quit') {
          assert.deepEqual(after, ['ls-open', 'hide', 'app:hide'], `${name}: cede sem trazer janela`)
          assert.equal(r.chat().isVisible(), false)
        } else if (name === 'app.hide lança') {
          assert.deepEqual(after, ['ls-open', 'hide', 'app:hide'], name)
        } else if (name === 'pacote comum') {
          // R11: o LaunchServices pôs o app no Dock; ocultar de novo já cede a ativação, sem app.hide.
          assert.deepEqual(after, ['ls-open', 'hide'], name)
          assert.equal(r.front(), 'outro')
        } else {
          assert.ok(!after.includes('app:hide'), `${name}: não oculta o app: ${after}`)
          assert.equal(r.chat().isVisible(), true, `${name}: a janela continua na tela`)
          assert.equal(r.hidden(), false)
          if (name === 'inativo') assert.deepEqual(after, [], 'inativo: nada')
          else assert.equal(r.front(), 'donnabot', `${name}: fica como o LaunchServices deixou`)
        }
        r.releaseShows(); await r.idle()
      } finally { await r.done() }
    })
  })
}

test('R13 N4: antes do início (o app lançado pelo próprio link) o link não age, e o início aplica a preferência', async () => {
  await unhandledGuard(async () => {
    const cold = await appRig({ agent: true })
    try {
      assert.equal(cold.emit('open-url', back).defaultPrevented, true)
      assert.deepEqual(cold.log, [])
      await cold.boot()
      assert.equal(cold.dock.visible, false)
    } finally { await cold.done() }
  })
})

// D169 R14 (R13-F1, S14): no pacote agente o LaunchServices não ativa o app ao lançar; com o item no Dock e a ilha
// (painel que não ativa) aparecendo antes do chat, o pedido de ativação do show() do chat não ativava o app (revisão R13:
// 0/4 contra 4/4 na base; fixture R14). O início passa a pedir a ativação uma vez, antes da ilha e do chat. O rig carrega a
// island.ts real; o primeiro plano tem só o fato medido (acima, no ChatWindow e no app.focus).
const bootOrder = (r: Awaited<ReturnType<typeof appRig>>) => r.log.filter((entry) => entry.startsWith('create:') || entry.startsWith('app:focus') || entry === 'activation-refused')
/** No máximo um pedido de ativação no início e, se houver, o mesmo pedido do show() (steal), antes das janelas. */
const assertBootRequest = (order: string[], label: string) => {
  const requests = order.filter((entry) => entry.startsWith('app:focus'))
  assert.ok(requests.length <= 1, `${label}: no máximo um pedido no início: ${order}`)
  if (!requests.length) return
  assert.deepEqual(requests, ['app:focus:steal'], label)
  for (const kind of ['create:island', 'create:chat']) if (order.includes(kind)) assert.ok(order.indexOf('app:focus:steal') < order.indexOf(kind), `${label}: o pedido vem antes de ${kind}: ${order}`)
}

for (const [showInDock, showIsland] of [[true, true], [true, false], [false, true], [false, false]] as const) {
  const label = `Dock ${showInDock ? 'ligado' : 'desligado'}, ilha ${showIsland ? 'ligada' : 'desligada'}`
  test(`R14 R13-F1: pacote agente, o início ativa o app e o chat aparece à frente (${label})`, async () => {
    await unhandledGuard(async () => {
      const r = await appRig({ agent: true, showInDock, showIsland })
      try {
        await r.boot()
        assert.equal(r.active(), true, `${label}: o app fica ativo no início`)
        assert.equal(r.front(), 'donnabot')
        assert.equal(r.chat().isVisible(), true, `${label}: o chat aparece`)
        assert.equal(r.island()?.isVisible() ?? false, showIsland, `${label}: a ilha conforme a preferência`)
        assert.equal(r.dock.visible, showInDock)
        const order = bootOrder(r)
        assert.ok(!order.includes('activation-refused'), `${label}: o pedido do chat não foi recusado: ${order}`)
        assertBootRequest(order, label)
        assert.equal(r.chat().ipcAtLoad, true, `${label}: island:supported existe antes do preload do chat`)
        assert.equal(r.listeners('did-resign-active'), 0, 'nada ouve trocas de app')
        assert.equal(r.dockTimers(), 0, 'nenhum timer')
        assert.deepEqual(r.prefWrites, [], 'nenhuma gravação')
      } finally { await r.done() }
    })
  })
}

for (const name of ['CODYNC_SHOW_INACTIVE', 'pacote comum, Dock e ilha ligados', 'pacote comum, Dock desligado e ilha ligada', 'link antes do início'] as const) {
  test(`R14 R13-F1 controles: o início não pede ativação nova onde não precisa (${name})`, async () => {
    await unhandledGuard(async () => {
      const common = name.startsWith('pacote comum')
      const r = await appRig({ agent: !common, showInDock: name !== 'pacote comum, Dock desligado e ilha ligada', showIsland: name !== 'link antes do início', env: name === 'CODYNC_SHOW_INACTIVE' ? { CODYNC_SHOW_INACTIVE: '1' } : {} })
      try {
        if (name === 'link antes do início') {
          assert.equal(r.emit('open-url', back).defaultPrevented, true)
          assert.deepEqual(r.log, [], 'o link antes do início não age')
        }
        await r.boot()
        const order = bootOrder(r)
        assert.equal(r.chat().isVisible(), true, `${name}: o chat aparece`)
        assert.equal(r.chat().ipcAtLoad, true)
        if (name === 'CODYNC_SHOW_INACTIVE') {
          assert.ok(!order.some((entry) => entry.startsWith('app:focus')), `${name}: nenhum pedido: ${order}`)
          assert.equal(r.active(), false, `${name}: o chat aparece sem ativar, como sem a correção`)
        } else if (common) {
          assert.ok(!order.some((entry) => entry.startsWith('app:focus')), `${name}: já chega ativo pelo LaunchServices: ${order}`)
          assert.equal(r.active(), true, name)
        } else {
          assertBootRequest(order, name)
          assert.equal(r.active(), true, name)
        }
        assert.equal(r.listeners('did-resign-active'), 0)
        assert.equal(r.dockTimers(), 0)
      } finally { await r.done() }
    })
  })
}

test('R14 R13-F1: depois do início com Dock e ilha ligados, reabrir, menu e segunda instância trazem o app e um gesto posterior não é desfeito', async () => {
  await unhandledGuard(async () => {
    const r = await appRig({ agent: true, showInDock: true, showIsland: true })
    try {
      await r.boot()
      assert.equal(r.active(), true, 'o início ativa o app')
      const boot = r.log.length
      for (const route of ['reabrir', 'menu da barra', 'segunda instância'] as const) {
        r.personSwitches(); await r.idle()
        const mark = r.log.length
        if (route === 'reabrir') { r.launchServicesOpen(); r.emit('activate', true) }
        if (route === 'menu da barra') r.menuOpen()
        if (route === 'segunda instância') r.emit('second-instance', ['/Applications/Donnabot.app'])
        await r.idle()
        assert.equal(r.front(), 'donnabot', `${route}: o app à frente`)
        assert.ok(r.since(mark).includes('window:focus'), `${route}: a janela vem`)
        for (const after of [80, 100, 200]) {
          r.advance(after)
          const gesture = r.log.length
          r.personSwitches(); await r.idle(); r.advance(2000); await r.idle()
          assert.deepEqual(windowCalls(r.since(gesture)), [], `${route}, gesto a ${after} ms: nada traz a janela de volta`)
          assert.equal(r.front(), 'outro')
          r.launchServicesOpen(); r.emit('activate', true); await r.idle()
        }
      }
      assert.ok(!r.since(boot).some((entry) => entry.startsWith('app:focus')), 'nenhum pedido de ativação depois do início')
      assert.equal(r.listeners('did-resign-active'), 0)
      assert.equal(r.dockTimers(), 0)
      assert.equal(r.windows(), 2, 'chat e ilha, nenhuma janela nova')
    } finally { await r.done() }
  })
})

// R13-D1 (revisão R13, contraprova `own-flow.mjs`): a guarda "ativado há no máximo 1 s" é um sinal temporal, não prova de
// que a ativação veio do link. Caracterização (igual no 3119 e no head): a pessoa volta ao Donnabot, 100 ms depois chega um
// link de outro esquema: o app cede e consome a marca; o login pendente continua pendente; o link certo resolve depois sem
// outra cedência; nenhuma janela, pedido de foco ou gravação.
test('R14 R13-D1: uma volta da pessoa menos de 1 s antes de um link também é cedida (sinal temporal, sem garantia de origem)', async () => {
  await unhandledGuard(async () => {
    const r = await appRig({ agent: true })
    try {
      await r.boot()
      const before = await r.disk()
      let settled = false
      const login = r.auth.authenticate('https://example.test/entrar', 'donnabot').then((url) => { settled = true; return url })
      r.personSwitches(); r.humanReturns(); r.advance(100)
      const mark = r.log.length
      r.launchServicesOpen(); r.emit('open-url', 'outro://retorno'); await r.idle()
      assert.ok(r.since(mark).includes('app:hide'), 'a volta da pessoa 100 ms antes é cedida (limite declarado)')
      assert.equal(r.active(), false)
      assert.equal(settled, false, 'o login pendente não resolve com outro esquema')
      r.emit('open-url', 'donnabot://final')
      assert.equal(await login, 'donnabot://final', 'o link certo resolve com o URL exato')
      await r.idle()
      const after = r.since(mark)
      assert.equal(after.filter((entry) => entry === 'app:hide').length, 1, 'uma vez por ativação: a marca foi consumida')
      assert.ok(!after.some((entry) => entry.startsWith('window:') || entry.startsWith('app:focus') || entry === 'show'), `sem janela, foco ou Dock: ${after}`)
      assert.deepEqual(await r.disk(), before)
      assert.deepEqual(r.prefWrites, [])
    } finally { await r.done() }
  })
})


// MX-9: o padrão é o ícone do pacote e nenhuma preferência antiga do seletor D169 o muda. Um `app-icon-v1.json` com
// variante do "d" fica no disco sem erro, sem ícone quebrado e sem setIcon: o app não lê o arquivo (nem apaga nem reescreve).
test('MX-9/D457: sem preferência nova, com cada variante antiga do "d" ou com arquivo inválido, o Dock fica com o pacote, sem setIcon', async () => {
  const estilos = ['simbolo', 'bot', 'ruiva', 'terno', 'donna', 'socio', 'associado']
  const paletas = ['grafite', 'violeta', 'noite', 'marfim', 'vinho', 'terno', 'musgo', 'cobre', 'oceano']
  const casos: { nome: string; saved?: unknown; raw?: string }[] = [
    { nome: 'sem preferência', saved: null },
    ...estilos.map((style, i) => ({ nome: `variante ${style}`, saved: { style, palette: paletas[i % paletas.length] } })),
    { nome: 'cores à mão', saved: { style: 'bot', palette: 'custom', colors: { bg: '#123456', ink: '#FFFFFF', wink: '#ABCDEF' } } },
    { nome: 'oficial D451', saved: { style: 'mexerica', palette: 'grafite' } },
    { nome: 'versão desconhecida', raw: JSON.stringify({ version: 99, selection: musgo }) },
    { nome: 'JSON quebrado', raw: '{quebrado' },
    { nome: 'null', raw: 'null' },
  ]
  for (const caso of casos) {
    for (const showInDock of [true, false]) {
      const r = await appRig({ saved: caso.saved === undefined ? musgo : caso.saved, raw: caso.raw ?? null, showInDock })
      try {
        const before = await r.disk()
        await r.boot()
        r.launchServicesOpen()
        r.emit('activate', true)
        await r.idle()
        assert.ok(r.packageOnly(), `${caso.nome}: o item mostra só o ícone do pacote, sem setIcon (${r.log})`)
        assert.equal(r.chat().sent.length, 0, `${caso.nome}: nenhum aviso de ícone à tela`)
        assert.deepEqual(await r.disk(), before, `${caso.nome}: o arquivo antigo não é lido para aplicar nem reescrito`)
      } finally { await r.done() }
    }
  }
})

test('MX-9: a escolha do manifesto aplica o PNG pronto; voltar ao padrão e remetente errado ficam seguros', async () => {
  const r = await appRig({ appIcon: 'icone-01-laranja-close', showInDock: true })
  try {
    await r.boot()
    await r.idle()
    assert.equal(r.appIcon.selected(), 'icone-01-laranja-close')
    assert.ok(r.customIcon(), `a opção chama setIcon (${r.log})`)
    const aplicados = r.log.filter((entry) => entry === 'setIcon').length
    assert.equal(r.appIcon.set('padrao'), 'padrao')
    assert.equal(r.appIcon.selected(), 'padrao')
    // O objeto nasce no contexto vm do main; a comparação vai pelo JSON que o userData recebe.
    const gravado = r.prefWrites.filter(([key]) => key === 'appIcon').at(-1)
    assert.equal(gravado?.[0], 'appIcon')
    assert.deepEqual(JSON.parse(JSON.stringify(gravado?.[1])), { version: 1, id: 'padrao' }, 'a escolha grava versionada no userData')
    assert.ok(r.log.filter((entry) => entry === 'setIcon').length > aplicados, 'voltar ao padrão reaplica o PNG do pacote')
    assert.equal(r.appIcon.set('../resources/icon.png'), 'padrao')
    assert.equal(r.appIcon.set({ file: 'x.png' }), 'padrao')
    const gravacoes = r.prefWrites.filter(([key]) => key === 'appIcon').length
    assert.equal(r.handle('appIcon:set')!({}, 'icone-01-laranja-close'), 'padrao')
    assert.equal(r.appIcon.selected(), 'padrao')
    assert.equal(r.prefWrites.filter(([key]) => key === 'appIcon').length, gravacoes, 'remetente não confiável não grava')
  } finally { await r.done() }
})

test('MX-9 (R1-01 do #523): a página Ícone do app é um radiogroup do manifesto, com IPC só de `id`', () => {
  const desktop = new URL('../../', import.meta.url)
  const read = (file: string) => readFileSync(new URL(file, desktop), 'utf8')
  const settings = read('src/renderer/views/settings/SettingsView.tsx')
  // D454 (fatia 4): toda página leva só a chave do catálogo.
  assert.match(settings, /\{ id: 'app-icon', chave: 'aparencia\.icone\.pagina'/)
  assert.match(settings, /case 'app-icon':\s*return <IconeApp \/>/)
  assert.doesNotMatch(settings, /LogoEditor/)
  const page = read('src/renderer/views/fork/icone-app/IconeApp.tsx').replace(/\/\/[^\n]*|\/\*[\s\S]*?\*\//g, '')
  // A grade vem do manifesto compartilhado; o renderer só envia o `id` e nunca caminho, bytes ou setIcon.
  assert.match(page, /APP_ICON_CHOICES/)
  assert.match(page, /role="radiogroup"/)
  assert.match(page, /role="radio"/)
  assert.match(page, /aria-checked/)
  assert.match(page, /window\.codync\.appIcon\.selected\(\)/)
  assert.match(page, /window\.codync\.appIcon\.set\(id\)/)
  assert.match(page, /t\('aparencia\.icone\.descricao'\)/)
  assert.match(page, /t\('aparencia\.icone\.aviso'\)/)
  assert.doesNotMatch(page, /resources\/icon\.png|setIcon|from 'electron'|node:|fork-app-icon|app-icon-v1/)
  const preload = read('src/preload/index.ts')
  assert.match(preload, /appIcon:\s*\{[\s\S]*selected:\s*\(\)\s*=>\s*ipcRenderer\.invoke\('appIcon:selected'\)[\s\S]*set:\s*\(id\)\s*=>\s*ipcRenderer\.invoke\('appIcon:set', id\)/)
  const main = read('src/main/index.ts')
  assert.match(main, /createAppIconController/)
  assert.match(main, /registerAppIconIPC\(appIcon, \(\) => chat, rendererURL\('chat'\)\)/)
  assert.doesNotMatch(main, /app-icon-v1|LogoEditor/)
})


test('apagar dados exige diálogo nativo e continua restrito ao frame enquanto aguarda', async () => {
  const chamadas: { janela: unknown, opcoes: Electron.MessageBoxOptions }[] = []
  let resposta = deferred<{ response: number }>()
  const app = await appRig({ dialogo: { showMessageBox: (janela, opcoes) => {
    chamadas.push({ janela, opcoes })
    return resposta.promise
  } } })
  try {
    await app.boot()
    const apagar = app.handle('app:resetAllData')!
    const janela = app.chat()
    const sender = janela.webContents
    const senderFrame = sender.mainFrame
    await apagar({ sender: app.panel().webContents, senderFrame })
    await apagar({ sender, senderFrame: { url: senderFrame.url } })
    assert.equal(chamadas.length, 0)

    const cancelado = apagar({ sender, senderFrame })
    assert.equal(chamadas.length, 1)
    assert.equal(chamadas[0]?.janela, janela)
    assert.equal(chamadas[0]?.opcoes.defaultId, 0)
    assert.equal(chamadas[0]?.opcoes.cancelId, 0)
    await apagar({ sender, senderFrame })
    assert.equal(chamadas.length, 1)
    resposta.resolve({ response: 0 })
    await cancelado

    resposta = deferred<{ response: number }>()
    const navegou = apagar({ sender, senderFrame })
    assert.equal(chamadas.length, 2)
    senderFrame.url = 'https://example.test/'
    resposta.resolve({ response: 1 })
    await navegou
  } finally { await app.done() }
})

test('Chaveiro: index real prepara antes de registrar conta, nuvem e voz', async () => {
  const rig = await appRig()
  try {
    await rig.boot()
    for (const consumer of ['account', 'cloud', 'voice']) {
      assert.ok(rig.startup.indexOf(consumer) > rig.startup.indexOf('keychain'), consumer)
    }
    assert.equal(rig.startup.filter(stage => stage === 'keychain').length, 1)
  } finally { await rig.done() }
})

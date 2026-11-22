// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { buildConfig, identity } from './environment'
import { keychainFailure, prepareKeychain } from './fork-keychain/generation'
import { registerVozIPC } from './fork-voz/registrar'
import { execFileSync } from 'node:child_process'
import { hostname } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { app, autoUpdater, BrowserWindow, clipboard, dialog, ipcMain, Menu, nativeTheme, safeStorage, session, shell, type IpcMainInvokeEvent, type WebContents } from 'electron'
import type { HostSnapshot, WindowCommand } from '../shared/ipc'
import { createAppIconController, registerAppIconIPC } from './fork-app-icon-controller'
import { dataDir, devPort, HostController, port, prefs } from './host-controller'
import { registerFileSaves } from './file-saves'
import { registerHostProxy } from './host-proxy'
import { registerProtocol as registerMexeriquinhaProtocol, registerScheme as registerMexeriquinhaScheme } from './fork-mexeriquinha-protocol'
import { generalWindowProvider } from './fork-secret-box/contexts'
import { readClipboardFiles, readFiles } from './files'
import { Tray } from './tray'
import { applyDockVisibility, keepDockHidden, keepDockHiddenAfterLink, onDockVisibility, setShowInDock, showInDock } from './dock'
import { AccountService } from './account'
import { Updates, yieldsToInstall } from './updates'
import { registerReleaseNotice } from './fork-release-notice'
import { handleURL, registerAuthIPC, registerSchemes, urlFromArgv } from './auth'
import { registerOAuthLoopbackIPC } from './fork-oauth-loopback-ipc'
import { registerSpeech } from './speech'
import { registerScreenIPC } from './screen'
import { registerCloud } from './cloud'
import { registerForkCloud } from './fork-cloud/register'
import { registerForkCloudService } from './fork-cloud/service'
import { forkFeatures } from '../shared/fork-features'
import { itensMenuApp, painelSobre } from '../shared/fork-menu-app'
import { nomeVisivel } from '../shared/fork-nome-visivel'
import { coresControles, FAIXA_ALTURA, SEMAFOROS } from '../shared/janela-faixa'
import { accountConfirmed as isAccountConfirmed, canPresentSurfaces } from '../shared/fork-surfaces'
import { registerSSH } from './ssh'
import { QuitLifecycle } from './quit-lifecycle'
import { aoMudarIdiomaDoApp, definirIdiomaDoApp, idiomaDoApp, suggestedLanguage } from './fork-idioma'
import { Island, registerIslandSupported } from './island'
import { registrarCofreSeguro } from './fork-cofre-seguro/janela'
import { registerErrorReports } from './error-reports'
import { reportHostSetupFailure } from './analytics'
import { traduzir } from '../renderer/lib/fork-i18n/index.ts'
import { launchAtLogin, openedAtLogin, setLaunchAtLogin, updateLoginItem } from './login-item'
import { openExternalURL, openSystemSettingsURL, registerWindowNavigation } from './navigation'
import { origemDoChat, permiteNoChat } from './seguranca-renderer'
import { onboardingDismissal } from './fork-onboarding-dismissal'

/** macOS's user-facing computer name, else the host name. */
function computerName() {
  if (process.platform === 'darwin') {
    try {
      return execFileSync('/usr/sbin/scutil', ['--get', 'ComputerName'], { encoding: 'utf8' }).trim()
    } catch {}
  }
  return hostname()
}

// UI checks drive the renderer over the DevTools protocol (development builds only).
if (!app.isPackaged && process.env.CODYNC_REMOTE_DEBUG) app.commandLine.appendSwitch('remote-debugging-port', process.env.CODYNC_REMOTE_DEBUG)

const generalWindows = generalWindowProvider(() => BrowserWindow.getAllWindows())
registerMexeriquinhaScheme()
const host = new HostController()
registerErrorReports(host)
host.on('setupFailed', reportHostSetupFailure)
const account = new AccountService()
const updates = new Updates(host)
let chat: BrowserWindow | null = null
let pairing: BrowserWindow | null = null
let appIcon: ReturnType<typeof createAppIconController> | null = null
const quitLifecycle = new QuitLifecycle(app, autoUpdater)
const isMac = process.platform === 'darwin'

function rendererURL(page: string) {
  return !app.isPackaged && process.env.ELECTRON_RENDERER_URL
    ? `${process.env.ELECTRON_RENDERER_URL}/?window=${page}`
    : `${pathToFileURL(join(__dirname, '../renderer/index.html')).href}?window=${page}`
}

const urlDoChat = rendererURL('chat')

function conteudoDoChat(conteudo: WebContents | null): boolean {
  return !!chat && !chat.isDestroyed() && chat.webContents === conteudo
}

function pedidoDoChat(event: IpcMainInvokeEvent): BrowserWindow | null {
  const janela = chat
  if (!janela || janela.isDestroyed() || event.sender !== janela.webContents) return null
  const frame = event.senderFrame
  return origemDoChat(urlDoChat, {
    janelaPrincipal: true,
    framePrincipal: frame !== null && frame === janela.webContents.mainFrame,
    url: frame?.url,
  }) ? janela : null
}

function registrarPermissoes() {
  const sessao = session.defaultSession
  sessao.setPermissionRequestHandler((conteudo, permissao, responder, detalhes) => {
    responder(permiteNoChat(urlDoChat, {
      janelaPrincipal: conteudoDoChat(conteudo),
      framePrincipal: detalhes.isMainFrame,
      url: detalhes.requestingUrl,
    }, permissao, 'mediaTypes' in detalhes ? detalhes.mediaTypes : undefined, 'securityOrigin' in detalhes ? detalhes.securityOrigin : undefined))
  })
  sessao.setPermissionCheckHandler((conteudo, permissao, origem, detalhes) => {
    const solicitante = {
      janelaPrincipal: conteudoDoChat(conteudo),
      framePrincipal: detalhes.isMainFrame,
      url: detalhes.requestingUrl,
    }
    const tipos = detalhes.mediaType ? [detalhes.mediaType] : []
    return permiteNoChat(urlDoChat, solicitante, permissao, tipos, origem)
      && (permissao !== 'media' || detalhes.securityOrigin === undefined
        || permiteNoChat(urlDoChat, solicitante, permissao, tipos, detalhes.securityOrigin))
  })
  sessao.setDisplayMediaRequestHandler((_pedido, responder) => responder({}))
}

const background = () => (nativeTheme.shouldUseDarkColors ? '#0A0A0A' : '#FFFFFF')

function snapshot(): HostSnapshot {
  return { state: host.state, local: host.local, dev: devPort !== null, logPath: host.logPath }
}

/** The chat window: roster on the left, the conversation on the right (Grok Bot's desktop layout). */
function openChat() {
  if (chat) {
    // Shown again after being hidden (closed to the background): the launch itself is `app_opened`.
    if (!chat.isVisible() && !chat.webContents.isLoading()) chat.webContents.send('app:command', { kind: 'windowOpened' } satisfies WindowCommand)
    chat.show()
    chat.focus()
    return chat
  }
  chat = new BrowserWindow({
    width: 1100,
    height: 760,
    minWidth: 760,
    minHeight: 500,
    title: nomeVisivel(app.getName()),
    show: false,
    backgroundColor: background(),
    // MX-122 v3: a faixa do topo (29 px) é do app em todas as plataformas. No Mac, os botões da janela ficam
    // centrados nela; no Windows/Linux, os controles do sistema ficam à direita, na mesma altura, e a barra de
    // título do sistema some (não vira uma segunda barra). As cores vêm do renderer (`app:windowControls`).
    titleBarStyle: 'hidden',
    ...(isMac
      ? { trafficLightPosition: SEMAFOROS }
      : { titleBarOverlay: { height: FAIXA_ALTURA, color: background(), symbolColor: nativeTheme.shouldUseDarkColors ? '#ffffff' : '#000000' } }),
    webPreferences: { preload: join(__dirname, '../preload/index.js'), sandbox: true, contextIsolation: true },
  })
  // Tela cheia: no Mac os botões da janela somem e o seletor de Space encosta à esquerda.
  const avisarTelaCheia = () => chat?.webContents.send('app:command', { kind: 'fullScreen', on: chat.isFullScreen() } satisfies WindowCommand)
  chat.on('enter-full-screen', avisarTelaCheia)
  chat.on('leave-full-screen', avisarTelaCheia)
  // CODYNC_SHOW_INACTIVE: development and UI checks open the window without taking focus.
  chat.on('ready-to-show', () => (process.env.CODYNC_SHOW_INACTIVE ? chat?.showInactive() : chat?.show()))
  // Closing hides: the window keeps the stores (and the menu bar's data) alive.
  chat.on('close', (e) => quitLifecycle.closeToBackground(e, () => chat?.hide()))
  chat.on('closed', () => (chat = null))
  void chat.loadURL(urlDoChat)
  return chat
}

function accountConfirmed() {
  return isAccountConfirmed(account.state)
}

// Ilha e bandeja: a versão sem login as mostra direto (dono, 05/11/2026); o pareamento segue em accountConfirmed.
const surfacesAllowed = () => canPresentSurfaces(account.state, forkFeatures.cloudAccess)

function openPairing() {
  if (!accountConfirmed()) { openChat(); return }
  if (pairing) {
    pairing.webContents.send('app:command', { kind: 'reviewApprovals' } satisfies WindowCommand)
    pairing.show()
    pairing.focus()
    return
  }
  pairing = new BrowserWindow({
    width: 340,
    height: 420,
    useContentSize: true,
    resizable: false,
    title: traduzir(idiomaDoApp(), 'sistema.barra.janelaParear'),
    backgroundColor: background(),
    webPreferences: { preload: join(__dirname, '../preload/index.js'), sandbox: true, contextIsolation: true },
  })
  pairing.on('closed', () => (pairing = null))
  void pairing.loadURL(rendererURL('pairing'))
}

export function send(command: WindowCommand, show = true) {
  const window = show ? openChat() : chat
  if (show && isMac) app.focus({ steal: true })
  window?.webContents.send('app:command', command)
}

function menu() {
  // D445c: painel "Sobre" com "Mexerica" e "baseado no Codync" (shared/fork-menu-app).
  if (isMac) app.setAboutPanelOptions?.(painelSobre(app.getName(), idiomaDoApp()))
  const template: Electron.MenuItemConstructorOptions[] = [
    // ajustes-tela-01: o menu do app do Mac com "Ajustes…" (⌘,), no lugar de sempre; os outros itens são os do papel `appMenu`.
    // D443: rótulos "Sobre a/Ocultar/Encerrar Mexerica" (shared/fork-menu-app); o `app.name` técnico não muda.
    ...(isMac ? [{ label: app.name, submenu: itensMenuApp(app.getName(), () => send({ kind: 'openSettings' }), idiomaDoApp()) }] : []),
    { role: 'fileMenu' },
    { role: 'editMenu' },
    {
      label: traduzir(idiomaDoApp(), 'sistema.barra.menu.visualizar'),
      submenu: [
        { label: traduzir(idiomaDoApp(), 'sistema.barra.menu.maior'), accelerator: 'CmdOrCtrl+=', click: () => send({ kind: 'stepTextSize', up: true }, false) },
        { label: traduzir(idiomaDoApp(), 'sistema.barra.menu.menor'), accelerator: 'CmdOrCtrl+-', click: () => send({ kind: 'stepTextSize', up: false }, false) },
        { label: traduzir(idiomaDoApp(), 'sistema.barra.menu.tamanhoReal'), accelerator: 'CmdOrCtrl+0', click: () => send({ kind: 'stepTextSize', up: null }, false) },
        { type: 'separator' },
        { label: traduzir(idiomaDoApp(), 'sistema.barra.menu.novoBot'), accelerator: 'CmdOrCtrl+N', click: () => send({ kind: 'newBot' }) },
        { label: traduzir(idiomaDoApp(), 'sistema.barra.menu.buscarBots'), accelerator: 'CmdOrCtrl+F', click: () => send({ kind: 'search' }) },
        { label: traduzir(idiomaDoApp(), 'sistema.barra.menu.barraLateral'), accelerator: 'Ctrl+CmdOrCtrl+S', click: () => send({ kind: 'toggleSidebar' }, false) },
        { label: traduzir(idiomaDoApp(), 'sistema.barra.menu.fixarPainel'), accelerator: 'CmdOrCtrl+I', click: () => send({ kind: 'toggleDetails' }, false) },
        { label: traduzir(idiomaDoApp(), 'sistema.barra.menu.atualizar'), accelerator: 'CmdOrCtrl+R', click: () => send({ kind: 'refresh' }, false) },
        ...(isMac ? [] : [{ label: traduzir(idiomaDoApp(), 'marca.menu.ajustes'), accelerator: 'CmdOrCtrl+,', click: () => send({ kind: 'openSettings' }) }]),
        { type: 'separator' },
        // The dev page reload sits on ⇧⌘R, leaving ⌘R to Refresh.
        ...(app.isPackaged ? [] : [{ role: 'toggleDevTools' as const }, { role: 'forceReload' as const }]),
        { role: 'togglefullscreen' },
      ],
    },
    { role: 'windowMenu' },
  ]
  Menu.setApplicationMenu(Menu.buildFromTemplate(template))
}

function registerIPC(tray: Tray) {
  registerHostProxy()
  registerMexeriquinhaProtocol(host)
  if (appIcon) registerAppIconIPC(appIcon, () => chat, rendererURL('chat'))
  registerVozIPC(() => chat, rendererURL('chat'), (comando) => send(comando))
  ipcMain.on('app:hostPort', (e) => (e.returnValue = identity.port))
  ipcMain.on('app:name', (e) => (e.returnValue = app.getName()))
  ipcMain.on('app:scheme', (e) => (e.returnValue = identity.scheme))
  registerFileSaves()
  ipcMain.on('app:version', (e) => (e.returnValue = app.getVersion()))
  ipcMain.on('app:computerName', (e) => (e.returnValue = computerName()))
  ipcMain.on('app:suggestedLanguage', (e) => (e.returnValue = suggestedLanguage()))
  // D454/D495: o idioma efetivo do app (escolha salva; sem escolha, inglês) para o menu e a barra.
  ipcMain.on('app:setLanguage', (_e, idioma: unknown) => definirIdiomaDoApp(idioma))
  ipcMain.on('app:openedAtLogin', (e) => (e.returnValue = openedAtLogin()))
  // MX-122 v3: a faixa do topo pergunta se a janela já abriu em tela cheia e pinta os controles do sistema
  // (Windows/Linux) com as cores do tema do app.
  ipcMain.handle('app:isFullScreen', (e) => BrowserWindow.fromWebContents(e.sender)?.isFullScreen() ?? false)
  ipcMain.on('app:windowControls', (e, valor: unknown) => {
    const cores = coresControles(valor)
    const janela = BrowserWindow.fromWebContents(e.sender)
    if (isMac || !cores || !janela || janela !== chat) return
    janela.setTitleBarOverlay({ ...cores, height: FAIXA_ALTURA })
  })
  ipcMain.on('app:debugOpen', (e) => (e.returnValue = app.isPackaged ? null : (process.env.CODYNC_DEBUG_OPEN ?? null)))
  ipcMain.handle('host:snapshot', () => snapshot())
  ipcMain.on('host:install', () => void host.install())
  ipcMain.handle('host:onboardingDismissal', (event, userId: unknown, action: unknown) => {
    if (!pedidoDoChat(event)) return null
    const confirmed = isAccountConfirmed(account.state) ? account.captureCloudSession() : null
    return onboardingDismissal(prefs, [identity.environment, app.getPath('userData'), dataDir, port, buildConfig.deploymentId], confirmed?.userId, userId, action)
  })
  ipcMain.on('host:restart', () => host.restart())
  ipcMain.on('app:openExternal', (_e, url: string) => {
    void openExternalURL(url)
  })
  ipcMain.on('app:openMemoryFile', (_e, path: string) => {
    // A note's own name in a memory folder, nothing else: the renderer can't open arbitrary files through this.
    if (/[\\/](shared-memory|bot-memory)[\\/][a-z0-9-]+\.md$/.test(path) && !/(^|[\\/])\.\.([\\/]|$)/.test(path)) {
      void shell.openPath(path)
    }
  })
  ipcMain.on('app:openSettings', (_e, url: string) => {
    void openSystemSettingsURL(url)
  })
  ipcMain.on('app:copy', (_e, text: string) => clipboard.writeText(text))
  ipcMain.handle('app:pickFiles', async (e) => {
    const window = BrowserWindow.fromWebContents(e.sender)
    const result = window
      ? await dialog.showOpenDialog(window, { properties: ['openFile', 'multiSelections'] })
      : await dialog.showOpenDialog({ properties: ['openFile', 'multiSelections'] })
    return result.canceled ? [] : readFiles(result.filePaths)
  })
  ipcMain.handle('app:clipboardFiles', () => readClipboardFiles())
  ipcMain.on('app:traySummary', (_e, summary) => tray.update(summary))
  ipcMain.handle('app:showInDock', () => showInDock())
  ipcMain.handle('app:setShowInDock', (_e, on: boolean) => {
    const visible = setShowInDock(on)
    tray.hostChanged()
    return visible
  })
  ipcMain.handle('app:launchAtLogin', () => launchAtLogin())
  ipcMain.handle('app:setLaunchAtLogin', (_e, on: boolean) => setLaunchAtLogin(on))
  let apagando = false
  ipcMain.handle('app:resetAllData', async (event) => {
    const janela = pedidoDoChat(event)
    if (!janela || apagando) return
    const frame = event.senderFrame
    apagando = true
    try {
      const idioma = idiomaDoApp()
      const { response } = await dialog.showMessageBox(janela, {
        type: 'warning',
        message: traduzir(idioma, 'navegacao.conta.apagarDados'),
        detail: traduzir(idioma, 'marca.conta.apagarTudo'),
        buttons: [traduzir(idioma, 'navegacao.janela.cancelar'), traduzir(idioma, 'navegacao.conta.apagarTudo')],
        defaultId: 0,
        cancelId: 0,
        noLink: true,
      })
      if (response === 1 && pedidoDoChat(event) === janela && event.senderFrame === frame) await apagarTodosOsDados()
    } finally {
      apagando = false
    }
  })

  async function apagarTodosOsDados() {
    // Tell paired devices first: connected ones see "No access" at once, and the account drops
    // this computer instead of keeping an unreachable older copy of it.
    await host.call('unclaim', {}, 30_000).catch(() => {})
    const devices = await host.call<{ devices: { key: string }[] }>('devices').catch(() => ({ devices: [] }))
    for (const device of devices.devices) await host.call('revokeDevice', { key: device.key }).catch(() => {})
    await account.signOutAll()
    if (!(await host.uninstall())) throw new Error('The host could not be safely uninstalled. Data was preserved.')
    const { rm } = await import('node:fs/promises')
    const { dataDir } = await import('./host-controller')
    await rm(dataDir, { recursive: true, force: true })
    await rm(app.getPath('userData'), { recursive: true, force: true })
    app.relaunch()
    quitLifecycle.beginQuit()
    app.exit(0)
  }
}

if (yieldsToInstall()) {
  app.exit(0)
} else if (!app.requestSingleInstanceLock()) {
  if (!app.isPackaged) console.error(`${app.getName()} is already running. Quit it first, or run another copy beside it: npm run dev -- --name <name>`)
  app.quit()
} else {
  registerWindowNavigation(urlDoChat)
  registerSchemes(keepDockHiddenAfterLink)
  app.on('second-instance', (_e, argv) => {
    const url = urlFromArgv(argv)
    if (url) handleURL(url)
    else openChat()
  })
  // D169 N3: reabrir pelo LaunchServices põe o app no Dock antes deste evento; com Mostrar no Dock desligado, ocultar
  // de novo antes de trazer a janela (ocultar depois desativaria o app). O pacote do Mac é agente (LSUIElement, N4) e
  // não volta ao Dock; isto vale para o app sem pacote. Nada retoma o foco depois.
  app.on('activate', () => { keepDockHidden(); openChat(); appIcon?.reapply() })
  app.on('window-all-closed', () => {
    // A menu bar app: it keeps running with no window.
  })
  void app.whenReady().then(async () => {
    try { prepareKeychain(app.getPath('userData'), process.platform, safeStorage) }
    catch (error) {
      const idioma = suggestedLanguage() ?? idiomaDoApp()
      dialog.showErrorBox(traduzir(idioma, 'sistema.barra.cofre.titulo'), traduzir(idioma, `sistema.barra.cofre.${keychainFailure(error)}`))
      app.exit(1)
      return
    }
    appIcon = createAppIconController({ get: () => prefs.get('appIcon'), set: (id) => prefs.set('appIcon', { version: 1, id }) }, onDockVisibility)
    applyDockVisibility()
    // D169 S14: o pacote agente (LSUIElement) não é ativado pelo LaunchServices ao lançar, e o pedido de ativação que o
    // chat faz ao aparecer não ativa o app se a ilha (um painel que não ativa) apareceu antes com o item no Dock. Pede
    // aqui, uma vez, o mesmo pedido, antes da ilha e do chat; o pacote comum já chega ativo e o CODYNC_SHOW_INACTIVE não.
    if (isMac && !process.env.CODYNC_SHOW_INACTIVE && !app.isActive()) app.focus({ steal: true })
    updateLoginItem()
    registrarPermissoes()
    // A janela primeiro: a criação e a carga dela correm em paralelo com o resto do arranque
    // (ilha, bandeja e registros de IPC). `island:supported` é sendSync: o preload do chat o
    // chama ao carregar, então só esse handler precisa existir antes do loadURL. O resto
    // registra neste mesmo turno, antes de qualquer invoke do bundle chegar ao processo principal.
    registerIslandSupported()
    openChat()
    const island = new Island({ url: rendererURL('island'), preload: join(__dirname, '../preload/index.js'), send, openApp: openChat, changed: () => tray.hostChanged(), canPresent: surfacesAllowed, chat: () => chat, chatUrl: rendererURL('chat') })
    const tray = new Tray({ openChat, openPairing, send, host, updates, island, canPresent: surfacesAllowed })
    updates.on('change', () => tray.hostChanged())
    registerIPC(tray)
    island.register()
    registerAuthIPC()
    registerOAuthLoopbackIPC()
    registerSpeech()
    registerScreenIPC()
    registerCloud(account)
    const localSSH = registerSSH(generalWindows)
    // MX-209: o cartão seguro (janela própria, módulo do fork); o main resolve o Hub e busca a entrada.
    // SSH usa a captura com guardas do túnel (nunca o anexo cru); nuvem/relay não entra.
    registrarCofreSeguro({ pai: () => chat, gerais: generalWindows, hubs: { local: () => host.local, ssh: (computerId, current) => localSSH.captureRequest(computerId, current) }, url: rendererURL('cofre-seguro') })
    if (forkFeatures.cloudAccess) {
      const factory = registerForkCloud(account)
      if (factory) registerForkCloudService(account, factory, localSSH, generalWindows, [rendererURL('chat'), rendererURL('pairing'), rendererURL('island')], () => [chat, pairing].some(w => !!w && !w.isDestroyed() && w.isVisible() && !w.isMinimized()))
    }
    account.on('change', () => {
      if (!accountConfirmed()) pairing?.hide()
      if (!surfacesAllowed()) tray.clear()
      island.accountChanged()
    })
    account.register(generalWindows)
    updates.register(generalWindows)
    // Nível 1 de atualizações: aviso discreto de nova versão (sem download automático, forkFeatures.updates segue false).
    registerReleaseNotice({ windows: generalWindows, userDataDir: app.getPath('userData'), currentVersion: app.getVersion() })
    menu()
    aoMudarIdiomaDoApp(() => { menu(); tray.hostChanged() })
    host.on('change', () => {
      const s = snapshot()
      for (const w of generalWindows()) w.webContents.send('host:change', s)
      tray.hostChanged()
    })
    nativeTheme.on('updated', () => {
      // Only the opaque windows: a color on the transparent notch island paints its whole panel.
      for (const w of [chat, pairing]) w?.setBackgroundColor(background())
    })
    host.start()
  })
}

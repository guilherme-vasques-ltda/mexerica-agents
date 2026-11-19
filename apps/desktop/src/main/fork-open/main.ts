import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { app, BrowserWindow, ipcMain, Menu, session } from 'electron'
import type { AccountState, HostSnapshot, UpdateState } from '../../shared/ipc'
import type { SSHState } from '../../shared/ssh'
import { privateDirectory } from '../../shared/fork-private-files'
import { isOpenAsset, open, openPaths } from './isolation'

if (process.platform !== 'darwin' || !app.isPackaged || app.getName() !== open.name ||
    JSON.stringify(JSON.parse(readFileSync(join(process.resourcesPath, 'open-profile.json'), 'utf8'))) !== JSON.stringify(open)) {
  throw new Error('Pacote Open inválido.')
}

app.setName(open.name)
const { userData, homeConfig } = openPaths(app.getPath('home'), app.getPath('appData'))
app.setPath('userData', userData)
app.setPath('sessionData', userData)
app.setPath('logs', privateDirectory(join(userData, 'logs')))
app.setPath('crashDumps', privateDirectory(join(userData, 'crashDumps')))
app.setPath('temp', privateDirectory(join(homeConfig, 'tmp')))
app.commandLine.appendSwitch('disable-background-networking')

const assetRoot = join(__dirname, '../renderer')
const chatURL = pathToFileURL(join(assetRoot, 'index.html')).href + '?window=chat'
let window: BrowserWindow | null = null

function openWindow() {
  if (window) { window.showInactive(); return }
  window = new BrowserWindow({
    width: 1100, height: 760, title: open.name, show: false, titleBarStyle: 'hidden',
    webPreferences: { preload: join(__dirname, '../preload/index.js'), sandbox: true, contextIsolation: true, devTools: false },
  })
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
  window.webContents.on('will-navigate', (event) => event.preventDefault())
  window.webContents.on('will-frame-navigate', (event) => event.preventDefault())
  window.once('ready-to-show', () => window?.showInactive())
  window.on('closed', () => { window = null })
  void window.loadURL(chatURL)
}

if (!app.requestSingleInstanceLock()) {
  app.quit()
} else {
  app.on('window-all-closed', () => app.quit())
  app.on('second-instance', () => { if (app.isReady()) openWindow() })
  void app.whenReady().then(() => {
    session.defaultSession.webRequest.onBeforeRequest((details, callback) => callback({ cancel: !isOpenAsset(assetRoot, details.url) }))
    session.defaultSession.setPermissionCheckHandler(() => false)
    session.defaultSession.setPermissionRequestHandler((_contents, _permission, callback) => callback(false))
    session.defaultSession.on('will-download', (event) => event.preventDefault())
    const sync = {
      'app:name': open.name, 'app:scheme': open.scheme, 'app:hostPort': open.port, 'app:version': app.getVersion(),
      'app:computerName': open.name, 'app:suggestedLanguage': 'pt-BR', 'app:openedAtLogin': false, 'app:debugOpen': null,
      'speech:available': false, 'speech:locales': [], 'island:supported': false,
    }
    for (const [channel, value] of Object.entries(sync)) ipcMain.on(channel, (event) => { event.returnValue = value })
    const state = {
      'host:snapshot': { state: { kind: 'disabled' }, local: null, dev: false, logPath: '' } satisfies HostSnapshot,
      'ssh:state': { profiles: [], status: {}, attachments: [] } satisfies SSHState,
      'account:state': {
        ready: true, configured: false, busy: false, error: null, user: null, cloudURL: null, socialProviders: [],
        signUp: null, signUpMode: null, waitlist: false, emailCode: false, passwordReset: null, pending: null,
      } satisfies AccountState,
      'account:reconciliationState': { kind: 'none' },
      'updates:state': {
        supported: false, checking: false, availableVersion: null, downloadPercent: null, staged: false,
        installing: false, autoCheck: false, autoDownload: false, lastCheck: null, error: null,
      } satisfies UpdateState,
      'screen:state': { available: false, needsApproval: false, registered: false },
      'app:showInDock': true, 'app:launchAtLogin': false, 'appIcon:selected': null, 'app:isFullScreen': false,
    }
    for (const [channel, value] of Object.entries(state)) ipcMain.handle(channel, () => value)
    Menu.setApplicationMenu(Menu.buildFromTemplate([
      { label: open.name, submenu: [{ label: 'Ajustes…', accelerator: 'Cmd+,', click: () => window?.webContents.send('app:command', { kind: 'openSettings' }) }, { role: 'quit' }] },
      { role: 'editMenu', submenu: [{ role: 'undo' }, { role: 'redo' }, { role: 'selectAll' }] },
      { role: 'windowMenu', submenu: [{ role: 'close' }, { role: 'minimize' }] },
    ]))
    openWindow()
  }).catch(() => app.exit(1))
}

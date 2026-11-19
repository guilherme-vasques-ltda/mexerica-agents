import { join } from 'node:path'
import { app, BrowserWindow, ipcMain, nativeImage, type IpcMainInvokeEvent, type NativeImage } from 'electron'
import { APP_ICON_DEFAULT_ID, appIconAsset, type AppIconChoice, type AppIconId } from '../shared/app-icons'
import { isChatSender } from './fork-chat-sender'

const ICON_SIZE = 256

export interface AppIconStore {
  get(): unknown
  set(id: AppIconId): void
}

export interface AppIconController {
  selected(): AppIconId
  set(id: unknown): AppIconId
  /** Reaplica a escolha depois de o item do Dock ser recriado (activate, mostrar de novo). */
  reapply(): void
}

const resources = () => (app.isPackaged ? process.resourcesPath : join(__dirname, '../../resources'))

function imageFor(choice: AppIconChoice): NativeImage {
  // O padrão aplica o ícone real do pacote (resources/icon.png, o mesmo que o build usa); as opções, o PNG 256 do manifesto.
  const image = nativeImage.createFromPath(join(resources(), choice.id === APP_ICON_DEFAULT_ID ? 'icon.png' : join('app-icons', choice.file)))
  const size = image.getSize()
  const valid = choice.id === APP_ICON_DEFAULT_ID
    ? !image.isEmpty()
    : !image.isEmpty() && size.width === ICON_SIZE && size.height === ICON_SIZE
  if (!valid) {
    // texts-ok: erro interno; o IPC devolve a escolha anterior e o console fica com o motivo.
    throw new Error(`Ícone inesperado: ${choice.id}`)
  }
  return image
}

/**
 * MX-9: uma escolha por `id` do manifesto, guardada só neste aparelho. O renderer nunca manda caminho nem bytes;
 * o main resolve o PNG empacotado. Sem escolha (ou com a antiga `app-icon-v1.json`, que ninguém lê), o pacote manda.
 */
export function createAppIconController(store: AppIconStore, followDock?: (watcher: (change: 'pending' | 'settled' | 'failed') => void) => unknown): AppIconController {
  const images = new Map<AppIconId, NativeImage>()
  const cached = (choice: AppIconChoice) => {
    let image = images.get(choice.id)
    if (!image) { image = imageFor(choice); images.set(choice.id, image) }
    return image
  }
  // A preferência é versionada (`{ version: 1, id }`); uma string solta também resolve, para não travar uma leitura futura.
  const current = (): AppIconId => {
    const raw = store.get()
    const id = raw !== null && typeof raw === 'object' && (raw as { version?: unknown }).version === 1
      ? (raw as { id?: unknown }).id
      : raw
    return appIconAsset(id)?.id ?? APP_ICON_DEFAULT_ID
  }
  /** A opção fora do padrão aplicada neste processo; `null` = o ícone do pacote está valendo. */
  let appliedOption: AppIconId | null = null
  let windowsWatched = false

  const applyDock = (image: NativeImage) => {
    if (process.platform !== 'darwin' || !app.dock) return
    app.dock.setIcon(image)
  }
  const applyWindows = (image: NativeImage) => {
    if (process.platform === 'darwin') return
    for (const win of BrowserWindow.getAllWindows()) {
      if (!win.isDestroyed()) win.setIcon(image)
    }
  }
  const applyImage = (image: NativeImage) => {
    applyDock(image)
    applyWindows(image)
  }
  const apply = (choice: AppIconChoice) => applyImage(cached(choice))
  const applyDefault = (choice: AppIconChoice) => {
    if (process.platform === 'darwin') {
      // Com Assets.car (Icon Composer), setIcon com PNG sai do ícone do pacote; a imagem vazia equivale a
      // NSApp.applicationIconImage = nil e devolve o ícone do bundle (verificado no fonte do Electron 44-x-y:
      // DockSetIcon monta gfx::Image vazio e AsNSImage() devolve nil para imagem vazia).
      if (app.dock) app.dock.setIcon(nativeImage.createEmpty())
    } else {
      applyWindows(cached(choice))
    }
  }
  const watchWindows = () => {
    if (windowsWatched || process.platform === 'darwin') return
    windowsWatched = true
    app.on('browser-window-created', (_event, win) => {
      if (appliedOption && !win.isDestroyed()) win.setIcon(cached(appIconAsset(appliedOption)!))
    })
  }

  const controller: AppIconController = {
    selected: current,
    set(id) {
      const choice = appIconAsset(id)
      if (!choice) return current()
      try {
        if (choice.id === APP_ICON_DEFAULT_ID) {
          // Com uma opção aplicada neste processo, o padrão volta por setIcon (reset no mac, icon.png nas janelas);
          // sem ela, o Dock/janelas já estão no padrão.
          if (appliedOption) applyDefault(choice)
          appliedOption = null
        } else {
          apply(choice)
          appliedOption = choice.id
        }
        store.set(choice.id)
        watchWindows()
        return choice.id
      } catch (error) {
        console.error('appIcon: não foi possível aplicar o ícone:', error)
        return current()
      }
    },
    reapply() {
      if (!appliedOption) return
      try {
        if (process.platform === 'darwin') {
          const dock = app.dock
          if (!dock || dock.isVisible() === false) return
        }
        applyImage(cached(appIconAsset(appliedOption)!))
      } catch (error) {
        console.error('appIcon: não foi possível reaplicar o ícone:', error)
      }
    },
  }

  // O início aplica a preferência antes da primeira janela: no macOS, o Dock; nos demais, as janelas que nascerem.
  const initial = current()
  if (initial !== APP_ICON_DEFAULT_ID) {
    try {
      const choice = appIconAsset(initial)!
      apply(choice)
      appliedOption = choice.id
    } catch (error) {
      console.error('appIcon: preferência ignorada, fica o ícone do pacote:', error)
      store.set(APP_ICON_DEFAULT_ID)
    }
  }
  watchWindows()
  followDock?.((change) => { if (change === 'settled') controller.reapply() })
  return controller
}

/** Só o frame principal da janela de chat local conhecida (a mesma regra da voz). */
export function registerAppIconIPC(controller: AppIconController, chat: () => BrowserWindow | null, rendererURL: string) {
  ipcMain.handle('appIcon:selected', (event: IpcMainInvokeEvent) => (isChatSender(event, chat(), rendererURL) ? controller.selected() : APP_ICON_DEFAULT_ID))
  ipcMain.handle('appIcon:set', (event: IpcMainInvokeEvent, id: unknown) => (isChatSender(event, chat(), rendererURL) ? controller.set(id) : APP_ICON_DEFAULT_ID))
}

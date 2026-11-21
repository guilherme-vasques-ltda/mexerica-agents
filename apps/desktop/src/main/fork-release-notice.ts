// Nível 1 de atualizações, adaptador do Electron: liga o controlador puro (fork-release-notice-core)
// ao app — handlers IPC, janelas ativas e abertura da página da release. O `electron-updater` segue
// desligado (forkFeatures.updates false): nada aqui baixa nem instala nada.

import { join } from 'node:path'
import { app, ipcMain, shell, type BrowserWindow, type IpcMainEvent, type IpcMainInvokeEvent } from 'electron'
import type { ReleaseNoticeState } from '../shared/fork-release-notice'
import { createFileReleaseNoticeStore, createReleaseNoticeController } from './fork-release-notice-core'

export interface RegisterReleaseNoticeOptions {
  windows: () => BrowserWindow[]
  userDataDir: string
  currentVersion: string
  /** Origem do remetente (perfil Open): negado, nenhum handler age — só devolve o estado atual ou false. */
  allowed?: (event: IpcMainEvent | IpcMainInvokeEvent) => boolean
}

export function registerReleaseNotice(options: RegisterReleaseNoticeOptions) {
  const controller = createReleaseNoticeController({
    currentVersion: options.currentVersion,
    store: createFileReleaseNoticeStore(join(options.userDataDir, 'release-notice.json')),
    openExternal: (url) => shell.openExternal(url).then(() => true, () => false),
    onChange: (state: ReleaseNoticeState) => {
      for (const window of options.windows()) {
        if (!window.isDestroyed()) window.webContents.send('releaseNotice:change', state)
      }
    },
  })
  const admit = (channel: string, event: IpcMainEvent | IpcMainInvokeEvent) => {
    if (options.allowed?.(event) === false) {
      console.warn('[release-notice] denied: unauthorized sender', { channel })
      return false
    }
    return true
  }
  const estadoNegado = (): ReleaseNoticeState => ({
    supported: false, currentVersion: options.currentVersion, status: 'idle', availableVersion: null, availableTag: null,
    releaseUrl: null, dismissedVersion: null, lastCheckAt: null,
  })
  ipcMain.handle('releaseNotice:state', (event) => (admit('state', event) ? controller.getState() : estadoNegado()))
  ipcMain.handle('releaseNotice:check', (event) => (admit('check', event) ? controller.check({ manual: true }) : Promise.resolve(estadoNegado())))
  ipcMain.handle('releaseNotice:dismiss', (event) => (admit('dismiss', event) ? controller.dismiss() : Promise.resolve(estadoNegado())))
  ipcMain.handle('releaseNotice:openDownload', (event) => (admit('openDownload', event) ? controller.openDownload() : Promise.resolve(false)))
  controller.start()
  app.on('before-quit', () => controller.stop())
}

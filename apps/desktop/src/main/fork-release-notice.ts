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
  const admit = (event: IpcMainEvent | IpcMainInvokeEvent) => options.allowed?.(event) !== false
  ipcMain.handle('releaseNotice:state', () => controller.getState())
  ipcMain.handle('releaseNotice:check', (event) => (admit(event) ? controller.check({ manual: true }) : controller.getState()))
  ipcMain.handle('releaseNotice:dismiss', (event) => (admit(event) ? controller.dismiss() : controller.getState()))
  ipcMain.handle('releaseNotice:openDownload', (event) => (admit(event) ? controller.openDownload() : Promise.resolve(false)))
  controller.start()
  app.on('before-quit', () => controller.stop())
}

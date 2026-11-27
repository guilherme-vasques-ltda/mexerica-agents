import { app, ipcMain, Menu, nativeImage, Tray, type BrowserWindow, type IpcMainEvent } from 'electron'
import { aoMudarIdiomaDoApp, idiomaDoApp } from '../fork-idioma'
import type { WindowCommand } from '../../shared/ipc'
import { openTrayMenu, openTraySummary, type OpenTraySummary } from './bandeja-menu'

/**
 * Bandeja da Open (barra de menus do Mac). Sem os ícones de marca (ficam fora desta fatia), o item aparece só com a
 * inicial do nome, como título. Só o resumo enviado pela própria janela do chat é aceito.
 */
export function registerOpenTray(name: string, window: () => BrowserWindow | null, allowed: (event: IpcMainEvent) => boolean, show: () => void) {
  const tray = new Tray(nativeImage.createEmpty())
  tray.setTitle(name.slice(0, 1))
  tray.setToolTip(name)
  let summary: OpenTraySummary | null = null
  const actions = {
    show,
    quit: () => app.quit(),
    send(command: WindowCommand, front: boolean) {
      if (front) show()
      window()?.webContents.send('app:command', command)
    },
  }
  const rebuild = () => tray.setContextMenu(Menu.buildFromTemplate(openTrayMenu(summary, name, idiomaDoApp(), actions)))
  ipcMain.on('app:traySummary', (event, value: unknown) => {
    if (!allowed(event)) return
    const next = openTraySummary(value)
    if (JSON.stringify(next) === JSON.stringify(summary)) return
    summary = next
    rebuild()
  })
  rebuild()
  aoMudarIdiomaDoApp(rebuild)
}

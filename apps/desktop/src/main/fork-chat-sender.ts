import type { BrowserWindow, IpcMainEvent, IpcMainInvokeEvent } from 'electron'

// O remetente de um IPC sensível é o frame principal da janela local conhecida (antes em fork-app-icon.ts, D169;
// o seletor de ícone saiu na D457 e a voz continua usando a mesma regra). URL sozinha não concede acesso.
function isLocalRendererURL(rendererURL: string): boolean {
  try {
    const url = new URL(rendererURL)
    return url.protocol === 'file:' && url.host === ''
      || ['http:', 'https:'].includes(url.protocol) && ['localhost', '127.0.0.1', '[::1]'].includes(url.hostname)
  } catch { return false }
}

/** Somente o frame principal da janela local conhecida `win`, na URL local esperada. URL sozinha não concede acesso. */
export function isWindowSender(event: Pick<IpcMainInvokeEvent | IpcMainEvent, 'sender' | 'senderFrame'>, win: BrowserWindow | null, rendererURL: string): boolean {
  return isLocalRendererURL(rendererURL) && !!win && !win.isDestroyed() && event.sender === win.webContents && event.senderFrame === win.webContents.mainFrame
    && event.senderFrame?.url.split('#')[0] === rendererURL.split('#')[0]
}

/** Somente o frame principal da janela de chat local conhecida. URL sozinha não concede acesso. */
export function isChatSender(event: IpcMainInvokeEvent, chat: BrowserWindow | null, rendererURL: string): boolean {
  return isWindowSender(event, chat, rendererURL)
}

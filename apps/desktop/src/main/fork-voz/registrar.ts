import { app, globalShortcut, ipcMain, net, safeStorage, type BrowserWindow, type IpcMainInvokeEvent } from 'electron'
import { isChatSender } from '../fork-chat-sender'
import type { WindowCommand } from '../../shared/ipc'
import { criarAtalhoVoz } from './atalho.ts'
import { CofreVoz } from './cofre.ts'
import type { Buscar } from './provedor.ts'
import { instalarCanaisVoz, ServicoVoz } from './servico.ts'

// voz-04: gancho do main (index.ts) para a chave de voz deste aparelho. O cofre é o `safeStorage` do
// Electron; os pedidos ao provedor saem pelo `net` do main, nunca pelo renderer.

export function registerVozIPC(chat: () => BrowserWindow | null, rendererURL: string, enviar: (comando: WindowCommand) => void) {
  const buscar: Buscar = (url, init) => net.fetch(url, init)
  const avisar = () => {
    const janela = chat()
    if (!janela || janela.isDestroyed()) return
    try {
      const frame = janela.webContents.mainFrame
      // Sem dados: a tela relê por `estado()`.
      if (frame.url.split('#')[0] === rendererURL.split('#')[0]) frame.send('voz:changed')
    } catch {} // Quadro descartado: a próxima abertura relê.
  }
  const cofre = new CofreVoz({ dados: app.getPath('userData'), sistema: safeStorage, plataforma: process.platform })
  instalarCanaisVoz(ipcMain, new ServicoVoz(cofre, buscar, avisar), (event) => isChatSender(event as IpcMainInvokeEvent, chat(), rendererURL))
  // só o chat registra o atalho global; nada aqui lê o microfone.
  const atalho: ReturnType<typeof criarAtalhoVoz> = criarAtalhoVoz({
    global: globalShortcut,
    plataforma: process.platform as 'darwin' | 'win32' | 'linux',
    enviar: (comando) => {
      const janela = chat()
      // Renderer caído: o registro não fica preso sem quem o solte. Janela modal (cofre seguro) à frente: nada de abrir a chamada por baixo.
      if (janela && !janela.isDestroyed() && janela.webContents.isCrashed()) return atalho.liberar()
      if (janela && !janela.isDestroyed() && janela.getChildWindows().some((w) => w.isModal() && w.isVisible())) return
      enviar(comando)
    },
  })
  ipcMain.handle('voz:atalho', (event, pedido: unknown) => isChatSender(event as IpcMainInvokeEvent, chat(), rendererURL) ? atalho.definir(pedido) : { ok: false, motivo: 'invalido' })
  app.on('will-quit', atalho.liberar)
}

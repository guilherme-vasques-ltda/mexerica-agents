import { randomUUID } from 'node:crypto'
import { join } from 'node:path'
import { BrowserWindow, ipcMain, type IpcMainInvokeEvent, type WebContents } from 'electron'
import type { HostController } from '../host-controller'
import { isGeneralContext, markPrivateContext } from './contexts'
import { observerSignals } from './observers'
import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'

// Janela dedicada da caixa D277, sem coleta e sem aprovação real. Mostra só a indisponibilidade.
// Não é Authority, prova humana nem isolamento do sistema.
const unavailable = `data:text/html;charset=utf-8,${encodeURIComponent(`<!doctype html><meta charset="utf-8"><title>${traduzir('pt-BR', 'marca.caixa.titulo')}</title><p>${traduzir('pt-BR', 'marca.caixa.indisponivel')}</p>`)}`
export const gestureChannel = 'fork-secret-box:gesture'
const watchMs = 1000

/** Envio main → Hub só pelo cliente Local do próprio main; nunca pelo `host:call` do renderer. */
type HubClient = Pick<HostController, 'call'>
export type GestureResult = { ok: false; reason: 'recusado' | 'indisponível' }

interface Instance { window: BrowserWindow; contents: WebContents; nonce: string; hub: HubClient; loaded: string | null; close: () => void }
let current: Instance | null = null
let registered = false

/** Mesma instância, viva, ainda privada e no conteúdo carregado; navegação ou recarga já a fecharam. */
const alive = (instance: Instance) => current === instance && !instance.window.isDestroyed()
  && !instance.contents.isDestroyed() && !isGeneralContext(instance.contents) && instance.loaded !== null && instance.contents.getURL() === instance.loaded

/** Id nativo lido agora da janela viva; nunca guardado nem enviado ao renderer. Encaixe da 03b. */
function nativeWindowId(instance: Instance): string | null {
  return alive(instance) ? instance.window.getMediaSourceId() || null : null
}

/** Conferência D331 C + id nativo a cada uso. Nulo = a caixa fecha. */
async function check(instance: Instance): Promise<{ nativeId: string } | null> {
  try {
    if (!alive(instance) || (await observerSignals()).length > 0) return null
    const nativeId = nativeWindowId(instance)
    return nativeId ? { nativeId } : null
  } catch {
    return null
  }
}

/** Encaixe main → Hub. O Hub ainda não anuncia a decisão humana (Times 05b); nada sai. */
function sendToHub(_hub: HubClient, _gesture: { requestRef: string; nonce: string; nativeId: string }): GestureResult {
  return { ok: false, reason: 'indisponível' }
}

async function onGesture(event: IpcMainInvokeEvent, nonce: unknown, requestRef: unknown): Promise<GestureResult> {
  const refused: GestureResult = { ok: false, reason: 'recusado' }
  const instance = current
  if (!instance || event.sender !== instance.contents || event.senderFrame !== instance.contents.mainFrame || !alive(instance)) return refused
  if (typeof nonce !== 'string' || nonce !== instance.nonce || typeof requestRef !== 'string' || !requestRef || requestRef.length > 200) return refused
  const facts = await check(instance)
  if (!facts) { instance.close(); return refused }
  return sendToHub(instance.hub, { requestRef, nonce, nativeId: facts.nativeId })
}

/**
 * Só macOS (D331 B) e sem observador no Mac (D331 C), conferido antes e enquanto aberta.
 * Cria, marca antes de carregar ou mostrar e fecha em navegação, queda, falha ou observador.
 */
export async function openPrivateBox(hub: HubClient): Promise<BrowserWindow> {
  if (process.platform !== 'darwin') throw new Error('caixa-indisponivel: sistema')
  if ((await observerSignals()).length > 0) throw new Error('caixa-indisponivel: observador')
  current?.close()
  if (!registered) { ipcMain.handle(gestureChannel, onGesture); registered = true }
  const nonce = randomUUID()
  const window = new BrowserWindow({
    width: 420, height: 240, resizable: false, show: false, title: traduzir('pt-BR', 'marca.caixa.titulo'),
    webPreferences: {
      preload: join(__dirname, '../preload/fork-secret-box.js'), additionalArguments: [`--donna-box-nonce=${nonce}`],
      sandbox: true, contextIsolation: true, nodeIntegration: false, devTools: false,
    },
  })
  let timer: ReturnType<typeof setInterval> | null = null
  const instance: Instance = {
    window, contents: window.webContents, nonce, hub, loaded: null,
    close: () => {
      if (timer) clearInterval(timer)
      if (current === instance) current = null
      if (!window.isDestroyed()) window.destroy()
    },
  }
  current = instance
  try {
    markPrivateContext(window.webContents)
    window.setContentProtection(true)
    window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
    window.webContents.on('will-navigate', event => event.preventDefault())
    window.webContents.on('render-process-gone', instance.close)
    window.on('closed', instance.close)
    await window.loadURL(unavailable)
    instance.loaded = window.webContents.getURL()
    // Qualquer navegação posterior, inclusive recarga, encerra esta instância; outra janela é outro objeto.
    window.webContents.on('did-start-navigation', instance.close)
    if (!(await check(instance))) throw new Error('caixa-indisponivel: observador')
    timer = setInterval(() => void check(instance).then(ok => { if (!ok) instance.close() }), watchMs)
    window.show()
    return window
  } catch (error) {
    instance.close()
    throw error
  }
}

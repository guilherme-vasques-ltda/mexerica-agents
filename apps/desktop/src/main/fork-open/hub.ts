import { join } from 'node:path'
import { app, dialog, ipcMain, shell, type BrowserWindow, type IpcMainEvent, type IpcMainInvokeEvent } from 'electron'
import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'
import { FALLBACK_LANGUAGE, SOURCE_LANGUAGE, sugerirIdioma } from '../../shared/fork-idioma'
import { privateDirectory } from '../../shared/fork-private-files'
import { hostCallError } from '../../shared/host-call-error'
import type { CallError, HostSnapshot } from '../../shared/ipc'
import { open } from './isolation'
import { launchPrivateHub } from '../fork-private-hub'
import { postProvado, streamProvado } from '../fork-proved-loopback'
import { beforeDeviceLogin, createCodexPreparer, CodexPreparationError } from './fork-codex-provision'
import { descartarPlano, guardarPlano } from './plano-login'
import { isLoginPageURL, openApiPath, loginPageDeadline, openStreamPath, readOpenEventData, readOpenResponse } from './hub-transport'

const origin = `http://127.0.0.1:${open.port}`
const unavailable: CallError = { status: 0, message: traduzir(SOURCE_LANGUAGE, 'navegacao.janela.hub.parado') }

export function registerOpenHub(profile: string, temp: string, chatURL: string, window: () => BrowserWindow | null) {
  let snapshot: HostSnapshot = { state: { kind: 'starting' }, local: null, dev: false, logPath: '' }
  let connection: Awaited<ReturnType<typeof launchPrivateHub>['ready']> | null = null
  let loginDeadline = 0
  let lifetime: ReturnType<typeof launchPrivateHub> | null = null
  let closing = false
  const streams = new Map<string, AbortController>()
  const allowed = (event: IpcMainEvent | IpcMainInvokeEvent) =>
    event.sender === window()?.webContents &&
    event.senderFrame !== null && event.senderFrame === event.sender.mainFrame &&
    event.senderFrame.url === chatURL
  const prepareCodex = createCodexPreparer(profile, process.arch, async (signal) => {
    const parent = window()
    if (!parent || parent.isDestroyed() || parent.webContents.getURL() !== chatURL) return false
    const idioma = sugerirIdioma(app.getPreferredSystemLanguages()) ?? FALLBACK_LANGUAGE
    const { response } = await dialog.showMessageBox(parent, {
      type: 'question', signal, defaultId: 1, cancelId: 1, noLink: true,
      message: traduzir(idioma, 'contas.open.preparar'),
      detail: traduzir(idioma, 'contas.open.consentimento'),
      buttons: [traduzir(idioma, 'contas.open.continuar'), traduzir(idioma, 'contas.cancelar')],
    })
    return response === 0
  })
  const publish = (state: HostSnapshot['state'], local: HostSnapshot['local'] = null) => {
    snapshot = { ...snapshot, state, local }
    const target = window()?.webContents
    if (target && !target.isDestroyed() && target.getURL() === chatURL) target.send('host:change', snapshot)
  }

  async function stop(child: NonNullable<typeof lifetime>) {
    connection = null
    loginDeadline = 0
    try {
      await child.stop()
      if (lifetime === child) lifetime = null
    } finally {
      if (lifetime === child || !closing) publish({ kind: 'failed', message: unavailable.message })
    }
  }

  async function start() {
    if (closing || lifetime) return
    publish({ kind: 'starting' })
    try {
      const home = privateDirectory(join(profile, 'home'))
      const child = lifetime = launchPrivateHub({
        temp, cwd: home, dataDir: join(home, open.dataFolder),
        executable: 'mexerica-open-hub', environment: open.environment,
        command: (socket) => ({
          args: ['serve', '--only-this-mac', '--bind', '127.0.0.1', '--port', String(open.port)],
          env: { HOME: home, PATH: '/usr/bin:/bin:/usr/sbin:/sbin', TMPDIR: temp, OPEN_HUB_SOCKET: socket },
        }),
      })
      const ready = await child.ready
      ready.signal.throwIfAborted()
      connection = ready
      publish({ kind: 'running' }, { computerId: ready.computerId, baseURL: origin, token: '' })
      await child.exited
    } catch {
      if (!lifetime && !closing) publish({ kind: 'failed', message: unavailable.message })
    } finally {
      if (lifetime && !closing) await stop(lifetime).catch(() => {})
    }
  }

  ipcMain.handle('host:snapshot', (event) => allowed(event) ? snapshot : null)
  ipcMain.on('app:openExternal', (event, url: unknown) => {
    if (allowed(event) && connection && Date.now() < loginDeadline && isLoginPageURL(url)) {
      void shell.openExternal(url).catch(() => {})
    }
  })
  ipcMain.on('host:restart', (event) => { if (allowed(event) && snapshot.state.kind === 'failed') void start() })
  ipcMain.handle('host:call', async (event, baseURL: unknown, _token: unknown, method: unknown, body: unknown, timeout: unknown) => {
    const path = openApiPath(method)
    const target = connection
    if (!allowed(event) || !path || baseURL !== origin || !target || typeof timeout !== 'number' || !Number.isSafeInteger(timeout) || timeout < 1) {
      return { ok: false, error: unavailable }
    }
    if (method === 'connectorSignInFinish') descartarPlano(body)
    if (method === 'connectorSignIn') descartarPlano(null)
    try {
      await beforeDeviceLogin(method, body, target.signal, prepareCodex)
      if (!allowed(event) || connection !== target) return { ok: false, error: unavailable }
      const response = await postProvado(target, path, target.token, JSON.stringify(body ?? {}), Math.min(timeout, 180_000), { signal: target.signal })
      const text = await readOpenResponse(response)
      const status = response.statusCode ?? 0
      if (status < 200 || status >= 300) return { ok: false, error: hostCallError(status, text) }
      const value: unknown = text ? JSON.parse(text) : {}
      if (!target.signal.aborted && (method === 'donnaEngineSetupStart' || method === 'donnaEngineSetupAction')) {
        loginDeadline = loginPageDeadline(value, Date.now())
      }
      if (!target.signal.aborted && method === 'connectorSignIn') guardarPlano(value, body)
      return { ok: true, value }
    } catch (error) {
      if (error instanceof CodexPreparationError) return { ok: false, error: { status: 422, code: error.code, message: error.code } satisfies CallError }
      return { ok: false, error: unavailable }
    }
  })
  ipcMain.on('host:stream:open', (event, id: unknown, url: unknown) => {
    if (!allowed(event) || typeof id !== 'string' || !/^s[0-9]{1,16}$/.test(id) || streams.has(id)) return
    const path = openStreamPath(origin, url)
    const target = connection
    const controller = new AbortController()
    const send = (channel: string, payload: string[] | CallError | null) => {
      if (!event.sender.isDestroyed() && allowed(event)) event.sender.send(channel, id, payload)
    }
    if (!path || !target) return send('host:stream:end', unavailable)
    streams.set(id, controller)
    void (async () => {
      try {
        const signal = AbortSignal.any([target.signal, controller.signal])
        const response = await streamProvado(target, path, target.token, signal, 45_000)
        if (response.statusCode !== 200) {
          response.destroy()
          return send('host:stream:end', unavailable)
        }
        try {
          for await (const data of readOpenEventData(response)) send('host:stream:data', [data])
        } finally {
          response.destroy()
        }
        send('host:stream:end', null)
      } catch {
        send('host:stream:end', unavailable)
      } finally {
        streams.delete(id)
      }
    })()
  })
  ipcMain.on('host:stream:close', (event, id: unknown) => {
    if (allowed(event) && typeof id === 'string') streams.get(id)?.abort()
  })
  app.on('before-quit', (event) => {
    closing = true
    if (!lifetime) return
    event.preventDefault()
    void stop(lifetime).then(() => app.quit(), () => {
      closing = false
      const idioma = sugerirIdioma(app.getPreferredSystemLanguages()) ?? FALLBACK_LANGUAGE
      dialog.showErrorBox(app.getName(), traduzir(idioma, 'navegacao.janela.hub.falhaAoParar'))
    })
  })
  void start()
}

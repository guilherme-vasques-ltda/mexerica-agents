import { spawn } from 'node:child_process'
import { closeSync, constants, fstatSync, lstatSync, mkdtempSync, openSync, readFileSync, rmdirSync, unlinkSync } from 'node:fs'
import { join } from 'node:path'
import { setTimeout as delay } from 'node:timers/promises'
import { app, ipcMain, shell, type BrowserWindow, type IpcMainEvent, type IpcMainInvokeEvent } from 'electron'
import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'
import { SOURCE_LANGUAGE } from '../../shared/fork-idioma'
import { privateDirectory, validatePrivateFile } from '../../shared/fork-private-files'
import { hostCallError } from '../../shared/host-call-error'
import type { CallError, HostSnapshot } from '../../shared/ipc'
import { open } from './isolation'
import { consultaProvada, reciboConfere, type LocalHealth } from '../fork-local-health'
import { isLoginPageURL, openApiPath, loginPageDeadline, openStreamPath, readOpenEventData, readOpenResponse, requestOpenHub } from './hub-transport'

const origin = `http://127.0.0.1:${open.port}`
const unavailable: CallError = { status: 0, message: traduzir(SOURCE_LANGUAGE, 'navegacao.janela.hub.parado') }

function readToken(home: string) {
  const path = join(privateDirectory(join(home, open.dataFolder)), 'token')
  const fd = openSync(path, constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK)
  try {
    const stat = fstatSync(fd)
    validatePrivateFile(stat)
    if (stat.size !== 64) throw new Error()
    const token = readFileSync(fd, 'utf8')
    if (!/^[0-9a-f]{64}$/.test(token)) throw new Error()
    return token
  } finally {
    closeSync(fd)
  }
}

export function registerOpenHub(profile: string, temp: string, chatURL: string, window: () => BrowserWindow | null) {
  let snapshot: HostSnapshot = { state: { kind: 'starting' }, local: null, dev: false, logPath: '' }
  let connection: { socket: string; token: string; signal: AbortSignal } | null = null
  let loginDeadline = 0
  let lifetime: AbortController | null = null
  let processExit: Promise<void> | null = null
  let closing = false
  const streams = new Map<string, AbortController>()
  const allowed = (event: IpcMainEvent | IpcMainInvokeEvent) =>
    event.sender === window()?.webContents &&
    event.senderFrame !== null && event.senderFrame === event.sender.mainFrame &&
    event.senderFrame.url === chatURL
  const publish = (state: HostSnapshot['state'], local: HostSnapshot['local'] = null) => {
    snapshot = { ...snapshot, state, local }
    const target = window()?.webContents
    if (target && !target.isDestroyed() && target.getURL() === chatURL) target.send('host:change', snapshot)
  }

  async function start() {
    if (closing || lifetime) return
    const controller = new AbortController()
    lifetime = controller
    publish({ kind: 'starting' })
    let sessionDir: string | undefined
    let child: ReturnType<typeof spawn> | undefined
    const failed = () => {
      controller.abort()
      connection = null
      loginDeadline = 0
      if (sessionDir) {
        const socket = join(sessionDir, 's')
        try { if (lstatSync(socket).isSocket()) unlinkSync(socket) } catch {}
        try { rmdirSync(sessionDir) } catch {}
      }
      lifetime = null
      processExit = null
      if (!closing) publish({ kind: 'failed', message: unavailable.message })
    }
    try {
      const home = privateDirectory(join(profile, 'home'))
      sessionDir = mkdtempSync(join(temp, 'h-'))
      privateDirectory(sessionDir)
      const socket = join(sessionDir, 's')
      if (Buffer.byteLength(socket) > 103) throw new Error()
      const bin = join(process.resourcesPath, 'mexerica-open-hub')
      if (!lstatSync(bin).isFile()) throw new Error()
      child = spawn(bin, ['serve', '--only-this-mac', '--bind', '127.0.0.1', '--port', String(open.port)], {
        cwd: home, env: { HOME: home, PATH: '/usr/bin:/bin:/usr/sbin:/sbin', TMPDIR: temp, OPEN_HUB_SOCKET: socket },
        stdio: ['pipe', 'ignore', 'ignore'],
      })
      child.stdin?.on('error', () => {})
      child.on('error', () => controller.abort())
      const spawned = child
      processExit = new Promise<void>((resolve) => spawned.once('close', () => { failed(); resolve() }))
      controller.signal.addEventListener('abort', () => {
        if (!spawned.pid || spawned.exitCode !== null || spawned.signalCode !== null) return
        spawned.stdin?.end()
        const timer = setTimeout(() => spawned.kill('SIGKILL'), 10_000)
        spawned.once('close', () => clearTimeout(timer))
      }, { once: true })
      const startup = AbortSignal.any([controller.signal, AbortSignal.timeout(30_000)])
      while (!startup.aborted) {
        try {
          const stat = lstatSync(socket)
          if (!stat.isSocket() || stat.uid !== process.getuid?.() || (stat.mode & 0o077) !== 0) throw new Error()
          // A identidade do `/health` exige a prova do token local; o token não vai na chamada, e o
          // recibo da resposta é o que prova que quem respondeu tem esse token.
          const token = readToken(home)
          const prova = consultaProvada(token)
          const response = await requestOpenHub(socket, `/health${prova.query}`, '', null, startup)
          const health: unknown = JSON.parse(await readOpenResponse(response))
          if (response.statusCode !== 200 || !health || typeof health !== 'object' ||
            !('environment' in health) || health.environment !== open.environment ||
            !('computerId' in health) || typeof health.computerId !== 'string' || !health.computerId ||
            !reciboConfere(token, prova.nonce, health as LocalHealth)) {
            throw new Error()
          }
          if (startup.aborted) break
          connection = { socket, token, signal: controller.signal }
          publish({ kind: 'running' }, { computerId: health.computerId, baseURL: origin, token: '' })
          return
        } catch {
          await delay(150, undefined, { signal: startup })
        }
      }
      controller.abort()
    } catch {
      if (child) controller.abort()
      else failed()
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
    try {
      const signal = AbortSignal.any([target.signal, AbortSignal.timeout(Math.min(timeout, 180_000))])
      const response = await requestOpenHub(target.socket, path, target.token, JSON.stringify(body ?? {}), signal)
      const text = await readOpenResponse(response)
      const status = response.statusCode ?? 0
      if (status < 200 || status >= 300) return { ok: false, error: hostCallError(status, text) }
      const value: unknown = text ? JSON.parse(text) : {}
      if (!target.signal.aborted && (method === 'donnaEngineSetupStart' || method === 'donnaEngineSetupAction')) {
        loginDeadline = loginPageDeadline(value, Date.now())
      }
      return { ok: true, value }
    } catch {
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
        const response = await requestOpenHub(target.socket, path, target.token, null, signal)
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
    if (!processExit) return
    event.preventDefault()
    lifetime?.abort()
    void processExit.then(() => app.quit())
  })
  void start()
}

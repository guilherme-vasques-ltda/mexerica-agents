// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { ipcMain, type IpcMainEvent, type IpcMainInvokeEvent, type WebContents } from 'electron'
import type { CallError } from '../shared/ipc'
import { hostCallError } from '../shared/host-call-error'
import { noteAnalyticsSetting } from './analytics'
import { isGeneralContext } from './fork-secret-box/contexts'
import { postProvado, streamProvado } from './fork-proved-loopback'

// Loopback HTTP + SSE for the renderer. The host has no CORS (it's a loopback API for local
// apps), so requests leave from the main process.

const streams = new Map<WebContents, Map<string, AbortController>>()
type PrivateTarget = { socketPath: string; token: string; signal: AbortSignal }
type PrivateHub = {
  origin: string
  target: () => PrivateTarget | null
  allowed: (event: IpcMainEvent | IpcMainInvokeEvent) => boolean
}

export function cancelHostStreams() {
  for (const owned of streams.values()) for (const controller of owned.values()) controller.abort()
  streams.clear()
}

function isLoopback(url: string) {
  try {
    const host = new URL(url).hostname
    return host === '127.0.0.1' || host === 'localhost' || host === '[::1]'
  } catch {
    return false
  }
}

async function call(baseURL: string, token: string, method: string, body: unknown, timeoutMs: number, target?: PrivateTarget, atual?: () => boolean): Promise<unknown> {
  if (!target && !isLoopback(baseURL)) throw { status: 0, message: 'Not a loopback host.' } satisfies CallError
  // A prova do token e a credencial passam pela mesma conexão, uma atrás da outra (ver
  // fork-proved-loopback): a troca de listener entre as duas não existe.
  let res
  try {
    res = await postProvado(target ?? baseURL, `/api/${method}`, target?.token ?? token, JSON.stringify(body ?? {}), timeoutMs, { signal: target?.signal, atual })
  } catch {
    throw { status: 0, message: "Can't reach your computer. Is it on and connected to the internet?" } satisfies CallError
  }
  res.setEncoding('utf8')
  let text = ''
  for await (const chunk of res) text += chunk as string
  if (res.statusCode !== 200) throw hostCallError(res.statusCode ?? 0, text)
  const value: unknown = text ? JSON.parse(text) : {}
  // The main process keeps the analytics setting for when there's no host (`host_setup_failed`).
  noteAnalyticsSetting(baseURL, method, value)
  return value
}

function stream(sender: WebContents, id: string, url: string, token: string, target?: PrivateTarget, current?: () => boolean) {
  const controller = new AbortController()
  const owned = streams.get(sender) ?? new Map<string, AbortController>()
  const previous = owned.get(id)
  owned.set(id, controller)
  streams.set(sender, owned)
  previous?.abort()
  const active = () => streams.get(sender) === owned && owned.get(id) === controller
  const send = (channel: string, payload: unknown) => {
    if (active() && isGeneralContext(sender) && (!current || current())) sender.send(channel, id, payload)
  }
  void (async () => {
    // Idle timeout, not a total one: the host pings every 15 s, so silence this long means it's gone.
    let idle: NodeJS.Timeout | null = null
    const arm = () => {
      if (idle) clearTimeout(idle)
      idle = setTimeout(() => controller.abort(), 45_000)
    }
    // Eventos que chegam no mesmo trecho do stream saem numa mensagem IPC só: o renderer
    // aplica o lote inteiro numa tarefa e o store faz um changed() por lote, não por linha.
    const pending: string[] = []
    const flush = () => {
      if (pending.length === 0) return
      send('host:stream:data', pending.splice(0, pending.length))
    }
    try {
      if (!target && !isLoopback(url)) throw { status: 0, message: 'Not a loopback host.' }
      arm()
      const origem = new URL(url)
      const signal = target ? AbortSignal.any([target.signal, controller.signal]) : controller.signal
      const res = await streamProvado(target ?? origem.origin, `${origem.pathname}${origem.search}`, target?.token ?? token, signal, 45_000, current)
      if (res.statusCode !== 200) {
        res.destroy()
        throw { status: res.statusCode ?? 0, message: `Host error ${res.statusCode}` }
      }
      const onAbort = () => res.destroy()
      controller.signal.addEventListener('abort', onAbort, { once: true })
      const decoder = new TextDecoder()
      let buffer = ''
      for await (const value of res) {
        arm()
        buffer += decoder.decode(value as Buffer, { stream: true })
        let nl: number
        while ((nl = buffer.indexOf('\n')) >= 0) {
          const line = buffer.slice(0, nl).replace(/\r$/, '')
          buffer = buffer.slice(nl + 1)
          if (line.startsWith('data:')) pending.push(line.slice(5).trimStart())
        }
        flush()
        if (controller.signal.aborted) break
      }
      flush()
      send('host:stream:end', null)
    } catch (error) {
      flush()
      if (active()) {
        const e = error as Partial<CallError>
        send('host:stream:end', controller.signal.aborted
          ? { status: 0, message: "Can't reach your computer." }
          : { status: e.status ?? 0, message: e.message ?? "Can't reach your computer." })
      }
    } finally {
      if (idle) clearTimeout(idle)
      if (active()) {
        owned.delete(id)
        if (!owned.size) streams.delete(sender)
      }
    }
  })()
}

export function registerHostProxy(privateHub?: PrivateHub) {
  const unavailable: CallError = { status: 0, message: 'Contexto indisponível.' }
  ipcMain.handle('host:call', (e, baseURL: string, token: string, method: string, body: unknown, timeoutMs: number) => {
    if (!isGeneralContext(e.sender)) return { ok: false, error: unavailable }
    const isPrivate = privateHub?.origin === baseURL
    const target = isPrivate ? privateHub.target() : null
    if (isPrivate && (!target || !privateHub.allowed(e) || typeof method !== 'string' || !/^[a-zA-Z][a-zA-Z0-9]*$/.test(method) ||
        !Number.isSafeInteger(timeoutMs) || timeoutMs < 1)) return { ok: false, error: unavailable }
    const current = target ? () => privateHub!.allowed(e) && privateHub!.target() === target : undefined
    return call(baseURL, token, method, body, target ? Math.min(timeoutMs, 180_000) : timeoutMs, target ?? undefined, current).then(
      (value) => ({ ok: true, value }),
      (error: CallError) => ({ ok: false, error }),
    )
  })
  ipcMain.on('host:stream:open', (e, id: string, url: string, token: string) => {
    if (!isGeneralContext(e.sender)) return
    let parsed: URL
    try { parsed = new URL(url) } catch { return }
    if (privateHub?.origin === parsed.origin) {
      const target = privateHub.target()
      if (!privateHub.allowed(e)) return
      if (!target || !/^s[0-9]{1,16}$/.test(id) ||
          (parsed.pathname !== '/events' && !/^\/term\/[^/]+$/.test(parsed.pathname))) {
        e.sender.send('host:stream:end', id, unavailable)
        return
      }
      stream(e.sender, id, url, token, target, () => privateHub.allowed(e))
      return
    }
    stream(e.sender, id, url, token)
  })
  ipcMain.on('host:stream:close', (e, id: string) => {
    if (!isGeneralContext(e.sender)) return
    const owned = streams.get(e.sender)
    if (!owned) return
    const controller = owned.get(id)
    owned.delete(id)
    if (!owned.size) streams.delete(e.sender)
    controller?.abort()
  })
}

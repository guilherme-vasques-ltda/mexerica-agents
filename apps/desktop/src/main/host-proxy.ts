// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { ipcMain, type WebContents } from 'electron'
import type { CallError } from '../shared/ipc'
import { hostCallError } from '../shared/host-call-error'
import { noteAnalyticsSetting } from './analytics'
import { isGeneralContext } from './fork-secret-box/contexts'

// Loopback HTTP + SSE for the renderer. The host has no CORS (it's a loopback API for local
// apps), so requests leave from the main process.

const streams = new Map<string, AbortController>()

function isLoopback(url: string) {
  try {
    const host = new URL(url).hostname
    return host === '127.0.0.1' || host === 'localhost' || host === '[::1]'
  } catch {
    return false
  }
}

async function call(baseURL: string, token: string, method: string, body: unknown, timeoutMs: number): Promise<unknown> {
  if (!isLoopback(baseURL)) throw { status: 0, message: 'Not a loopback host.' } satisfies CallError
  let res: Response
  try {
    res = await fetch(`${baseURL}/api/${method}`, {
      method: 'POST',
      headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
      body: JSON.stringify(body ?? {}),
      signal: AbortSignal.timeout(timeoutMs),
    })
  } catch {
    throw { status: 0, message: "Can't reach your computer. Is it on and connected to the internet?" } satisfies CallError
  }
  const text = await res.text()
  if (!res.ok) throw hostCallError(res.status, text)
  const value: unknown = text ? JSON.parse(text) : {}
  // The main process keeps the analytics setting for when there's no host (`host_setup_failed`).
  noteAnalyticsSetting(baseURL, method, value)
  return value
}

function stream(sender: WebContents, id: string, url: string, token: string) {
  const controller = new AbortController()
  streams.set(id, controller)
  const send = (channel: string, payload: unknown) => {
    if (isGeneralContext(sender)) sender.send(channel, id, payload)
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
      if (!isLoopback(url)) throw { status: 0, message: 'Not a loopback host.' }
      arm()
      const res = await fetch(url, {
        headers: { Authorization: `Bearer ${token}`, Accept: 'text/event-stream' },
        signal: controller.signal,
      })
      if (res.status !== 200 || !res.body) throw { status: res.status, message: `Host error ${res.status}` }
      const reader = res.body.getReader()
      const decoder = new TextDecoder()
      let buffer = ''
      for (;;) {
        const { value, done } = await reader.read()
        if (done) break
        arm()
        buffer += decoder.decode(value, { stream: true })
        let nl: number
        while ((nl = buffer.indexOf('\n')) >= 0) {
          const line = buffer.slice(0, nl).replace(/\r$/, '')
          buffer = buffer.slice(nl + 1)
          if (line.startsWith('data:')) pending.push(line.slice(5).trimStart())
        }
        flush()
      }
      flush()
      send('host:stream:end', null)
    } catch (error) {
      flush()
      const e = error as Partial<CallError>
      send('host:stream:end', controller.signal.aborted && streams.has(id)
        ? { status: 0, message: "Can't reach your computer." }
        : { status: e.status ?? 0, message: e.message ?? "Can't reach your computer." })
    } finally {
      if (idle) clearTimeout(idle)
      streams.delete(id)
    }
  })()
}

export function registerHostProxy() {
  ipcMain.handle('host:call', (e, baseURL: string, token: string, method: string, body: unknown, timeoutMs: number) => {
    if (!isGeneralContext(e.sender)) return { ok: false, error: { status: 0, message: 'Contexto indisponível.' } satisfies CallError }
    return call(baseURL, token, method, body, timeoutMs).then(
      (value) => ({ ok: true, value }),
      (error: CallError) => ({ ok: false, error }),
    )
  })
  ipcMain.on('host:stream:open', (e, id: string, url: string, token: string) => {
    if (!isGeneralContext(e.sender)) return
    stream(e.sender, id, url, token)
  })
  ipcMain.on('host:stream:close', (e, id: string) => {
    if (!isGeneralContext(e.sender)) return
    const controller = streams.get(id)
    streams.delete(id)
    controller?.abort()
  })
}

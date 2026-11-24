import { hostCallError } from '../../shared/host-call-error.ts'
import type { CallError, CodyncBridge, HostSnapshot } from '../../shared/ipc.ts'
import type { RemoteHub } from './controller.ts'
import type { WorkspaceContext } from './workspace-session.ts'
import type { Workspace } from './workspaces.ts'

interface HubBinding {
  origin: string
  authorize(request: Request): Promise<HeadersInit>
  fetch(request: Request): Promise<Response>
}

type Callbacks = Parameters<RemoteHub['confirm']>[1]
interface Connection {
  origin: string
  binding: HubBinding
  callbacks: Callbacks
  token: string
  context?: WorkspaceContext
  computerId?: string
}

const transportError = (status: number, code: string): CallError => ({ status, code, message: code })
const offline = transportError(0, 'hub_offline')
const inactive = transportError(409, 'workspace_inactive')
const invalid = transportError(502, 'invalid_hub_response')
const unauthorized = transportError(401, 'auth_invalid')
const streamIdleMs = 45_000
const maxFrameLength = 8 * 1024 * 1024
const terminalTypes = new Set(['workspace_revoked', 'role_changed', 'device_revoked'])

function abortable<T>(signal: AbortSignal, run: () => Promise<T>): Promise<T> {
  signal.throwIfAborted()
  return new Promise<T>((resolve, reject) => {
    const abort = () => reject(signal.reason)
    signal.addEventListener('abort', abort, { once: true })
    Promise.resolve().then(() => {
      signal.throwIfAborted()
      return run()
    }).then(resolve, reject).finally(() => signal.removeEventListener('abort', abort))
  })
}

async function request(c: Connection, url: URL, init: RequestInit, signal: AbortSignal) {
  const unsigned = new Request(url, { ...init, signal, credentials: 'omit', redirect: 'error', cache: 'no-store', referrerPolicy: 'no-referrer' })
  const proof = await abortable(signal, async () => new Headers(await c.binding.authorize(unsigned.clone())))
    .catch(() => { throw signal.aborted ? offline : unauthorized })
  const headers = new Headers(unsigned.headers)
  for (const [key, value] of proof) headers.set(key, value)
  try {
    return await abortable(signal, () => c.binding.fetch(new Request(unsigned, { headers })))
  } catch {
    throw offline
  }
}

async function readText(response: Response, signal: AbortSignal) {
  try { return await abortable(signal, () => response.text()) } catch { throw offline }
}

function readContext(event: Record<string, unknown>): WorkspaceContext {
  const c = event.workspace as Partial<WorkspaceContext> | null | undefined
  if (!c || c.schema !== 1 || typeof c.hubId !== 'string' || !c.hubId.trim()
    || (c.kind !== 'personal' && c.kind !== 'team') || typeof c.workspaceId !== 'string'
    || typeof c.userId !== 'string' || !c.userId || !['owner', 'admin', 'member'].includes(c.role ?? '')
    || typeof c.workspaceEpoch !== 'number' || !Number.isSafeInteger(c.workspaceEpoch) || c.workspaceEpoch < 0
    || !Array.isArray(c.capabilities) || !c.capabilities.every((value) => typeof value === 'string')) throw invalid
  return c as WorkspaceContext
}

async function readEvents(c: Connection, url: URL, signal: AbortSignal, onData: (data: string) => boolean) {
  const timer = new AbortController()
  const scoped = AbortSignal.any([signal, timer.signal])
  let timeout: ReturnType<typeof setTimeout> | undefined
  const reset = () => {
    clearTimeout(timeout)
    timeout = setTimeout(() => timer.abort(), streamIdleMs)
  }
  reset()
  let awaitingHello = url.pathname === '/events'
  let reader: ReadableStreamDefaultReader<Uint8Array> | undefined
  try {
    const response = await request(c, url, { headers: { Accept: 'text/event-stream' } }, scoped)
    if (!response.ok) throw hostCallError(response.status, await readText(response, scoped))
    if (!response.body || !/^text\/event-stream(?:\s*;|$)/i.test(response.headers.get('content-type') ?? '')) throw invalid
    reader = response.body.getReader()
    const input = reader
    const decoder = new TextDecoder()
    let buffer = ''
    let data = ''
    let skipLF = false
    for (;;) {
      const { value, done } = await abortable(scoped, () => input.read()).catch(() => { throw offline })
      if (done) return
      if (!awaitingHello) reset()
      let chunk = decoder.decode(value, { stream: true })
      if (skipLF && chunk) {
        if (chunk.startsWith('\n')) chunk = chunk.slice(1)
        skipLF = false
      }
      buffer += chunk
      let end: number
      while ((end = buffer.search(/[\r\n]/)) >= 0) {
        const line = buffer.slice(0, end)
        const cr = buffer[end] === '\r'
        skipLF = cr && end === buffer.length - 1
        buffer = buffer.slice(end + (cr && buffer[end + 1] === '\n' ? 2 : 1))
        if (line.startsWith('data:')) data += line.slice(5).replace(/^ /, '') + '\n'
        if (data.length > maxFrameLength) throw invalid
        if (!line && data) {
          scoped.throwIfAborted()
          const helloAccepted = onData(data.slice(0, -1))
          if (awaitingHello && helloAccepted) { awaitingHello = false; reset() }
          data = ''
        }
      }
      if (buffer.length > maxFrameLength) throw invalid
    }
  } catch (error) {
    if (timer.signal.aborted) throw offline
    throw error
  } finally {
    clearTimeout(timeout)
    void reader?.cancel().catch(() => {})
  }
}

export function createRemoteHub(resolve: (workspace: Workspace) => HubBinding | null): RemoteHub & { host: CodyncBridge['host'] } {
  let current: Connection | undefined
  const listeners = new Set<(snapshot: HostSnapshot) => void>()
  const snapshot = (): HostSnapshot => {
    const c = current
    const local = c?.computerId && c.callbacks.acceptsResponse()
      ? { computerId: c.computerId, baseURL: c.origin, token: c.token }
      : null
    return { state: { kind: local ? 'running' : 'disabled' }, local, dev: false, logPath: '' }
  }
  const publish = () => { for (const listener of listeners) listener(snapshot()) }
  const connection = (origin: string, token: string) => {
    const c = current
    if (!c?.context || origin !== c.origin || token !== c.token || !c.callbacks.acceptsResponse()) throw inactive
    return c as Connection & { context: WorkspaceContext }
  }
  const fail = (c: Connection, error: unknown) => {
    if (c.callbacks.signal.aborted) return
    if (error === offline) c.callbacks.offline()
    else if (error === invalid) c.callbacks.accessLost(403, 'workspace_forbidden')
    else if (typeof (error as CallError | undefined)?.status === 'number') {
      const { status, code } = error as CallError
      if (status === 409 && code === 'workspace_epoch_mismatch') c.callbacks.recoverContext()
      else c.callbacks.accessLost(status, code)
    }
  }
  const inspect = (c: Connection, event: Record<string, unknown>) => {
    const context = c.context!
    const { type, workspaceId, workspaceEpoch } = event
    if (typeof type === 'string' && terminalTypes.has(type)) {
      if (workspaceId !== context.workspaceId || typeof workspaceEpoch !== 'number' || !Number.isSafeInteger(workspaceEpoch)
        || workspaceEpoch < 0) throw invalid
      if (type === 'device_revoked') {
        if (workspaceEpoch >= context.workspaceEpoch) c.callbacks.accessLost(401, type)
      } else if (workspaceEpoch > context.workspaceEpoch) c.callbacks.recoverContext()
      return false
    }
    if (workspaceId !== undefined && workspaceId !== context.workspaceId) throw invalid
    if (type === 'hello') {
      const hello = readContext(event)
      if (hello.hubId !== context.hubId || hello.workspaceId !== context.workspaceId || hello.userId !== context.userId
        || hello.kind !== context.kind || event.hostId !== context.hubId) throw invalid
      if (hello.workspaceEpoch !== context.workspaceEpoch) {
        c.callbacks.recoverContext()
        return false
      }
      if (hello.role !== context.role) throw invalid
      return true
    }
    if (typeof workspaceEpoch !== 'number' || !Number.isSafeInteger(workspaceEpoch) || workspaceEpoch < 0) return false
    if (workspaceEpoch > context.workspaceEpoch) c.callbacks.recoverContext()
    return workspaceEpoch === context.workspaceEpoch
  }
  const parseEvent = (data: string): Record<string, unknown> => {
    let event: unknown
    try { event = JSON.parse(data) } catch { throw invalid }
    if (!event || typeof event !== 'object' || typeof (event as Record<string, unknown>).type !== 'string') throw invalid
    return event as Record<string, unknown>
  }

  return {
    async confirm(workspace, callbacks) {
      const binding = resolve(workspace)
      if (!binding || typeof binding.authorize !== 'function' || typeof binding.fetch !== 'function') throw inactive
      const url = new URL(binding.origin)
      if (url.protocol !== 'https:' || url.username || url.password || url.href !== `${url.origin}/`) throw inactive
      const c: Connection = { origin: url.origin, binding, callbacks, token: crypto.randomUUID() }
      current = c
      callbacks.onDispose(() => {
        if (current === c) { current = undefined; publish() }
      })
      return new Promise<WorkspaceContext>((resolve, reject) => {
        void readEvents(c, new URL('/events', url), callbacks.signal, (data) => {
          const event = parseEvent(data)
          if (c.context) { inspect(c, event); return false }
          if (typeof event.type === 'string' && terminalTypes.has(event.type) && event.workspaceId === workspace.id) {
            if (typeof event.workspaceEpoch !== 'number' || !Number.isSafeInteger(event.workspaceEpoch) || event.workspaceEpoch < 0) throw invalid
            if (event.type === 'device_revoked') callbacks.accessLost(401, 'device_revoked')
            else callbacks.recoverContext()
            return false
          }
          if (event.type !== 'hello' || typeof event.computerId !== 'string' || !event.computerId.trim()) throw invalid
          const context = readContext(event)
          if (event.hostId !== context.hubId || !callbacks.acceptContext(context)) throw invalid
          c.context = context
          c.computerId = event.computerId
          publish()
          resolve(context)
          return true
        }).then(() => { throw offline }).catch((error: unknown) => {
          reject(error)
          fail(c, error)
        })
      })
    },
    host: {
      snapshot: async () => snapshot(),
      onChange(listener) { listeners.add(listener); return () => { listeners.delete(listener) } },
      install() {},
      restart() {},
      async call(origin, token, method, body, timeoutMs) {
        const c = connection(origin, token)
        if (!/^[A-Za-z0-9_-]+$/.test(method) || (body != null && (typeof body !== 'object' || Array.isArray(body)))) throw invalid
        const signal = AbortSignal.any([c.callbacks.signal, AbortSignal.timeout(timeoutMs)])
        try {
          const response = await request(c, new URL(`/api/${method}`, origin), {
            method: 'POST', headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ ...body as object, workspaceEpoch: c.context.workspaceEpoch }),
          }, signal)
          const text = await readText(response, signal)
          if (c.callbacks.signal.aborted) throw inactive
          if (signal.aborted) throw offline
          if (!response.ok) throw hostCallError(response.status, text)
          try { return text ? JSON.parse(text) : {} } catch { throw invalid }
        } catch (error) {
          if (c.callbacks.signal.aborted) throw inactive
          const reason = signal.aborted ? offline : error
          fail(c, reason)
          throw reason
        }
      },
      stream(raw, token, onData, onEnd) {
        const stop = new AbortController()
        let ended = false
        const finish = (error: CallError | null) => {
          if (ended || stop.signal.aborted) return
          ended = true
          onEnd(error)
        }
        void (async () => {
          const url = new URL(raw)
          const c = connection(url.origin, token)
          if (url.username || url.password || url.hash || !/^\/(?:events|term\/[^/]+)$/.test(url.pathname)) throw inactive
          url.searchParams.set('workspaceEpoch', String(c.context.workspaceEpoch))
          const signal = AbortSignal.any([c.callbacks.signal, stop.signal])
          try {
            let needsHello = url.pathname === '/events'
            await readEvents(c, url, signal, (data) => {
              const event = parseEvent(data)
              const allowed = inspect(c, event)
              if (needsHello && allowed) {
                if (event.type !== 'hello') throw invalid
                needsHello = false
              }
              if (allowed && !signal.aborted && c.callbacks.acceptsResponse()) onData(data)
              return allowed && event.type === 'hello'
            })
            if (!signal.aborted) {
              if (url.pathname === '/events') throw offline
              finish(null)
            }
          } catch (error) {
            if (signal.aborted) return
            fail(c, error)
            finish(typeof (error as CallError | undefined)?.status === 'number' ? error as CallError : invalid)
          }
        })().catch(() => finish(inactive))
        return () => stop.abort()
      },
    },
  }
}

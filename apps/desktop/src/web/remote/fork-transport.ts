import type { CallError, CodyncBridge, HostSnapshot } from '../../shared/ipc.ts'
import { FALLBACK_LANGUAGE, sugerirIdioma } from '../../shared/fork-idioma.ts'
import { tradutor } from '../../renderer/lib/fork-i18n/index.ts'
import { WEB_UNAVAILABLE } from '../platform.ts'
import type { RemoteHub } from './controller.ts'
import { markDevice, remoteDevice } from './device.ts'
import { createPartitions, partitionId } from './partitions.ts'
import { channelOrigin, openRemoteChannel, type ChannelEnd, type ChannelTarget } from './socket.ts'
import { hubOffline, type WorkspaceContext } from './workspace-session.ts'
import type { Workspace } from './workspaces.ts'

export interface HubBinding extends ChannelTarget {
  userId: string
  getSessionToken(): Promise<string | null>
}

const texto = tradutor(sugerirIdioma(globalThis.navigator?.languages ?? []) ?? FALLBACK_LANGUAGE)
const unavailable: CallError = { status: 0, code: hubOffline.code, message: texto('botCreate.erroCriacao.offline') }
const inactive: CallError = { status: 0, code: WEB_UNAVAILABLE, message: texto('web.login.indisponivel') }
const timeoutMs = 8_000
const object = (v: unknown): Record<string, unknown> =>
  v && typeof v === 'object' && !Array.isArray(v) ? v as Record<string, unknown> : {}

function publicError(error: { status: number; code?: string }, send = false): CallError {
  const status = [0, 400, 401, 403, 404, 409, 429, 500, 503].includes(error.status) ? error.status : 500
  if (status === 0) return unavailable
  if (status === 401 || status === 403) {
    const code = status === 401 && error.code === 'auth_invalid' ? 'auth_invalid'
      : status === 401 && error.code === 'device_unpaired' ? 'device_unpaired'
      : status === 403 && error.code === 'workspace_forbidden' ? 'workspace_forbidden'
      : status === 403 && error.code === 'role_insufficient' ? 'role_insufficient' : 'access_denied'
    return { status, code, message: texto('conversa.arquivar.falha.notAllowed') }
  }
  const code = status === 409 && error.code === 'workspace_epoch_mismatch' ? 'workspace_epoch_mismatch'
    : status === 409 ? 'conflict' : status === 404 ? 'not_found' : status === 429 ? 'busy' : 'request_failed'
  return { status, code, message: texto(send ? 'spaces.compose.falhou' : 'web.login.erro.geral') }
}

function readContext(raw: unknown, binding: HubBinding): WorkspaceContext {
  const c = object(raw)
  if (c.schema !== 1 || c.hubId !== binding.hubId || c.userId !== binding.userId
    || !Array.isArray(c.capabilities) || !c.capabilities.every((cap) => typeof cap === 'string')) throw inactive
  return c as unknown as WorkspaceContext
}

export function createRemoteHub(
  resolve: (workspace: Workspace) => HubBinding | null,
  deps = { device: remoteDevice, mark: markDevice, connect: openRemoteChannel, now: Date.now },
): RemoteHub & { host: CodyncBridge['host']; rendererContext(): { storage: Storage; host: CodyncBridge['host'] } | undefined } {
  type Callbacks = Parameters<RemoteHub['confirm']>[1]
  type Link = ReturnType<typeof openRemoteChannel>
  type Connection = {
    origin: string
    token: string
    link: Link
    context: WorkspaceContext
    callbacks: Callbacks
    authorized: () => boolean
    storage: Storage
  }
  let current: Connection | undefined
  const partitions = createPartitions(deps.now)
  const listeners = new Set<(snapshot: HostSnapshot) => void>()
  const snapshot = (): HostSnapshot => {
    const c = current
    const local = c?.authorized() ? { computerId: c.link.computerId, baseURL: c.origin, token: c.token } : null
    return { state: { kind: local ? 'running' : 'disabled' }, local, dev: false, logPath: '' }
  }
  const publish = () => { for (const listener of listeners) listener(snapshot()) }
  const connection = (origin: string, token: string) => {
    const c = current
    if (!c?.authorized() || c.origin !== origin || c.token !== token) throw inactive
    return c
  }
  const recover = (c: Connection, code: unknown) => {
    if (c.authorized() && code === 'workspace_epoch_mismatch') c.callbacks.recoverContext()
  }

  const readReceipt = async (c: Connection, method: string, b: Record<string, unknown>, timeout: number) => {
    const contextual = method === 'donnaConversationSend'
    const chat = contextual ? b.conversationId : b.botId
    const nonce = b.clientNonce
    if (!c.authorized() || typeof chat !== 'string' || typeof nonce !== 'string' || !nonce
      || typeof b.text !== 'string') return
    const target = contextual ? { conversationId: chat }
      : { botId: chat, ...(typeof b.conversationId === 'string' && { conversationId: b.conversationId }), sessionId: b.sessionId ?? chat }
    const result = await c.link.call(contextual ? 'donnaConversationHistory' : 'history',
      { ...target, ...(typeof b.hubId === 'string' && { hubId: b.hubId }), ...(typeof b.spaceId === 'string' && { spaceId: b.spaceId }),
        sendReceipt: { nonce, text: b.text, replyTo: b.replyTo, revision: b.expectedRevision },
        workspaceEpoch: c.context.workspaceEpoch }, timeout, c.authorized).catch(() => undefined)
    if (!c.authorized() || result?.kind !== 'ok') return
    const page = object(result.value)
    if (!Array.isArray(page.entries)) return
    const context = object(page.context)
    if (contextual && (page.hubId !== b.hubId || context.conversationId !== chat || context.spaceId !== b.spaceId)) return
    const text = b.text.trim()
    const entry = page.entries.find((raw: unknown) => {
      const e = object(raw), data = object(e.data)
      return e.kind === 'user' && typeof e.id === 'string' && e.botId === chat
        && e.sessionId === (contextual ? chat : b.sessionId ?? chat)
        && (!b.spaceId || !e.spaceId || b.spaceId === e.spaceId)
        && data.clientNonce === nonce && data.text === text
    })
    return entry ? (contextual ? { hubId: page.hubId, context: page.context, entry, replayed: true } : { entry }) : undefined
  }

  const remote: ReturnType<typeof createRemoteHub> = {
    async confirm(workspace, callbacks, pairingCode) {
      const binding = resolve(workspace)
      if (!binding) throw inactive
      const origin = channelOrigin(binding.origin).replace(/^wss:/, 'https:')
      const signal = callbacks.signal
      const scope = partitionId(binding.hubId, workspace.id, binding.userId)
      const device = await deps.device(scope, pairingCode !== undefined)
      signal.throwIfAborted()
      if (device?.state === 'revoked') {
        callbacks.accessLost(401, 'device_revoked')
        throw inactive
      }
      if (!device) {
        callbacks.needsPairing()
        throw inactive
      }
      const getToken = async () => {
        const token = await binding.getSessionToken()
        signal.throwIfAborted()
        if (!token) throw inactive
        return token
      }
      if (pairingCode !== undefined) {
        const pair = deps.connect(binding, device.signer, signal, true, () => {})
        try {
          await pair.ready
          const result = await pair.call('pair', { code: pairingCode, platform: 'web', name: 'Web', sessionToken: await getToken() }, timeoutMs)
          if (result.kind !== 'ok' || object(result.value).computerId !== pair.computerId) throw inactive
        } catch {
          if (!signal.aborted) callbacks.needsPairing()
          throw inactive
        } finally {
          pair.close()
        }
      }
      signal.throwIfAborted()
      let context: WorkspaceContext | undefined
      let expiresAt = 0
      let renewal: ReturnType<typeof setTimeout> | undefined
      let hardExpiry: ReturnType<typeof setTimeout> | undefined
      const onEnd = (reason?: ChannelEnd) => {
        if (signal.aborted) return
        if (reason?.code === 'device_revoked') {
          partitions.revokeHub(binding.hubId)
          void deps.mark(scope, 'revoked').catch(() => {})
          callbacks.accessLost(401, 'device_revoked')
        } else if (reason?.code === 'device_unpaired') {
          void deps.mark(scope, 'new').catch(() => {})
          callbacks.needsPairing()
        } else if (context && (reason?.code === 'role_changed' || reason?.code === 'workspace_revoked')) {
          partitions.cut(context, reason.epoch)
          if (reason.code === 'workspace_revoked') callbacks.accessLost(403, 'workspace_forbidden')
          else callbacks.offline('stopped')
        } else {
          callbacks.offline(reason?.code === 'transport_lost' ? 'transport' : 'stopped')
        }
      }
      const link = deps.connect(binding, device.signer, signal, false, onEnd)
      callbacks.onDispose(() => {
        clearTimeout(renewal)
        clearTimeout(hardExpiry)
        link.close()
        if (current?.link === link) { current = undefined; publish() }
      })
      const authorized = () => link.isOpen() && !signal.aborted && deps.now() < expiresAt && callbacks.acceptsResponse()
      const authenticate = async () => {
        const token = await getToken()
        const result = await link.call('sessionAuthenticate', { sessionToken: token }, timeoutMs,
          () => !signal.aborted && (!expiresAt || deps.now() < expiresAt))
        if (result.kind !== 'ok') throw inactive
        const verified = object(result.value)
        const next = verified.expiresAt
        if (verified.userId !== binding.userId || typeof next !== 'number' || !Number.isSafeInteger(next)
          || next <= Math.max(expiresAt, deps.now()) || (context && verified.role !== context.role)) throw inactive
        expiresAt = next
        const remaining = next - deps.now()
        clearTimeout(hardExpiry)
        hardExpiry = setTimeout(() => link.close(), Math.min(remaining, 2_147_483_647))
        renewal = setTimeout(() => {
          void authenticate().then(() => { if (authorized() && current?.link === link) partitions.authenticate(current.context) }).catch(() => link.close())
        }, Math.max(1, Math.min(remaining * 0.75, remaining - 5_000, 2_147_483_647)))
        return verified.role
      }
      try {
        await link.ready
        const role = await authenticate()
        const result = await link.call('workspaceContext', {}, timeoutMs, () => deps.now() < expiresAt && !signal.aborted)
        if (result.kind !== 'ok' || signal.aborted) throw inactive
        const candidate = readContext(result.value, binding)
        if (candidate.role !== role || !callbacks.acceptContext(candidate)) throw inactive
        context = candidate
        link.bindEpoch(candidate.workspaceEpoch)
        const hello = await link.call('hello', { corte: 1, workspaceEpoch: candidate.workspaceEpoch }, timeoutMs, authorized)
        if (hello.kind !== 'ok' || !authorized()) throw inactive
        const storage = partitions.authenticate(candidate)
        if (device.state === 'new') void deps.mark(scope, 'paired').catch(() => {})
        current = { origin, token: crypto.randomUUID(), link, context, callbacks, authorized, storage }
        publish()
        return context
      } catch (error) {
        link.close()
        throw error
      }
    },
    rendererContext() {
      const c = current
      if (!c?.authorized()) return undefined
      const allowed = () => current === c && c.authorized()
      const empty: HostSnapshot = { state: { kind: 'disabled' }, local: null, dev: false, logPath: '' }
      const storage = c.storage
      return {
        storage: {
          get length() { return allowed() ? storage.length : 0 },
          key: (index: number) => allowed() ? storage.key(index) : null,
          getItem: (key: string) => allowed() ? storage.getItem(key) : null,
          setItem(key: string, value: string) { if (allowed()) storage.setItem(key, value) },
          removeItem(key: string) { if (allowed()) storage.removeItem(key) },
          clear() { if (allowed()) storage.clear() },
        },
        host: {
          ...remote.host,
          snapshot: async () => allowed() ? snapshot() : empty,
          onChange(listener) {
            if (!allowed()) return () => {}
            const stop = remote.host.onChange((next) => { if (allowed()) listener(next) })
            c.callbacks.onDispose(stop)
            return stop
          },
          call: (origin, token, method, body, timeout) => allowed()
            ? remote.host.call(origin, token, method, body, timeout) : Promise.reject(inactive),
          stream: (url, token, onData, onEnd) => {
            if (allowed()) return remote.host.stream(url, token, onData, onEnd)
            let cancelled = false
            queueMicrotask(() => { if (!cancelled) onEnd(inactive) })
            return () => { cancelled = true }
          },
        },
      }
    },
    host: {
      snapshot: async () => snapshot(),
      onChange(listener) { listeners.add(listener); return () => { listeners.delete(listener) } },
      install() {},
      restart() {},
      async call(origin, token, method, body, requestedTimeout) {
        const c = connection(origin, token)
        if (method === 'pair' || method === 'sessionAuthenticate' || (body != null && (typeof body !== 'object' || Array.isArray(body)))) throw inactive
        const b = object(body)
        const send = method === 'send' || method === 'donnaConversationSend'
        const result = await c.link.call(method, { ...b, workspaceEpoch: c.context.workspaceEpoch }, requestedTimeout, c.authorized)
          .catch(() => undefined)
        if (!c.authorized()) throw unavailable
        if (result?.kind === 'ok') return result.value
        const error = result?.kind === 'err' ? publicError(result, send) : unavailable
        if (send && !('retryEntryId' in b) && (result?.kind === 'unknown' || error.code === 'request_failed')) {
          const saved = await readReceipt(c, method, b, requestedTimeout)
          if (saved && c.authorized()) return saved
        }
        recover(c, error.code)
        throw error
      },
      stream(raw, token, onData, onEnd) {
        let active = true
        let sub: Awaited<ReturnType<Link['subscribe']>>
        const finish = (error: CallError | null) => {
          if (!active) return
          active = false
          onEnd(error)
        }
        void (async () => {
          const url = new URL(raw)
          const c = connection(url.origin, token)
          if (url.pathname !== '/events' || url.hash || url.username || url.password) throw inactive
          const body = {
            ...(url.searchParams.has('spaceId') ? { spaceId: url.searchParams.get('spaceId') } : {}),
            ...(url.searchParams.has('client') ? { client: url.searchParams.get('client') } : {}),
            ...(url.searchParams.has('since') ? { since: Number(url.searchParams.get('since')) } : {}),
            workspaceEpoch: c.context.workspaceEpoch,
          }
          const allowed = () => active && c.authorized()
          sub = await c.link.subscribe('events', body, (event) => {
            if (allowed()) onData(JSON.stringify(event))
          }, (error) => {
            const safe = error ? publicError(error) : null
            if (safe) recover(c, safe.code)
            finish(safe)
          }, allowed)
          if (!active) sub?.cancel()
          else if (!sub) finish(unavailable)
        })().catch(() => finish(unavailable))
        return () => { active = false; sub?.cancel() }
      },
    },
  }
  return remote
}

// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { sessionDeletedOf, type SessionDeleted } from '../../shared/fork-session-delete'
import type { SpaceEvent, SpacesSnapshot } from '../../shared/fork-spaces'
import type { OriginReport } from '@shared/guard-hub'
import type { CallError } from '@shared/ipc'
import {
  normalizeBot,
  normalizeScreen,
  type AccessRequest,
  type Bot,
  type CloudStatus,
  type Entry,
  type Hello,
  type ScreenState,
  type Usage,
} from '@shared/models'

/** How a client reaches a host API. Only loopback today: the computer's own host or an SSH tunnel. */
export type HostRoute = 'loopback' | 'direct' | 'relay'

export class HostError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly code?: string,
  ) {
    super(message)
  }

  static unreachable() {
    return new HostError(0, "Can't reach your computer. Is it on and connected to the internet?")
  }

  /** The link failed, not the request: the same call can work once the computer is reachable again. */
  get isTransient() {
    return this.status === 0
  }
}

export interface HostTransport {
  call(method: string, body: unknown, timeoutMs: number): Promise<unknown>
  /** Each element is one event's JSON (what SSE puts after `data:`). */
  stream(path: string, onData: (line: string) => void, onEnd: (error: HostError | null) => void): () => void
}

export class LoopbackTransport implements HostTransport {
  constructor(
    readonly baseURL: string,
    private token: string,
  ) {}

  async call(method: string, body: unknown, timeoutMs: number) {
    try {
      return await window.codync.host.call(this.baseURL, this.token, method, body, timeoutMs)
    } catch (e) {
      const error = e as CallError
      throw new HostError(error.status ?? 0, error.status === 401 ? "This device isn't allowed on that computer anymore." : error.message, error.code)
    }
  }

  stream(path: string, onData: (line: string) => void, onEnd: (error: HostError | null) => void) {
    return window.codync.host.stream(`${this.baseURL}${path}`, this.token, onData, (error) =>
      onEnd(error ? new HostError(error.status, error.message) : null),
    )
  }
}

export type HostEvent =
  | SessionDeleted
  | SpaceEvent
  | { type: 'hello'; hostId: string; rev: number; spaces?: SpacesSnapshot; spaceId?: string; spacesContract?: number; conversations?: import('../../shared/fork-spaces').SpaceConversation[]; usage: Usage; screen: ScreenState | null; botGroups?: import('../../shared/bot-groups').BotGroupsSnapshot }
  | { type: 'botGroups'; spaceId: string; snapshot: import('../../shared/bot-groups').BotGroupsSnapshot }
  | { type: 'bot'; bot: Bot }
  | { type: 'botDeleted'; id: string; rev: number }
  /** Open: o /clear apagou a conversa no Hub; o app esvazia a janela carregada dela (fork/open_contexto). */
  | { type: 'chatCleared'; chatId: string; rev: number; cutoff: number }
  | { type: 'entry'; entry: Entry }
  | { type: 'usage'; usage: Usage }
  | { type: 'screen'; screen: ScreenState; spaceId?: string }
  | { type: 'accessRequests'; requests: AccessRequest[] }
  | { type: 'cloud'; cloud: CloudStatus }
  | { type: 'functionModelsChanged'; revision: number }
  | { type: 'backgroundSpaceSelectionsChanged'; spaceId: string }
  | { type: 'backgroundHistoryChanged' }
  | { type: 'donnaAttentionChanged' | 'donnaControlTowerChanged'; hubId: string; spaceId: string }
  /** whatsapp R2: só o aviso; o app relê `donnaWhatsapp` (código e QR nunca vêm no evento). R3/R5c: com o ID do bot,
   a caixa do WhatsApp dele relê a lista e a conversa aberta. */
  | { type: 'whatsappChanged'; botId?: string }
  | { type: 'resync' }
  /** Times (fork/times): só o space e a revisão; o app relê o snapshot pelo RPC do dono. */
  | { type: 'times'; spaceId: string; revision: number }
  /** Saúde do Hub (doctor): sinal mínimo de invalidação por space (sem dados privados). */
  | { type: 'donnaDoctorChanged'; hubId: string; spaceId: string }
  /** Sessões (fork/sessoes): uma sessão nova ou mudada (renomear); o app guarda só as conversas abertas. */
  | { type: 'session'; session: import('../views/fork/sessoes/estado').HubSession }
  /** An event this app version couldn't read (host newer/older than the app). */
  | { type: 'undecodable'; kind: string }

export function parseEvent(line: string): HostEvent | null {
  let raw: Record<string, unknown>
  try {
    raw = JSON.parse(line) as Record<string, unknown>
  } catch {
    return null
  }
  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return null
  const type = raw.type
  switch (type) {
    case 'hello':
      return {
        type,
        hostId: (raw.hostId as string) ?? '',
        rev: (raw.rev as number) ?? 0,
        usage: (raw.usage as Usage) ?? { providers: [] },
        screen: normalizeScreen(raw.screen as ScreenState | undefined),
        spaces: raw.spaces as SpacesSnapshot | undefined,
        spaceId: raw.spaceId as string | undefined,
        spacesContract: raw.spacesContract as number | undefined,
        conversations: raw.conversations as import('../../shared/fork-spaces').SpaceConversation[] | undefined,
        botGroups: raw.botGroups as import('../../shared/bot-groups').BotGroupsSnapshot | undefined,
      }
    case 'spaces':
      return raw.snapshot ? { type, spaceId: raw.spaceId as string, rev: raw.rev as number, snapshot: raw.snapshot as SpacesSnapshot } : null
    case 'conversation':
      return raw.conversation ? { type, spaceId: raw.spaceId as string, rev: raw.rev as number, conversation: raw.conversation as import('../../shared/fork-spaces').SpaceConversation } : null
    case 'botGroups':
      return raw.snapshot && typeof raw.spaceId === 'string' ? { type, spaceId: raw.spaceId, snapshot: raw.snapshot as import('../../shared/bot-groups').BotGroupsSnapshot } : null
    case 'bot': {
      const bot = raw.bot as (Bot & { deleted?: boolean }) | undefined
      if (!bot || typeof bot.id !== 'string') return { type: 'undecodable', kind: 'bot' }
      if (bot.deleted) return { type: 'botDeleted', id: bot.id, rev: bot.rev ?? 0 }
      return { type, bot: normalizeBot(bot) }
    }
    case 'entry': {
      const entry = raw.entry as Entry | undefined
      if (!entry || typeof entry.id !== 'string' || typeof entry.botId !== 'string') return { type: 'undecodable', kind: 'entry' }
      return { type, entry: { ...entry, data: entry.data ?? {} } }
    }
    case 'usage':
      return raw.usage ? { type, usage: raw.usage as Usage } : null
    case 'screen': {
      const screen = normalizeScreen(raw.screen as ScreenState | undefined)
      // R1: o Hub diz de que space é a projeção; a vista de um space confere antes de ler.
      return screen ? { type, screen, ...(typeof raw.spaceId === 'string' ? { spaceId: raw.spaceId } : {}) } : null
    }
    case 'accessRequests':
      return { type, requests: (raw.requests as AccessRequest[]) ?? [] }
    case 'cloud':
      return raw.cloud ? { type, cloud: raw.cloud as CloudStatus } : null
    case 'functionModelsChanged':
      return typeof raw.revision === 'number' && Number.isSafeInteger(raw.revision) && raw.revision >= 0 ? { type, revision: raw.revision } : null
    case 'backgroundSpaceSelectionsChanged':
      return typeof raw.spaceId === 'string' && raw.spaceId.length > 0 ? { type, spaceId: raw.spaceId } : null
    case 'backgroundHistoryChanged':
      return { type }
    case 'donnaAttentionChanged':
    case 'donnaDoctorChanged':
    case 'donnaControlTowerChanged': {
      const { hubId, spaceId } = raw
      return typeof hubId === 'string' && hubId.trim() === hubId && hubId.length > 0
        && typeof spaceId === 'string' && spaceId.trim() === spaceId && spaceId.length > 0
        ? { type, hubId, spaceId } : null
    }
    case 'whatsappChanged':
      return typeof raw.botId === 'string' ? { type, botId: raw.botId } : { type }
    case 'resync':
      return { type }
    case 'times':
      return typeof raw.spaceId === 'string' && typeof raw.revision === 'number' ? { type, spaceId: raw.spaceId, revision: raw.revision } : null
    case 'chatCleared':
      return typeof raw.chatId === 'string' && raw.chatId && Number.isSafeInteger(raw.rev) && (raw.rev as number) > 0 && Number.isSafeInteger(raw.cutoff) && (raw.cutoff as number) >= 0
        ? { type, chatId: raw.chatId, rev: raw.rev as number, cutoff: raw.cutoff as number }
        : { type: 'undecodable', kind: 'chatCleared' }
    case 'sessionDeleted':
      return sessionDeletedOf(raw) ?? { type: 'undecodable', kind: 'sessionDeleted' }
    case 'session': {
      const session = raw.session as import('../views/fork/sessoes/estado').HubSession | undefined
      return session && typeof session.id === 'string' && typeof session.chatId === 'string' ? { type, session } : null
    }
    default:
      return null
  }
}

/** Typed host calls (host/src/api/); anything else goes through `call`. */
export class HostClient {
  readFile(entryId: string, fileId: string, offset: number): Promise<{ data: string; size: number }> {
    return this.call('readFile', { entryId, fileId, offset })
  }

  private knownHubId: string | undefined
  constructor(readonly transport: HostTransport, readonly spaceId?: string, readonly boundHubId?: string) {}

  inSpace(spaceId: string) { return new HostClient(this.transport, spaceId, this.boundHubId ?? this.knownHubId) }

  call<T>(method: string, body: unknown = {}, timeoutMs = 20_000): Promise<T> {
    if (this.spaceId) {
      const object = body as Record<string, unknown>
      if (object.spaceId !== undefined && object.spaceId !== this.spaceId) return Promise.reject(new Error('cross_space_reference'))
      if (this.boundHubId && object.hubId !== undefined && object.hubId !== this.boundHubId) return Promise.reject(new Error('cross_hub_reference'))
      body = { ...object, spaceId: this.spaceId, ...(this.boundHubId ? { hubId: this.boundHubId } : {}) }
    }
    return this.transport.call(method, body, timeoutMs) as Promise<T>
  }

  /** Catch-up since `since`, then live updates. */
  events(since: number, client: string, onEvent: (e: HostEvent) => void, onEnd: (error: HostError | null) => void) {
    const query = new URLSearchParams({ since: String(since), client, ...(this.spaceId ? { spaceId: this.spaceId } : {}) })
    let active = true
    const stop = this.transport.stream(`/events?${query}`, (line) => {
      if (!active) return
      if (this.spaceId) {
        try {
          const raw = JSON.parse(line) as { type?: string; spaceId?: string; hostId?: string }
          if (raw.type === 'hello' && this.boundHubId && raw.hostId !== this.boundHubId) { active = false; onEnd(new HostError(409, 'hub_context_mismatch')); return }
          if (raw.type !== 'spaces' && raw.type !== 'resync' && raw.type !== 'botGroups' && raw.spaceId !== this.spaceId) return
        } catch { return }
      }
      const event = parseEvent(line)
      if (event) onEvent(event)
    }, (error) => {
      // R4: a stream ends once. After its end (or its closer) no line of it reaches a consumer, whatever the transport.
      if (!active) return
      active = false
      onEnd(error)
    })
    return () => { active = false; stop() }
  }

  hello() {
    return this.call<Hello>('hello').then(h => this.acceptHello(h))
  }

  /** O hello interno da store também vincula clientes derivados ao Hub confirmado. Não concede autoridade. */
  acceptHello(h: Hello) {
    if (this.boundHubId && h.hostId !== this.boundHubId) throw new HostError(409, 'hub_context_mismatch')
    if (this.spaceId && (h.spacesContract !== 1 || h.spaceId !== this.spaceId)) throw new HostError(409, 'space_context_unavailable')
    this.knownHubId = h.hostId
    return { ...h, backends: h.backends ?? [], screen: normalizeScreen(h.screen) ?? undefined }
  }

  async history(botId: string, beforeSeq: number, limit = 100) {
    return (await this.call<{ entries: Entry[] }>('history', { botId, beforeSeq, limit })).entries
  }

  async botConversation(botId: string, peerId: string) {
    return (await this.call<{ entries: Entry[] }>('botConversation', { botId, peerId })).entries
  }

  /** `pasted`: what the user pasted rather than typed, which memory never takes as their own words. `replyTo`: the
   * chat message this one replies to. */
  /** `sessionId`: uma extra (UI-01); só passado depois de o Hub anunciar `send` (store/fork-sessions.ts). */
  async send(botId: string, text: string, clientNonce: string, attachments: string[] | null, pasted: string[] = [], replyTo: string | null = null, contentOrigin?: OriginReport, sessionId?: string) {
    return (await this.call<{ entry: Entry }>('send', { botId, text, clientNonce, attachments, ...(contentOrigin ? { contentOrigin } : {}), ...(pasted.length ? { pasted } : {}), ...(replyTo ? { replyTo } : {}), ...(sessionId ? { sessionId } : {}) })).entry
  }

  /** Bytes per `upload` call: base64 of it stays well under the channel's 1 MiB message. */
  static uploadChunk = 384 * 1024

  async upload(botId: string, uploadId: string, name: string, data: Uint8Array) {
    let offset = 0
    do {
      const end = Math.min(offset + HostClient.uploadChunk, data.length)
      await this.call('upload', { botId, uploadId, name, data: toBase64(data.subarray(offset, end)), offset, done: end === data.length }, 60_000)
      offset = end
    } while (offset < data.length)
  }

  async readUpload(botId: string, uploadId: string) {
    const parts: Uint8Array[] = []
    let size = 0
    for (;;) {
      const res = await this.call<{ data: string; size: number }>('readUpload', { botId, uploadId, offset: size }, 60_000)
      const chunk = fromBase64(res.data)
      if (!chunk.length) break
      parts.push(chunk)
      size += chunk.length
      if (size >= res.size) break
    }
    const out = new Uint8Array(size)
    let at = 0
    for (const p of parts) {
      out.set(p, at)
      at += p.length
    }
    return out
  }
}

export function toBase64(bytes: Uint8Array) {
  let binary = ''
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
  return btoa(binary)
}

export function fromBase64(text: string) {
  const binary = atob(text)
  const out = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i)
  return out
}

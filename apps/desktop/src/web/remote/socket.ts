import { nobleBackend } from '../../shared/relay-protocol/backend-noble.ts'
import { createChannel } from '../../shared/relay-protocol/channel.ts'
import { createProtocolInternals, type AsyncSigner, type TranscriptContext } from '../../shared/relay-protocol/protocol-internal.ts'
import { browserSocket, DEFAULT_TIMINGS, deviceRelayURL, type SocketLike } from '../../shared/relay-protocol/relay-socket.ts'

const { protocol, createSigningSlot } = createProtocolInternals(nobleBackend)
const { Channel } = createChannel(protocol)
const disconnected = () => new Error('hub_offline')
const timeout = 8_000
const maxMessage = 400_000

type Terminal = { code: 'device_revoked' | 'device_unpaired' }
  | { code: 'device_revoked' | 'workspace_revoked' | 'role_changed'; epoch: number }
export type ChannelEnd = Terminal | { code: 'transport_lost' }
const transientCloseCodes = new Set([1000, 1001, 1005, 1006, 1011, 1012, 1013, 4011])
export interface ChannelTarget {
  origin: string
  relayOrigin?: string
  hubId: string
  signKey: string
}

export function channelOrigin(raw: string): string {
  const url = new URL(raw)
  if (url.protocol !== 'wss:' || url.href !== `${url.origin}/`) throw new Error('remote_invalid_origin')
  return url.origin
}

export function openRemoteChannel(
  target: ChannelTarget,
  device: AsyncSigner,
  signal: AbortSignal,
  pair: boolean,
  ended: (reason?: ChannelEnd) => void,
  connect: (url: string, headers?: Record<string, string>) => SocketLike =
    (url, headers) => headers ? browserSocket(url, headers) : new WebSocket(url),
) {
  signal.throwIfAborted()
  const pin = protocol.unb64url(target.signKey, 32)
  if (!pin || !target.hubId || target.hubId.includes('\0')) throw new Error('remote_invalid_hub')
  const computerId = protocol.computerIdOf(pin)
  const relay = !pair && target.relayOrigin ? channelOrigin(target.relayOrigin) : null
  const url = relay
    ? deviceRelayURL(relay.replace(/^wss:/, 'https:'), computerId)
    : new URL('/channel?v=1', channelOrigin(target.origin))
  const attempt = createSigningSlot().begin(device, pin, timeout)
  let socket: SocketLike | undefined
  let opened = false
  let started = false
  let lastHeard = 0
  let heartbeat: ReturnType<typeof setInterval> | undefined
  let channel: InstanceType<typeof Channel> | undefined
  let transcript: TranscriptContext | undefined
  let epoch: number | undefined
  let closed = false
  let waiting = true
  let resolve!: () => void
  let reject!: (error: Error) => void
  const ready = new Promise<void>((yes, no) => { resolve = yes; reject = no })

  const close = (reason?: ChannelEnd) => {
    if (closed) return
    closed = true
    clearTimeout(deadline)
    clearInterval(heartbeat)
    signal.removeEventListener('abort', abort)
    attempt.cancel()
    channel?.close()
    socket?.close()
    if (waiting) reject(disconnected())
    if (!signal.aborted) ended(reason)
  }
  const abort = () => close()
  const transportLost = () => close({ code: 'transport_lost' })
  const deadline = setTimeout(transportLost, timeout)
  signal.addEventListener('abort', abort, { once: true })
  const send = (frame: object) => {
    if (closed || signal.aborted || !opened || !socket) throw disconnected()
    socket.send(JSON.stringify(frame))
  }

  const hello = () => {
    if (started) return
    started = true
    void attempt.sendHello((hello) => { send({ ...hello, ...(pair ? { pair: true } : {}) }); return undefined })
      .then((sent) => { if (!sent) close() })
  }
  const attach = (href: string, headers?: Record<string, string>) => {
    if (closed || signal.aborted) return
    socket = connect(href, headers)
    socket.addEventListener('open', () => {
      opened = true
      if (!relay) { hello(); return }
      lastHeard = Date.now()
      heartbeat = setInterval(() => {
        if (Date.now() - lastHeard >= DEFAULT_TIMINGS.silenceMs) { transportLost(); return }
        try { send({ t: 'ping' }) } catch { transportLost() }
      }, DEFAULT_TIMINGS.pingMs)
    })
    socket.addEventListener('close', ({ code }) => {
      if (transientCloseCodes.has(code)) transportLost()
      else close()
    })
    socket.addEventListener('error', transportLost)
    socket.addEventListener('message', ({ data }) => {
      if (closed) return
      try {
        if (typeof data !== 'string' || data.length > maxMessage) return close()
        const frame = JSON.parse(data)
        if (relay) {
          lastHeard = Date.now()
          if (frame.t === 'presence') {
            if (frame.online === true && !started) hello()
            else if (typeof frame.online === 'boolean') transportLost()
            else close()
            return
          }
          if (frame.t === 'pong') return
        }
        if (frame.t === 'reject') {
          if (transcript && epoch !== undefined) return close(attempt.verifyCut(transcript, epoch, frame) ?? undefined)
          const code = attempt.verifyRefusal(frame)
          return close(code ? { code } : undefined)
        }
        if (frame.t === 'welcome' && !channel && frame.v === 1) {
          const accepted = attempt.receiveWelcome(frame, (keys, context) => {
            transcript = context
            channel = new Channel(keys, { send })
            clearTimeout(deadline)
            waiting = false
            resolve()
            return undefined
          })
          if (!accepted) close()
          return
        }
        if (frame.t === 'f' && channel) {
          channel.receive(frame)
          return
        }
        close()
      } catch {
        close()
      }
    })
  }
  if (relay) {
    void attempt.sendUpgrade(url, (href, header) => {
      attach(href, { 'Codync-Sig': header })
      return undefined
    }).then((sent) => { if (!sent) close() })
  } else {
    try { attach(url.href) } catch { close() }
  }

  return {
    ready, computerId, close: () => close(),
    isOpen: () => !!channel?.isOpen && !closed,
    bindEpoch(value: number) { epoch = value },
    async call(method: string, body: object, timeoutMs: number, authorized: () => boolean = () => true) {
      if (!channel || !transcript || closed || !authorized()) throw disconnected()
      const pop = await attempt.possession(transcript, target.hubId, method)
      try {
        return await channel.call(method, body, timeoutMs, () => !closed && authorized(), pop)
      } catch {
        close()
        throw disconnected()
      }
    },
    async subscribe(
      kind: 'events', body: object, event: (value: unknown) => void,
      end: (error: { status: number; message: string; code?: string } | null) => void,
      authorized: () => boolean,
    ) {
      if (!channel || !transcript || closed || !authorized()) return null
      const pop = await attempt.possession(transcript, target.hubId, `sub:${kind}`)
      if (closed || !authorized()) return null
      try {
        return channel.subscribe(kind, body, event, end, pop)
      } catch {
        close()
        throw disconnected()
      }
    },
  }
}

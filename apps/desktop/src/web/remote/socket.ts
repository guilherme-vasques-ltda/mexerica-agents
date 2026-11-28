import { nobleBackend } from '../../shared/relay-protocol/backend-noble.ts'
import { createChannel } from '../../shared/relay-protocol/channel.ts'
import { createProtocolInternals, type AsyncSigner, type TranscriptContext } from '../../shared/relay-protocol/protocol-internal.ts'

const { protocol, createSigningSlot } = createProtocolInternals(nobleBackend)
const { Channel } = createChannel(protocol)
const disconnected = () => new Error('hub_offline')
const timeout = 8_000
const maxMessage = 400_000

export type Terminal = { code: 'device_revoked' | 'device_unpaired' }
  | { code: 'device_revoked' | 'workspace_revoked' | 'role_changed'; epoch: number }
export interface ChannelTarget {
  origin: string
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
  ended: (reason?: Terminal) => void,
  connect: (url: string) => WebSocket = (url) => new WebSocket(url),
) {
  signal.throwIfAborted()
  const pin = protocol.unb64url(target.signKey, 32)
  if (!pin || !target.hubId || target.hubId.includes('\0')) throw new Error('remote_invalid_hub')
  const computerId = protocol.computerIdOf(pin)
  const socket = connect(new URL('/channel?v=1', channelOrigin(target.origin)).href)
  const attempt = createSigningSlot().begin(device, pin, timeout)
  let channel: InstanceType<typeof Channel> | undefined
  let transcript: TranscriptContext | undefined
  let epoch: number | undefined
  let closed = false
  let waiting = true
  let resolve!: () => void
  let reject!: (error: Error) => void
  const ready = new Promise<void>((yes, no) => { resolve = yes; reject = no })

  const close = (reason?: Terminal) => {
    if (closed) return
    closed = true
    clearTimeout(deadline)
    signal.removeEventListener('abort', abort)
    attempt.cancel()
    channel?.close()
    socket.close()
    if (waiting) reject(disconnected())
    if (!signal.aborted) ended(reason)
  }
  const abort = () => close()
  const deadline = setTimeout(abort, timeout)
  signal.addEventListener('abort', abort, { once: true })
  const send = (frame: object) => {
    if (closed || signal.aborted || socket.readyState !== WebSocket.OPEN) throw disconnected()
    socket.send(JSON.stringify(frame))
  }

  socket.addEventListener('open', () => {
    void attempt.sendHello((hello) => { send({ ...hello, ...(pair ? { pair: true } : {}) }); return undefined })
      .then((sent) => { if (!sent) close() })
  })
  socket.addEventListener('close', abort)
  socket.addEventListener('error', abort)
  socket.addEventListener('message', ({ data }) => {
    if (closed) return
    try {
      if (typeof data !== 'string' || data.length > maxMessage) return close()
      const frame = JSON.parse(data)
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
      kind: 'events' | 'term', body: object, event: (value: unknown) => void,
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

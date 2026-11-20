import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'
import { concat, type ChannelKeys, type DeviceSigner, type Protocol } from './protocol.ts'
import { decodeFrame, FRAME_MAX_WIRE, FRAMES_CAPABILITY, FrameCodecError, isFrameBytes, type BrowserFrame } from './browser-frames.ts'

/**
 * Canal E2E do aparelho (docs/reference/remote-relay.md §6.2–§6.5) depois do aperto de mão:
 * quadros cifrados com contador por direção, RPC interno e assinaturas de eventos.
 *
 * Contrato `donna-acesso/v1` §4.9: o `id` do request e a entrega do quadro são correlação de
 * transporte, não aceite durável. Uma chamada enviada que não recebe resposta antes de o canal
 * cair (ou do prazo) termina como `unknown` e **nunca** é reenviada aqui; quem chama decide.
 *
 * Isomórfico: as primitivas de cripto entram por `createChannel(proto)`; não há backend
 * padrão (o app desktop liga `backend-node` no calço `main/fork-cloud/channel.ts`).
 */

export type CallResult =
  | { kind: 'ok'; value: unknown }
  | { kind: 'err'; status: number; message: string; code?: string }
  /** Enviada; o Hub pode ou não ter executado. Não repetir automaticamente uma mutação. */
  | { kind: 'unknown'; reason: 'closed' | 'timeout' }
  /** Nada saiu deste Mac. */
  | { kind: 'notSent'; reason: 'notReady' | 'busy' | 'tooLarge' }

export interface Subscription {
  readonly id: number
  cancel(): void
}

/** Para onde vão os quadros cifrados (a ponte embrulha cada um como mensagem do canal). */
export interface ChannelWire {
  send(message: { t: 'f'; c: number; d: string }): void
}

/** Limites do Hub (§6.5): além deles o Hub responde 429; aqui nem sai. */
export const MAX_REQUESTS = 32
export const MAX_SUBSCRIPTIONS = 4
/** Hub fecha com 4013 acima disso no sentido aparelho→Hub (§6.3). */
export const MAX_OUTGOING = 1024 * 1024
/** Reconstrução máxima no sentido Hub→aparelho (§6.3). */
export const MAX_INCOMING = 16 * 1024 * 1024

/** Quebra do canal: contador fora de ordem, tag inválida ou mensagem grande demais. Fechar com 4002/4013. */
/** Código (não frase) de um aperto de mão que falhou por outro motivo que não `HandshakeError`. */
const HANDSHAKE_INVALID = 'handshakeInvalid'

export class ChannelBroken extends Error {
  readonly closeCode: number
  constructor(closeCode: number, message: string) {
    super(message)
    this.closeCode = closeCode
  }
}

const dec = new TextDecoder()

/** Quem recebe os quadros do navegador: um só por canal, sempre o mais recente primeiro (o Hub já descarta os intermediários). */
export interface FrameSink {
  onFrame: (frame: BrowserFrame) => void
  onEnd: (error: { status: number; message: string } | null) => void
}

interface Pending {
  resolve: (r: CallResult) => void
  timer: ReturnType<typeof setTimeout>
}

interface Sub {
  onEvent: (ev: unknown) => void
  onEnd: (error: { status: number; message: string } | null) => void
}

/** `Channel` e `Handshake` ligados a um protocolo; a API é a histórica de `fork-cloud/channel.ts`. */
export function createChannel(proto: Protocol) {
  const { CHUNK, deviceHello, HandshakeError, openFrame, sealFrame } = proto

  class Channel {
    private sendC = 0
    private recvC = 0
    private parts: Uint8Array[] = []
    private partsSize = 0
    private nextId = 1
    private closed = false
    private readonly pending = new Map<number, Pending>()
    private readonly subs = new Map<number, Sub>()
    private readonly keys: ChannelKeys
    private readonly wire: ChannelWire
    private binary = false
    private frameSink: FrameSink | null = null
    /** O Hub aceitou `browserFramesV1`; sem isso, qualquer quadro recebido quebra o canal. */
    readonly framesNegotiated: boolean

    constructor(keys: ChannelKeys, wire: ChannelWire, framesNegotiated = false) {
      this.keys = keys
      this.wire = wire
      this.framesNegotiated = framesNegotiated
    }

    get isOpen() {
      return !this.closed
    }

    /** Um quadro `f` recebido. Lança `ChannelBroken` (o canal inteiro deve ser descartado). */
    receive(frame: { c?: unknown; d?: unknown }) {
      if (this.closed) return
      if (frame.c !== this.recvC || typeof frame.d !== 'string') throw new ChannelBroken(4002, 'frame.outOfOrder')
      let opened: { final: boolean; chunk: Uint8Array }
      try {
        opened = openFrame(this.keys.h2d, this.recvC, frame.d)
      } catch {
        throw new ChannelBroken(4002, 'frame.unauthenticated')
      }
      this.recvC += 1
      this.partsSize += opened.chunk.length
      if (this.partsSize > MAX_INCOMING) throw new ChannelBroken(4013, 'message.tooLarge')
      if (this.parts.length === 0) this.binary = isFrameBytes(opened.chunk)
      // Quadro acima do teto é recusado antes de juntar os pedaços ou copiar o JPEG.
      if (this.binary && this.partsSize > FRAME_MAX_WIRE) throw new ChannelBroken(4013, 'message.tooLarge')
      this.parts.push(opened.chunk)
      if (!opened.final) return
      const bytes = concat(...this.parts)
      const binary = this.binary
      this.parts = []
      this.partsSize = 0
      this.binary = false
      if (binary) return this.dispatchFrame(bytes)
      let message: unknown
      try {
        message = JSON.parse(dec.decode(bytes))
      } catch {
        throw new ChannelBroken(4002, 'message.unreadable')
      }
      this.dispatch(message)
    }

    private dispatchFrame(bytes: Uint8Array) {
      // Hub que não negociou, quadro corrompido ou gigante: o canal inteiro é descartado (falha fechada).
      if (!this.framesNegotiated) throw new ChannelBroken(4002, 'frame.notNegotiated')
      let frame: BrowserFrame
      try {
        frame = decodeFrame(bytes)
      } catch (error) {
        throw new ChannelBroken(error instanceof FrameCodecError && error.kind === 'tooLarge' ? 4013 : 4002, 'frame.unreadable')
      }
      this.frameSink?.onFrame(frame)
    }

    private dispatch(m: unknown) {
      if (!m || typeof m !== 'object') return
      const msg = m as { id?: unknown; ok?: unknown; err?: { status?: unknown; message?: unknown; code?: unknown }; ev?: unknown; end?: unknown }
      if (typeof msg.id !== 'number') return
      const err = msg.err ? { status: typeof msg.err.status === 'number' ? msg.err.status : 500, message: typeof msg.err.message === 'string' ? msg.err.message : traduzir('pt-BR', 'nuvem.protocolo.erroDoHub'), ...(typeof msg.err.code === 'string' ? { code: msg.err.code } : {}) } : null
      const p = this.pending.get(msg.id)
      if (p && ('ok' in msg || err)) {
        this.pending.delete(msg.id)
        clearTimeout(p.timer)
        p.resolve(err ? { kind: 'err', ...err } : { kind: 'ok', value: msg.ok })
        return
      }
      const sub = this.subs.get(msg.id)
      if (!sub) return
      if ('ev' in msg) sub.onEvent(msg.ev)
      else if (msg.end === true || err) {
        this.subs.delete(msg.id)
        sub.onEnd(err)
      }
    }

    private send(inner: unknown): boolean {
      const bytes = new TextEncoder().encode(JSON.stringify(inner))
      if (bytes.length > MAX_OUTGOING) return false
      for (let at = 0; at === 0 || at < bytes.length; at += CHUNK) {
        const chunk = bytes.subarray(at, at + CHUNK)
        const final = at + CHUNK >= bytes.length
        this.wire.send({ t: 'f', c: this.sendC, d: sealFrame(this.keys.d2h, this.sendC, final, chunk) })
        this.sendC += 1
      }
      return true
    }

    call(method: string, body: unknown = null, timeoutMs = 20_000): Promise<CallResult> {
      if (this.closed) return Promise.resolve({ kind: 'notSent', reason: 'notReady' })
      // O Hub só inscreve e controla o navegador ao vivo com `browserFramesV1` aceito neste canal.
      if (method.startsWith('browserStream') && !this.framesNegotiated) return Promise.resolve({ kind: 'notSent', reason: 'notReady' })
      if (this.pending.size >= MAX_REQUESTS) return Promise.resolve({ kind: 'notSent', reason: 'busy' })
      const id = this.nextId++
      return new Promise<CallResult>((resolve) => {
        if (!this.send({ id, m: method, b: body })) return resolve({ kind: 'notSent', reason: 'tooLarge' })
        const timer = setTimeout(() => {
          if (!this.pending.delete(id)) return
          // O Hub descarta a resposta; uma mutação já iniciada não é desfeita (§6.5).
          if (!this.closed) this.send({ id, cancel: true })
          resolve({ kind: 'unknown', reason: 'timeout' })
        }, timeoutMs)
        this.pending.set(id, { resolve, timer })
      })
    }

    /** `null` quando o canal não está aberto ou já tem 4 assinaturas. */
    subscribe(kind: 'events' | 'term', body: unknown, onEvent: (ev: unknown) => void, onEnd: Sub['onEnd'] = () => {}): Subscription | null {
      if (this.closed || this.subs.size >= MAX_SUBSCRIPTIONS) return null
      const id = this.nextId++
      this.subs.set(id, { onEvent, onEnd })
      if (!this.send({ id, sub: kind, b: body })) {
        this.subs.delete(id)
        return null
      }
      return {
        id,
        cancel: () => {
          if (this.subs.delete(id) && !this.closed) this.send({ id, cancel: true })
        },
      }
    }

    /**
     * Liga o consumidor dos quadros do navegador. `null` quando o canal está fechado, o Hub não aceitou a
     * capacidade ou já há um consumidor. Quadro sem consumidor é descartado.
     */
    subscribeFrames(sink: FrameSink): Subscription | null {
      if (this.closed || !this.framesNegotiated || this.frameSink) return null
      this.frameSink = sink
      return {
        id: 0,
        cancel: () => {
          if (this.frameSink === sink) this.frameSink = null
        },
      }
    }

    /**
     * Fecha o canal: chamadas em voo viram `unknown`, assinaturas terminam. Idempotente.
     * `denied` (acesso revogado ou encerrado) chega às assinaturas de quadros como 403, e não como queda comum.
     */
    close(denied = false) {
      if (this.closed) return
      this.closed = true
      const sink = this.frameSink
      this.frameSink = null
      sink?.onEnd({ status: denied ? 403 : 0, message: traduzir('pt-BR', 'nuvem.protocolo.conexaoEncerrada') })
      for (const [id, p] of this.pending) {
        clearTimeout(p.timer)
        this.pending.delete(id)
        p.resolve({ kind: 'unknown', reason: 'closed' })
      }
      for (const [id, s] of this.subs) {
        this.subs.delete(id)
        s.onEnd({ status: 0, message: traduzir('pt-BR', 'nuvem.protocolo.conexaoEncerrada') })
      }
    }
  }

  /** Códigos de `reject` do Hub (§6.1). */
  type RejectCode = 'unauthorized' | 'revoked' | 'leaseExpired' | 'badSignature' | 'unsupportedVersion' | 'pairingClosed' | 'rateLimited'

  type HandshakeOutcome =
    | { kind: 'ready'; channel: Channel }
    | { kind: 'rejected'; code: RejectCode | 'unknown'; message: string }
    | { kind: 'identityMismatch'; message: string }
    /** Socket caiu, prazo venceu ou a presença do Hub sumiu antes da resposta. */
    | { kind: 'abandoned' }

  /**
   * Um aperto de mão em curso: manda o `hello` e resolve com o `welcome`/`reject` seguinte.
   * A chave do Hub é a pinada pelo vínculo (contrato §2, âncoras A1/A2), nunca a do diretório.
   */
  class Handshake {
    private readonly started: ReturnType<typeof deviceHello>
    private settle: ((o: HandshakeOutcome) => void) | null = null
    private settled: HandshakeOutcome | null = null
    readonly outcome: Promise<HandshakeOutcome>
    private readonly wire: ChannelWire & { sendRaw(message: object): void }
    private readonly offeredFrames: boolean

    /** `frames`: oferece `browserFramesV1` no `hello`; só este canal de dados do app negocia quadros. */
    constructor(signer: DeviceSigner, hostSignPub: Uint8Array, wire: ChannelWire & { sendRaw(message: object): void }, opts: { frames?: boolean } = {}) {
      this.wire = wire
      this.offeredFrames = opts.frames === true
      this.started = deviceHello(signer, hostSignPub, this.offeredFrames ? { caps: [FRAMES_CAPABILITY] } : {})
      this.outcome = new Promise((resolve) => (this.settle = resolve))
      wire.sendRaw(this.started.hello)
    }

    /** Mensagem do canal recebida durante o aperto de mão; true quando foi consumida. */
    receive(m: { t?: unknown; code?: unknown; message?: unknown; ek?: unknown; sig?: unknown; caps?: unknown }): boolean {
      if (!this.settle) return false
      if (m.t === 'reject') {
        const known: RejectCode[] = ['unauthorized', 'revoked', 'leaseExpired', 'badSignature', 'unsupportedVersion', 'pairingClosed', 'rateLimited']
        const code = known.includes(m.code as RejectCode) ? (m.code as RejectCode) : 'unknown'
        this.finish({ kind: 'rejected', code, message: typeof m.message === 'string' ? m.message : '' })
        return true
      }
      if (m.t !== 'welcome') return false
      try {
        const keys = this.started.finish(m)
        const accepted = this.offeredFrames && Array.isArray(m.caps) && m.caps.includes(FRAMES_CAPABILITY)
        this.finish({ kind: 'ready', channel: new Channel(keys, this.wire, accepted) })
      } catch (error) {
        this.finish({ kind: 'identityMismatch', message: error instanceof HandshakeError ? error.message : HANDSHAKE_INVALID })
      }
      return true
    }

    /** Desiste sem resposta do Hub (socket caiu, prazo, presença perdida). Idempotente. */
    abandon() {
      this.finish({ kind: 'abandoned' })
    }

    get pending() {
      return this.settle !== null
    }

    /** O resultado, disponível na mesma volta em que `receive` o produziu. */
    get result() {
      return this.settled
    }

    private finish(o: HandshakeOutcome) {
      const settle = this.settle
      if (!settle) return
      this.settle = null
      this.settled = o
      settle(o)
    }
  }

  return { Channel, Handshake }
}

export type ChannelClasses = ReturnType<typeof createChannel>
export type ChannelInstance = InstanceType<ChannelClasses['Channel']>
export type HandshakeInstance = InstanceType<ChannelClasses['Handshake']>

/** Códigos de `reject` do Hub (§6.1). */
export type RejectCode = 'unauthorized' | 'revoked' | 'leaseExpired' | 'badSignature' | 'unsupportedVersion' | 'pairingClosed' | 'rateLimited'

export type HandshakeOutcome =
  | { kind: 'ready'; channel: ChannelInstance }
  | { kind: 'rejected'; code: RejectCode | 'unknown'; message: string }
  | { kind: 'identityMismatch'; message: string }
  /** Socket caiu, prazo venceu ou a presença do Hub sumiu antes da resposta. */
  | { kind: 'abandoned' }

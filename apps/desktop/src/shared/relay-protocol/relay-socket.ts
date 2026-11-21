import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'
import { b64url, type DeviceSigner, type Protocol } from './protocol.ts'
import { ChannelBroken, type ChannelInstance, type ChannelClasses, type HandshakeInstance, type HandshakeOutcome, type RejectCode } from './channel.ts'

/**
 * A conexão de saída do aparelho até a ponte do Hub (docs/reference/remote-relay.md §7.1–§7.6):
 * `wss://<nuvem>/v1/relay/device/<computerId>?v=1` assinado com a chave do aparelho, presença do
 * Hub, `hello` a cada `presence online:true`, ping exato, recuo com jitter e a política de códigos de
 * fechamento. Contrato `donna-acesso/v1` §4.6.
 *
 * O socket aberto não autoriza nada: só o canal com a chave pinada entrega dados. Cada socket tem
 * uma geração; mensagens, quedas e respostas de uma geração antiga são descartadas.
 *
 * Isomórfico: as primitivas e as classes do canal entram por `createRelaySocket(deps)`; não há
 * backend padrão (o app desktop liga `backend-node` no calço `main/fork-cloud/relay-socket.ts`).
 * `browserSocket` manda o `Codync-Sig` como subprotocolo `codync-sig.v1` (o WebSocket do
 * navegador não aceita cabeçalhos); `nodeSocket` usa a ponte `headers` do WebSocket do Node.
 */

export interface SocketLike {
  send(data: string): void
  close(code?: number, reason?: string): void
  addEventListener(type: 'open' | 'error', listener: () => void): void
  addEventListener(type: 'message', listener: (event: { data: unknown }) => void): void
  addEventListener(type: 'close', listener: (event: { code: number; reason: string }) => void): void
}

export type SocketFactory = (url: string, headers: Record<string, string>) => SocketLike

const WS = () => (globalThis as unknown as { WebSocket: new (url: string, init: unknown) => SocketLike }).WebSocket

/** O WebSocket do Node (undici) aceita cabeçalhos no segundo argumento; o DOM não tipa isso. */
export const nodeSocket: SocketFactory = (url, headers) => new (WS())(url, { headers })

/** Teto do valor decodificado no subprotocolo (o servidor, `cloud/src/auth.ts`, recusa mais que isso). */
const SIG_SUBPROTOCOL_MAX_BYTES = 512
/** Mesmo texto que o cabeçalho aceita: ASCII imprimível (espaço só no meio), sem controles nem espaços nas pontas. */
const SIG_VALUE = /^[\x21-\x7e](?:[\x20-\x7e]*[\x21-\x7e])?$/

/**
 * O WebSocket do navegador não envia cabeçalhos no upgrade: a assinatura viaja como
 * subprotocolo. A oferta tem exatamente dois itens, nesta ordem e caixa: `codync-sig.v1` e
 * `base64url(utf8(valor do Codync-Sig))` (até 512 bytes, ASCII imprimível). O servidor da ponte
 * (cloud/src/auth.ts `sigFromSubprotocol`, PR #742) aceita só isso e seleciona `codync-sig.v1`
 * no 101; o vetor compartilhado está em docs/reference/fixtures/browser-socket-subprotocol.json.
 */
export const browserSocket: SocketFactory = (url, headers) => {
  const value = headers['Codync-Sig']
  if (value === undefined) throw new Error('socket.sigMissing')
  const bytes = new TextEncoder().encode(value)
  if (bytes.length > SIG_SUBPROTOCOL_MAX_BYTES || !SIG_VALUE.test(value)) throw new Error('socket.sigInvalid')
  return new (WS())(url, ['codync-sig.v1', b64url(bytes)])
}

export type RetryCause =
  | 'connectFailed'
  | 'socketClosed'
  | 'leaseExpired'
  | 'rateLimited'
  | 'silent'
  | 'handshakeTimeout'
  | 'protocol'
  | 'rekey'

/**
 * Fim sem nova tentativa automática: só uma ação da pessoa (ou um novo vínculo) recomeça.
 * `handshakeRefused`: o Hub mandou `reject` antes do `welcome`; a recusa não autentica o Hub nem
 * prova a causa (o código vai em `rejectCode`). `hubUnverified`: o `welcome` não confere com a chave
 * pinada (outro Hub ou caminho adulterado; o módulo não afirma qual). Nenhum dos dois troca pin ou chave.
 */
export type StopReason = 'stopped' | 'revoked' | 'unauthorized' | 'updateRequired' | 'handshakeRefused' | 'hubUnverified'

export type LinkState =
  | { kind: 'connecting'; attempt: number }
  /** Socket aberto, Hub fora da ponte (`presence online:false`). */
  | { kind: 'hubOffline'; lastSeenAt: number | null }
  | { kind: 'handshaking' }
  | { kind: 'ready' }
  | { kind: 'retrying'; attempt: number; delayMs: number; cause: RetryCause; connectFailures: number }
  | { kind: 'stopped'; reason: StopReason; message?: string; rejectCode?: RejectCode }

export interface Timings {
  pingMs: number
  silenceMs: number
  handshakeMs: number
  backoffBaseMs: number
  backoffMaxMs: number
  stableMs: number
}

/** §7.6: ping 30 s, silêncio 90 s, aperto 10 s, recuo 1 s→30 s, estável após 60 s. */
export const DEFAULT_TIMINGS: Timings = { pingMs: 30_000, silenceMs: 90_000, handshakeMs: 10_000, backoffBaseMs: 1_000, backoffMaxMs: 30_000, stableMs: 60_000 }

export interface RelayLinkOptions {
  cloudURL: string
  computerId: string
  /** Chave do Hub pinada no vínculo (A1/A2). */
  hostSignPub: Uint8Array
  signer: DeviceSigner
  socket?: SocketFactory
  timings?: Partial<Timings>
  random?: () => number
  /** Guarda da sessão capturada; sem guarda, uso legado continua idêntico. */
  isAuthorized?: () => boolean
  onState?: (state: LinkState) => void
  /** O canal pronto, ou `null` quando ele deixa de valer (chamadas em voo já viraram `unknown`). */
  onChannel?: (channel: ChannelInstance | null) => void
  /** Oferece `browserFramesV1` no `hello` (só o canal de dados do app; os de vínculo e pareamento não pedem). */
  browserFrames?: boolean
}

const PING = '{"t":"ping"}'
const HANDSHAKE_REFUSED = traduzir('pt-BR', 'nuvem.protocolo.hubRecusou')
const HUB_UNVERIFIED = traduzir('pt-BR', 'nuvem.protocolo.hubNaoVerificado')
const COMPUTER_ID = /^[A-Za-z0-9_-]{22}$/

/** URL do socket do aparelho; recusa base que não seja https (ou http de loopback, só teste local). */
export function deviceRelayURL(cloudURL: string, computerId: string): URL {
  if (!COMPUTER_ID.test(computerId)) throw new Error('url.invalidComputerId')
  const base = new URL(cloudURL)
  const loopback = ['127.0.0.1', 'localhost', '[::1]'].includes(base.hostname)
  if (base.protocol !== 'https:' && !(base.protocol === 'http:' && loopback)) throw new Error('url.httpsRequired')
  if (base.username || base.password || base.search || base.hash || (base.pathname !== '/' && base.pathname !== '')) throw new Error('url.invalidCloudURL')
  const url = new URL(`/v1/relay/device/${computerId}?v=1`, base)
  url.protocol = base.protocol === 'https:' ? 'wss:' : 'ws:'
  return url
}

export interface RelaySocketDeps {
  proto: Protocol
  channel: ChannelClasses
}

/** `RelayLink` ligado a um protocolo e às classes do canal; a API é a histórica de `fork-cloud/relay-socket.ts`. */
export function createRelaySocket({ proto, channel }: RelaySocketDeps) {
  const { signatureHeader } = proto
  const { Handshake } = channel

  class RelayLink {
    private readonly t: Timings
    private readonly url: URL
    private gen = 0
    private socket: SocketLike | null = null
    private opened = false
    private handshake: HandshakeInstance | null = null
    private channel: ChannelInstance | null = null
    private attempt = 0
    private connectFailures = 0
    private readyAt: number | null = null
    private lastHeard = 0
    private timers: ReturnType<typeof setTimeout>[] = []
    private ticker: ReturnType<typeof setInterval> | null = null
    private stateNow: LinkState = { kind: 'stopped', reason: 'stopped' }
    private terminal = false
    private lastPing = 0
    private readonly o: RelayLinkOptions

    constructor(o: RelayLinkOptions) {
      this.o = o
      this.t = { ...DEFAULT_TIMINGS, ...o.timings }
      this.url = deviceRelayURL(o.cloudURL, o.computerId)
    }

    get state() {
      return this.stateNow
    }

    get currentChannel() {
      return this.channel
    }

    /**
     * Cada `RelayLink` é uma encarnação: depois de parar (por pedido ou por fim sem nova tentativa)
     * não reabre. Outra conta, outro vínculo ou nova tentativa da pessoa criam outro objeto, e nada
     * do anterior pode ser confundido com ele (contrato §4.7).
     */
    start() {
      if (this.terminal || this.socket || this.timers.length) return
      this.connect()
    }

    /** Para de vez (logout, troca de conta, revogação local): nada desta conexão chega depois. */
    stop(reason: StopReason = 'stopped', message?: string, rejectCode?: RejectCode) {
      if (this.terminal) return
      this.terminal = true
      this.detach(1000, reason === 'revoked' || reason === 'unauthorized')
      this.clearTimers()
      this.setState({ kind: 'stopped', reason, ...(message ? { message } : {}), ...(rejectCode ? { rejectCode } : {}) })
    }

    private setState(s: LinkState) {
      this.stateNow = s
      this.o.onState?.(s)
    }

    private isCurrent(gen: number) {
      return !this.terminal && gen === this.gen && (this.o.isAuthorized?.() ?? true)
    }

    private clearTimers() {
      for (const t of this.timers) clearTimeout(t)
      this.timers = []
      if (this.ticker) clearInterval(this.ticker)
      this.ticker = null
    }

    private after(ms: number, fn: () => void) {
      const gen = this.gen
      if (!this.isCurrent(gen)) return
      const t = setTimeout(() => {
        this.timers = this.timers.filter((x) => x !== t)
        if (this.isCurrent(gen)) fn()
      }, ms)
      this.timers.push(t)
    }

    private connect() {
      if (this.terminal) return
      const gen = ++this.gen
      this.opened = false
      this.setState({ kind: 'connecting', attempt: this.attempt })
      if (!this.isCurrent(gen)) return
      let socket: SocketLike
      try {
        const headers = { 'Codync-Sig': signatureHeader(this.o.signer, 'GET', this.url) }
        if (!this.isCurrent(gen)) return
        socket = (this.o.socket ?? nodeSocket)(this.url.toString(), headers)
      } catch {
        if (this.isCurrent(gen)) this.retry('connectFailed')
        return
      }
      if (!this.isCurrent(gen)) {
        try {
          socket.close(1000)
        } catch {}
        return
      }
      this.socket = socket
      socket.addEventListener('open', () => {
        if (!this.isCurrent(gen)) return
        this.opened = true
        this.connectFailures = 0
        this.lastHeard = Date.now()
        this.setState({ kind: 'hubOffline', lastSeenAt: null })
        if (!this.isCurrent(gen)) return
        this.ticker = setInterval(() => this.tick(gen), Math.min(this.t.pingMs, this.t.silenceMs) / 2)
      })
      socket.addEventListener('message', (event) => {
        if (!this.isCurrent(gen)) return
        this.lastHeard = Date.now()
        this.onMessage(event.data)
      })
      socket.addEventListener('close', (event) => {
        if (!this.isCurrent(gen)) return
        this.onClose(event.code)
      })
      socket.addEventListener('error', () => {
        if (!this.isCurrent(gen)) return
        // O Node pode recusar o upgrade emitindo só error, sem um close posterior.
        this.retry(this.opened ? 'socketClosed' : 'connectFailed')
      })
    }

    private tick(gen: number) {
      if (!this.isCurrent(gen) || !this.socket) return
      const now = Date.now()
      if (now - this.lastHeard > this.t.silenceMs) return this.retry('silent')
      if (now - this.lastPing >= this.t.pingMs) {
        this.lastPing = now
        this.socket.send(PING)
      }
    }

    private onMessage(data: unknown) {
      const gen = this.gen
      if (!this.isCurrent(gen)) return
      let m: Record<string, unknown>
      try {
        m = JSON.parse(String(data)) as Record<string, unknown>
      } catch {
        return
      }
      if (!m || typeof m !== 'object') return
      switch (m.t) {
        case 'presence':
          if (m.online === true) this.beginHandshake()
          else {
            this.dropChannel()
            if (!this.isCurrent(gen)) return
            this.setState({ kind: 'hubOffline', lastSeenAt: typeof m.lastSeenAt === 'number' ? m.lastSeenAt : null })
          }
          return
        case 'welcome':
        case 'reject': {
          if (m.t === 'reject' && this.channel) {
            // O Hub também rejeita um canal pronto (por exemplo, quando o lease vence).
            const known: RejectCode[] = ['unauthorized', 'revoked', 'leaseExpired', 'badSignature', 'unsupportedVersion', 'pairingClosed', 'rateLimited']
            const code = known.includes(m.code as RejectCode) ? (m.code as RejectCode) : 'unknown'
            this.onHandshake({ kind: 'rejected', code, message: typeof m.message === 'string' ? m.message : '' })
            return
          }
          // Síncrono: nenhum quadro pode chegar entre o `welcome` e o canal pronto.
          const hs = this.handshake
          if (hs?.receive(m) && hs.result) this.onHandshake(hs.result)
          return
        }
        case 'f':
          if (!this.channel) return
          try {
            this.channel.receive(m)
          } catch (error) {
            this.retry('protocol', error instanceof ChannelBroken ? error.closeCode : 4002)
          }
          return
        default:
        // `pong`, `mbox.*` e mensagens desconhecidas não mudam o estado.
      }
    }

    private wire(gen: number) {
      const send = (message: object) => {
        if (this.isCurrent(gen)) this.socket?.send(JSON.stringify(message))
      }
      return { send, sendRaw: send }
    }

    private beginHandshake() {
      const gen = this.gen
      if (!this.isCurrent(gen)) return
      this.dropChannel()
      if (!this.isCurrent(gen)) return
      const hs = new Handshake(this.o.signer, this.o.hostSignPub, this.wire(gen), { frames: this.o.browserFrames === true })
      if (!this.isCurrent(gen)) {
        hs.abandon()
        return
      }
      this.handshake = hs
      this.setState({ kind: 'handshaking' })
      if (!this.isCurrent(gen)) return
      this.after(this.t.handshakeMs, () => {
        if (gen === this.gen && this.handshake === hs && hs.pending) this.retry('handshakeTimeout')
      })
    }

    private onHandshake(o: HandshakeOutcome) {
      const gen = this.gen
      if (!this.isCurrent(gen)) return
      const wasReady = this.channel !== null
      this.handshake = null
      switch (o.kind) {
        case 'ready':
          this.channel = o.channel
          this.readyAt = Date.now()
          this.setState({ kind: 'ready' })
          if (this.isCurrent(gen) && this.channel === o.channel) this.o.onChannel?.(o.channel)
          return
        case 'identityMismatch':
          // §6.2: não tentar de novo a mesma chave por outro caminho. Não verificado ≠ Hub trocado.
          return this.stop('hubUnverified', HUB_UNVERIFIED)
        case 'rejected':
          switch (o.code) {
            case 'revoked':
              return this.stop('revoked', o.message)
            case 'unsupportedVersion':
              return this.stop('updateRequired', o.message)
            case 'leaseExpired':
              return this.retry('leaseExpired')
            case 'rateLimited':
              return this.retry('rateLimited')
            case 'unknown':
              return this.retry('protocol')
            default:
              if (wasReady) return this.stop('unauthorized')
              // `unauthorized`, `badSignature`, `pairingClosed`: recusa antes do `welcome`, sem autenticar
              // o Hub; o texto vindo do outro lado não é repetido.
              return this.stop('handshakeRefused', HANDSHAKE_REFUSED, o.code)
          }
        case 'abandoned':
      }
    }

    private onClose(code: number) {
      if (code === 4003) return this.stop('revoked')
      if (code === 4001 || code === 4410) return this.stop('unauthorized')
      if (code === 4400) return this.stop('updateRequired')
      if (code === 4011) return this.retry('rekey')
      if (code === 4008) return this.retry('rateLimited')
      this.retry(this.opened ? 'socketClosed' : 'connectFailed')
    }

    /** Larga o socket atual sem esperar o fechamento: eventos dele são ignorados pela geração. */
    private detach(code: number, denied = false) {
      const gen = ++this.gen
      const socket = this.socket
      this.socket = null
      if (this.ticker) clearInterval(this.ticker)
      this.ticker = null
      this.dropChannel(denied)
      try {
        socket?.close(code)
      } catch {}
      return gen
    }

    private dropChannel(denied = false) {
      this.handshake?.abandon()
      this.handshake = null
      const had = this.channel
      this.channel = null
      if (had) {
        had.close(denied)
        this.o.onChannel?.(null)
      }
    }

    private retry(cause: RetryCause, closeCode = 1000) {
      if (this.terminal) return
      if (this.readyAt !== null && Date.now() - this.readyAt >= this.t.stableMs) this.attempt = 0
      this.readyAt = null
      if (cause === 'connectFailed') this.connectFailures += 1
      const gen = this.detach(closeCode)
      if (!this.isCurrent(gen)) return
      this.clearTimers()
      let delayMs = 0
      if (cause !== 'rekey') {
        this.attempt += 1
        const cap = Math.min(this.t.backoffBaseMs * 2 ** (this.attempt - 1), this.t.backoffMaxMs)
        delayMs = Math.max(Math.floor((this.o.random ?? Math.random)() * (cap + 1)), Math.min(250, cap))
      }
      this.setState({ kind: 'retrying', attempt: this.attempt, delayMs, cause, connectFailures: this.connectFailures })
      if (!this.isCurrent(gen)) return
      this.after(delayMs, () => this.connect())
    }
  }

  return { RelayLink }
}

export type { ChannelInstance, ChannelClasses, HandshakeInstance, HandshakeOutcome, RejectCode }

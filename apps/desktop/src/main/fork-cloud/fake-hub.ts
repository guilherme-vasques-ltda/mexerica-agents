import { b64url, cidRaw, channelKeys, CHUNK, ephemeral, hs1Input, openFrame, sealFrame, transcriptHash, unb64url, verifyEd25519, type ChannelKeys, type DeviceSigner } from './crypto.ts'
import type { SocketLike } from './relay-socket.ts'

/**
 * Somente testes: o lado do Hub e da ponte (DO) jogado em memória, com as mesmas primitivas e
 * vetores do Hub real, para provar as fronteiras do cliente sem rede. A prova com o Hub real e o
 * Worker local fica no e2e (`cloud/test/e2e/donna-acesso.e2e.ts`).
 */

const enc = new TextEncoder()
const dec = new TextDecoder()

/**
 * O Hub responde um `hello`: confere a assinatura do aparelho e assina o transcript. `signWith`
 * simula um impostor que conhece o `computerId` mas não a chave do Hub.
 */
export function hostAccept(host: DeviceSigner, hello: { dk: string; ek: string; n: string; sig: string; caps?: string[] }, signWith: DeviceSigner = host, acceptCaps = false) {
  const cid = cidRaw(host.publicKey)
  const dk = unb64url(hello.dk, 32)!
  const ekD = unb64url(hello.ek, 32)!
  const n = unb64url(hello.n, 32)!
  if (!verifyEd25519(dk, unb64url(hello.sig, 64)!, hs1Input(cid, dk, ekD, n))) throw new Error('hello não confere')
  const eph = ephemeral()
  const th = transcriptHash(cid, dk, ekD, n, eph.pub)
  return { welcome: { t: 'welcome', v: 1, ek: b64url(eph.pub), sig: b64url(signWith.sign(th)), ...(acceptCaps && hello.caps?.length ? { caps: hello.caps } : {}) }, keys: channelKeys(eph.dh(ekD), th), dk: hello.dk }
}

/** Quadros do lado do Hub: sela h2d e abre d2h com contadores próprios. */
export class HostCodec {
  private sendC = 0
  private recvC = 0
  private parts: Uint8Array[] = []
  private readonly keys: ChannelKeys
  constructor(keys: ChannelKeys) {
    this.keys = keys
  }

  seal(message: unknown): { t: 'f'; c: number; d: string }[] {
    const bytes = enc.encode(JSON.stringify(message))
    const out: { t: 'f'; c: number; d: string }[] = []
    for (let at = 0; at === 0 || at < bytes.length; at += CHUNK) {
      const final = at + CHUNK >= bytes.length
      out.push({ t: 'f', c: this.sendC, d: sealFrame(this.keys.h2d, this.sendC, final, bytes.subarray(at, at + CHUNK)) })
      this.sendC += 1
    }
    return out
  }

  /** Mensagem interna binária (quadro `MXJ1`), selada e repartida em pedaços como qualquer outra. */
  sealBytes(bytes: Uint8Array): { t: 'f'; c: number; d: string }[] {
    const out: { t: 'f'; c: number; d: string }[] = []
    for (let at = 0; at === 0 || at < bytes.length; at += CHUNK) {
      const final = at + CHUNK >= bytes.length
      out.push({ t: 'f', c: this.sendC, d: sealFrame(this.keys.h2d, this.sendC, final, bytes.subarray(at, at + CHUNK)) })
      this.sendC += 1
    }
    return out
  }

  /** Mensagem interna repartida exatamente nestes pedaços (o último é o final), para provar os limites do receptor. */
  sealPieces(pieces: Uint8Array[]): { t: 'f'; c: number; d: string }[] {
    return pieces.map((piece, i) => {
      const frame = { t: 'f' as const, c: this.sendC, d: sealFrame(this.keys.h2d, this.sendC, i === pieces.length - 1, piece) }
      this.sendC += 1
      return frame
    })
  }

  open(frame: { c: number; d: string }): unknown {
    if (frame.c !== this.recvC) throw new Error('contador')
    const { final, chunk } = openFrame(this.keys.d2h, frame.c, frame.d)
    this.recvC += 1
    this.parts.push(chunk)
    if (!final) return undefined
    const m = JSON.parse(dec.decode(Buffer.concat(this.parts)))
    this.parts = []
    return m
  }
}

type Listener = (event: never) => void

/** Um WebSocket em memória: o teste age como a ponte e o Hub do outro lado. */
export class FakeSocket implements SocketLike {
  readonly sent: string[] = []
  closedWith: number | null = null
  private listeners = new Map<string, Listener[]>()
  readonly url: string
  readonly headers: Record<string, string>

  constructor(url: string, headers: Record<string, string>) {
    this.url = url
    this.headers = headers
  }

  addEventListener(type: string, listener: Listener) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener])
  }

  private emit(type: string, event: unknown) {
    for (const l of this.listeners.get(type) ?? []) l(event as never)
  }

  send(data: string) {
    this.sent.push(data)
  }

  close(code = 1000) {
    this.closedWith = code
  }

  // Ações do lado da ponte/Hub.
  open() {
    this.emit('open', {})
  }
  deliver(message: unknown) {
    this.emit('message', { data: typeof message === 'string' ? message : JSON.stringify(message) })
  }
  drop(code: number, reason = '') {
    this.emit('close', { code, reason })
  }

  /** Mensagens JSON enviadas pelo aparelho desde `from`. */
  messages(from = 0): Record<string, unknown>[] {
    return this.sent.slice(from).map((s) => JSON.parse(s) as Record<string, unknown>)
  }

  lastHello() {
    const hello = this.messages().filter((m) => m.t === 'hello').pop()
    if (!hello) throw new Error('nenhum hello enviado')
    return hello as { dk: string; ek: string; n: string; sig: string; caps?: string[] }
  }
}

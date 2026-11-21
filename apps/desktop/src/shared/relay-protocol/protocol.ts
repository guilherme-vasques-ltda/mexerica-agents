import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'
import { chacha20poly1305 } from '@noble/ciphers/chacha.js'
import type { CryptoBackend, DeviceSigner, Ephemeral } from './backend.ts'
import { strictInputs } from './ed25519-strict.ts'

/**
 * Primitivas do protocolo da ponte (docs/reference/remote-relay.md §1, §5, §6;
 * contrato `donna-acesso/v1` §2–§3), parametrizadas por um `CryptoBackend`
 * (`backend.ts`): Ed25519, X25519 e HKDF-SHA256 vêm do backend; ChaCha20-Poly1305 é
 * portátil de `@noble/ciphers`, inclusive no Electron/BoringSSL, que não fornece essa
 * cifra nativa. Conferidas pelos vetores compartilhados
 * (`docs/reference/fixtures/remote-relay-vectors.json`) em `protocol.test.ts`.
 *
 * Isomórfico: não importa `node:*` nem usa `Buffer` (o desktop liga o backend
 * `node:crypto` em `backend-node.ts`; a web liga `@noble/*` em `backend-noble.ts`).
 */

export type { CryptoBackend, DeviceSigner, Ephemeral } from './backend.ts'

const enc = new TextEncoder()

// ---- base64url estrito, sem padding ----

const B64U = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_'
const B64U_REV = (() => {
  const t = new Int16Array(128).fill(-1)
  for (let i = 0; i < B64U.length; i++) t[B64U.charCodeAt(i)] = i
  return t
})()

export const b64url = (bytes: Uint8Array) => {
  let out = ''
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i]!
    const b = i + 1 < bytes.length ? bytes[i + 1]! : -1
    const c = i + 2 < bytes.length ? bytes[i + 2]! : -1
    out += B64U[a >> 2] + B64U[((a & 3) << 4) | (b < 0 ? 0 : b >> 4)]
    if (b >= 0) out += B64U[((b & 15) << 2) | (c < 0 ? 0 : c >> 6)]
    if (c >= 0) out += B64U[c & 63]
  }
  return out
}

/** base64url estrito, sem padding; `null` quando malformado ou com tamanho diferente de `len`. */
export function unb64url(s: unknown, len?: number): Uint8Array | null {
  if (typeof s !== 'string' || !/^[A-Za-z0-9_-]*$/.test(s) || s.length % 4 === 1) return null
  const out = new Uint8Array(Math.floor((s.length * 6) / 8))
  let acc = 0
  let bits = 0
  let at = 0
  for (let i = 0; i < s.length; i++) {
    const v = B64U_REV[s.charCodeAt(i)]!
    acc = ((acc << 6) | v) & 0xffff
    bits += 6
    if (bits >= 8) {
      bits -= 8
      out[at++] = (acc >> bits) & 0xff
    }
  }
  const bytes = out.subarray(0, at)
  if (len !== undefined && bytes.length !== len) return null
  return b64url(bytes) === s ? bytes : null
}

export const concat = (...parts: Uint8Array[]) => {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0))
  let at = 0
  for (const p of parts) {
    out.set(p, at)
    at += p.length
  }
  return out
}

export class HandshakeError extends Error {}

export interface ChannelKeys {
  d2h: Uint8Array
  h2d: Uint8Array
}

export interface HelloMessage {
  t: 'hello'
  v: 1
  dk: string
  ek: string
  n: string
  sig: string
  /** Capacidades opcionais oferecidas; fora da assinatura do aperto de mão. Ausente = nenhuma. */
  caps?: string[]
}

const u64be = (c: number) => {
  const out = new Uint8Array(8)
  new DataView(out.buffer).setBigUint64(0, BigInt(c))
  return out
}

// ---- frames (§6.3) ----

export const CHUNK = 256 * 1024

const frameNonce = (c: number) => concat(new Uint8Array(4), u64be(c))
const frameAad = (c: number) => concat(enc.encode('codync/frame/v1'), u64be(c))

/** A API pública histórica de `main/fork-cloud/crypto.ts`, ligada a um backend. */
export function createProtocol(backend: CryptoBackend) {
  const { sha256, random } = backend
  const ephemeral = backend.x25519.ephemeral
  const { signerFromSeed, newSeed: newDeviceSeed } = backend.ed25519
  /**
   * Verificação Ed25519 única do protocolo (handshake, assinaturas de eventos…): a política estrita
   * (A/R canônicos e de ordem não baixa, S < L) roda aqui, antes do backend, para que node 22/26,
   * Electron e navegador deem o mesmo veredito independentemente do OpenSSL/BoringSSL.
   */
  const verify = (pub: Uint8Array, sig: Uint8Array, message: Uint8Array) => strictInputs(pub, sig) !== null && backend.ed25519.verify(pub, sig, message)

  const cidRaw = (hostSignPub: Uint8Array) => sha256(hostSignPub).subarray(0, 16)
  const computerIdOf = (hostSignPub: Uint8Array) => b64url(cidRaw(hostSignPub))

  const sigCanonical = (method: string, authority: string, pathAndQuery: string, ts: number, nonce: string, body: Uint8Array) =>
    ['codync-sig-v1', method.toUpperCase(), authority.toLowerCase(), pathAndQuery, String(ts), nonce, b64url(sha256(body))].join('\n')

  /** Valor do cabeçalho `Codync-Sig` para `url` (authority em minúsculas, com porta não padrão). */
  const signatureHeader = (
    signer: DeviceSigner,
    method: string,
    url: URL,
    body: Uint8Array = new Uint8Array(),
    ts = Date.now(),
    nonce = b64url(random(16)),
  ) => {
    const canonical = sigCanonical(method, url.host, `${url.pathname || '/'}${url.search}`, ts, nonce, body)
    return `v=1,kid=${b64url(signer.publicKey)},ts=${ts},nonce=${nonce},sig=${b64url(signer.sign(enc.encode(canonical)))}`
  }

  // ---- handshake (§6.2) ----

  const hs1Input = (cid: Uint8Array, dk: Uint8Array, ekD: Uint8Array, n: Uint8Array) => concat(enc.encode('codync/hs1/v1'), cid, dk, ekD, n)

  const transcriptHash = (cid: Uint8Array, dk: Uint8Array, ekD: Uint8Array, n: Uint8Array, ekH: Uint8Array) =>
    sha256(concat(enc.encode('codync/hs2/v1'), cid, dk, ekD, n, ekH))

  const hkdfExtract = (salt: Uint8Array, ikm: Uint8Array) => backend.hmacSha256(salt, ikm)

  const channelKeys = (ss: Uint8Array, th: Uint8Array): ChannelKeys => {
    const key = (info: string) => backend.hkdfSha256(ss, th, enc.encode(info), 32)
    return { d2h: key('codync/d2h/v1'), h2d: key('codync/h2d/v1') }
  }

  /**
   * Lado do aparelho: monta o `hello` e, com o `welcome`, confere a assinatura com a chave do Hub
   * **pinada** (nunca a que o diretório mandou depois) e deriva as chaves do canal.
   */
  const deviceHello = (device: DeviceSigner, hostSignPub: Uint8Array, opts: { eph?: Ephemeral; n?: Uint8Array; caps?: string[] } = {}) => {
    const eph = opts.eph ?? ephemeral()
    const n = opts.n ?? random(32)
    const cid = cidRaw(hostSignPub)
    const hello: HelloMessage = {
      t: 'hello',
      v: 1,
      dk: b64url(device.publicKey),
      ek: b64url(eph.pub),
      n: b64url(n),
      sig: b64url(device.sign(hs1Input(cid, device.publicKey, eph.pub, n))),
      ...(opts.caps?.length ? { caps: [...opts.caps] } : {}),
    }
    return {
      hello,
      finish(welcome: { ek?: unknown; sig?: unknown }): ChannelKeys {
        const ekH = unb64url(welcome.ek, 32)
        const sig = unb64url(welcome.sig, 64)
        if (!ekH || !sig) throw new HandshakeError(traduzir('pt-BR', 'nuvem.protocolo.apertoMalformado'))
        const th = transcriptHash(cid, device.publicKey, eph.pub, n, ekH)
        if (!verify(hostSignPub, sig, th)) throw new HandshakeError(traduzir('pt-BR', 'nuvem.protocolo.identidadeNaoConfere'))
        return channelKeys(eph.dh(ekH), th)
      },
    }
  }

  const sealFrame = (key: Uint8Array, c: number, final: boolean, chunk: Uint8Array): string => {
    const plaintext = concat(new Uint8Array([final ? 0 : 1]), chunk)
    return b64url(chacha20poly1305(key, frameNonce(c), frameAad(c)).encrypt(plaintext))
  }

  const openFrame = (key: Uint8Array, c: number, d: string): { final: boolean; chunk: Uint8Array } => {
    const bytes = unb64url(d)
    if (!bytes || bytes.length < 17) throw new Error('frame.malformed')
    const pt = chacha20poly1305(key, frameNonce(c), frameAad(c)).decrypt(bytes)
    if (pt[0] !== 0 && pt[0] !== 1) throw new Error('frame.invalidMarker')
    return { final: pt[0] === 0, chunk: pt.subarray(1) }
  }

  // ---- pedido de acesso por código (§4.2 B) ----

  const sasCommit = (dk: Uint8Array, nD: Uint8Array) => sha256(concat(enc.encode('codync/sascommit/v1'), dk, nD))

  const sasCode = (hostSignPub: Uint8Array, dk: Uint8Array, nD: Uint8Array, nH: Uint8Array): string => {
    const h = sha256(concat(enc.encode('codync/sas/v2'), hostSignPub, dk, nD, nH))
    return String(new DataView(h.buffer, h.byteOffset).getUint32(0) % 1_000_000).padStart(6, '0')
  }

  return {
    b64url,
    unb64url,
    concat,
    sha256,
    random,
    cidRaw,
    computerIdOf,
    signerFromSeed,
    newDeviceSeed,
    verifyEd25519: verify,
    ephemeral,
    sigCanonical,
    signatureHeader,
    hs1Input,
    transcriptHash,
    hkdfExtract,
    channelKeys,
    deviceHello,
    HandshakeError,
    CHUNK,
    sealFrame,
    openFrame,
    sasCommit,
    sasCode,
  }
}

export type Protocol = ReturnType<typeof createProtocol>

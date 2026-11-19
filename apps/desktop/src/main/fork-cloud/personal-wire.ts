/** Consumidor fechado PL1. Não é IPC nem seleção de raiz; bytes verificados são imutáveis. */
import { createHash, hkdfSync } from 'node:crypto'
import { chacha20poly1305 } from '@noble/ciphers/chacha.js'
import { b64url, computerIdOf, ephemeral, unb64url, verifyEd25519, type Ephemeral } from './crypto.ts'
import { personalCanonical, type PersonalContext } from './personal-protocol.ts'
import { WIRE_SHAPES, type PersonalKind } from './personal-shapes.ts'
export type { PersonalKind }
export const personalDomain = (name: string) => `donna/personal-linux/${name}/v1`
export const personalHash = (v: unknown) => createHash('sha256').update(JSON.stringify(v)).digest('hex')
const bytes = (v: unknown) => Buffer.from(JSON.stringify(v))
function need(v: unknown): asserts v { if (!v) throw new Error('invalidPersonalWire') }
function freeze(v: unknown): unknown {
  if (v && typeof v === 'object') { for (const x of Object.values(v)) freeze(x); Object.freeze(v) }
  return v
}
export interface PersonalKeys { readonly issuer: string; readonly app: string; readonly hub: string; readonly box: string }
export interface PersonalSigned { readonly payload: readonly unknown[]; readonly wire: string; readonly hash: string }
// Modos internos. Só a transação/journal futuro decide o instante histórico, nunca renderer.
export type PersonalTime = { readonly kind: 'current'; readonly now: number } | { readonly kind: 'committed'; readonly at: number } | { readonly kind: 'historical'; readonly now: number }
const instant = (time: PersonalTime) => time.kind === 'committed' ? time.at : time.now
function field(token: string, v: unknown, C: PersonalContext): boolean {
  if (token.startsWith('?')) return v === null || field(token.slice(1), v, C)
  if (token.startsWith('=')) return v === token.slice(1)
  switch (token) {
    case 'C': return JSON.stringify(v) === JSON.stringify(C)
    case 'T': return Array.isArray(v) && v.length === 19 && field('C', v[0], C) &&
      'id mode hash hash id id key key id key cid key key key =first-owner int int int'.split(' ').every((t, i) => field(t, v[i + 1], C)) &&
      v[18] > v[17] && v[18] - v[17] <= 600000
    case 'id': return typeof v === 'string' && /^[A-Za-z0-9_./:-]{1,256}$/.test(v)
    case 'grant': return typeof v === 'string' && /^grt_[A-Za-z0-9_-]{22}$/.test(v) && !!unb64url(v.slice(4), 16)
    case 'key': return !!unb64url(v, 32)
    case 'cid': return !!unb64url(v, 16)
    case 'nonce16': return !!unb64url(v, 16)
    case 'hash': return typeof v === 'string' && /^[a-f0-9]{64}$/.test(v)
    case 'cipher': { const b = unb64url(v); return !!b && b.length >= 16 && b.length <= 16384 }
    case 'int': return typeof v === 'number' && Number.isSafeInteger(v) && v > 0
    case 'zero': return v === 0
    case 'caps': return JSON.stringify(v) === '["control","owner.devices"]'
    case 'mode': return v === 'command' || v === 'device'
    case 'role': return v === 'host' || v === 'device'
    case 'path': return v === '/v1/personal-relay/host' || v === '/v1/personal-relay/device'
    case 'reason': return v === 'ownerRevoked' || v === 'relink' || v === 'issuerRevoked'
    case 'phase': return typeof v === 'string' && ['open', 'personalConfirmed', 'hubPending', 'published', 'hubActive', 'canceled', 'expired', 'refused', 'revoking', 'revoked'].includes(v)
    default: return false
  }
}
export class PersonalVerifier {
  private readonly C: PersonalContext
  private readonly keys: PersonalKeys
  private readonly verified = new WeakSet<PersonalSigned>()
  constructor(context: PersonalContext, keys: PersonalKeys) {
    this.C = freeze(JSON.parse(JSON.stringify(context))) as PersonalContext
    this.keys = Object.freeze({ ...keys })
    for (const k of Object.values(keys)) need(unb64url(k, 32))
    need(keys.issuer !== keys.hub && keys.app !== keys.hub)
  }
  verify(kind: PersonalKind, raw: Uint8Array, time: PersonalTime, reader: 'app' | 'hub' = 'app'): PersonalSigned {
    const copy = Buffer.from(raw), value = personalCanonical(copy)
    need(Array.isArray(value) && value.length === 2 && Array.isArray(value[0]))
    const p: unknown[] = value[0], [role, fields, at, end, max] = WIRE_SHAPES[kind], tokens = fields.split(' ')
    need(p.length === tokens.length + 2 && p[0] === personalDomain(kind) && field('C', p[1], this.C))
    need(tokens.every((t, i) => field(t, p[i + 2], this.C)))
    const signer = role === 'E' ? this.keys.issuer : role === 'A' ? this.keys.app : role === 'H' ? this.keys.hub :
      role === 'reader' ? this.keys[reader] : p[2] === 'host' ? this.keys.hub : this.keys.app
    need(verifyEd25519(unb64url(signer, 32)!, unb64url(value[1], 64) ?? new Uint8Array(), bytes(p)))
    const now = instant(time); need(Number.isSafeInteger(now) && now > 0)
    if (max) {
      const a = p[at] as number, b = p[end] as number
      need(a <= now + 30000 && (time.kind === 'historical' || b > now) && b > a && b - a <= max)
    }
    if (kind === 'local-ticket') need(p[7] === this.keys.hub && p[8] === this.keys.box && p[6] === computerIdOf(unb64url(this.keys.hub, 32)!))
    if (kind === 'intent') need(p[6] === this.keys.app)
    if (kind === 'channel-open') need(p[3] === 'use' && p[6] !== null && p[7] === this.keys.app && p[4] === computerIdOf(unb64url(this.keys.hub, 32)!) && p[9] === `/v1/personal-relay/${p[2]}`)
    if (kind === 'host-state-request') need((p[4] === null) === (p[5] === null))
    if (kind === 'fence' || kind === 'fence-ack') need((p[4] === null) === (p[5] === null))
    if (kind === 'time') need((p[5] as number) <= 300000 && (p[4] as number) <= now + 30000)
    if (kind === 'time' || kind === 'time-request') need(p[3] === computerIdOf(unb64url(this.keys.hub, 32)!))
    for (const index of kind === 'receipt' ? [7] : kind === 'fence' || kind === 'revocation' ? [8] : kind === 'fence-ack' ? [9] : kind === 'state' ? [15] : []) need((p[index] as number) <= now + 30000)
    const result = Object.freeze({ payload: freeze(p) as readonly unknown[], wire: copy.toString('utf8'), hash: personalHash(p) })
    this.verified.add(result)
    return result
  }
  verifyOpen(raw: Uint8Array, time: PersonalTime, request: { role: 'host' | 'device'; kid: string; httpNonce: string; pathAndQuery: string; httpUntil: number }): PersonalSigned {
    need(time.kind === 'current')
    const s = this.verify('channel-open', raw, time), p = s.payload
    need(request.kid === (request.role === 'host' ? this.keys.hub : this.keys.app) && p[2] === request.role &&
      p[8] === request.httpNonce && p[9] === request.pathAndQuery && (p[12] as number) <= request.httpUntil)
    return s
  }
  tuple(I: PersonalSigned, Q: PersonalSigned): readonly unknown[] {
    need(this.verified.has(I) && this.verified.has(Q))
    const a = I.payload, b = Q.payload
    need(a[0] === personalDomain('intent') && b[0] === personalDomain('local-ticket') && a[2] === b[2] && a[3] === b[3])
    const t = freeze([this.C, a[2], a[3], I.hash, Q.hash, a[4], a[5], a[6], a[7], b[4], b[5], b[6], b[7], b[8], b[9], 'first-owner', a[10], a[8], Math.min(a[9] as number, b[12] as number)]) as readonly unknown[]
    need(field('T', t, this.C))
    return t
  }
}

/** POP: A cria B com epk/segredo novos; H abre somente B já verificado e da mesma T/CA/CH. */
function boxAAD(B: PersonalSigned): Buffer {
  const p = B.payload
  need(p[0] === personalDomain('box-challenge'))
  return bytes([personalDomain('box-aad'), p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[9], p[10]])
}
export function personalBoxSeal(epk: Ephemeral, hubBox: string, aadFields: readonly unknown[], secret: string): string {
  need(aadFields.length === 10 && aadFields[0] === personalDomain('box-aad') && aadFields[6] === b64url(epk.pub) &&
    aadFields[7] === personalHash([personalDomain('box-secret-hash'), secret]) && unb64url(secret, 32))
  const aad = bytes(aadFields), dh = epk.dh(unb64url(hubBox, 32)!)
  const key = new Uint8Array(hkdfSync('sha256', dh, createHash('sha256').update(aad).digest(), Buffer.from(personalDomain('box-key')), 32))
  try { return b64url(chacha20poly1305(key, new Uint8Array(12), aad).encrypt(bytes([personalDomain('box-secret-plaintext'), aadFields[2], secret]))) }
  finally { dh.fill(0); key.fill(0) }
}
export function personalBoxOpen(hubPrivate: Uint8Array, B: PersonalSigned, hT: string, hCA: string, hCH: string): string {
  const p = B.payload
  need(p[2] === hT && p[3] === hCA && p[4] === hCH)
  const aad = boxAAD(B), dh = ephemeral(hubPrivate).dh(unb64url(p[6], 32)!)
  const key = new Uint8Array(hkdfSync('sha256', dh, createHash('sha256').update(aad).digest(), Buffer.from(personalDomain('box-key')), 32))
  let plain: Uint8Array | undefined
  try {
    plain = chacha20poly1305(key, new Uint8Array(12), aad).decrypt(unb64url(p[8])!)
    const value = personalCanonical(plain)
    need(Array.isArray(value) && value.length === 3 && value[0] === personalDomain('box-secret-plaintext') && value[1] === hT && unb64url(value[2], 32) &&
      personalHash([personalDomain('box-secret-hash'), value[2]]) === p[7])
    return value[2] as string
  } finally { dh.fill(0); key.fill(0); plain?.fill(0) }
}

/** Verifica originais completos; G não torna atuais os preparos no relógio de recuperação. */
export function verifyPersonalCeremony(verifier: PersonalVerifier, raw: Uint8Array, time: PersonalTime) {
  need(raw.length > 0 && raw.length <= 192 * 1024)
  const text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(Buffer.from(raw)), packet: unknown = JSON.parse(text)
  need(packet && typeof packet === 'object' && !Array.isArray(packet) && JSON.stringify(packet) === text)
  const input = packet as Record<string, unknown>, originals = new Map<PersonalKind, PersonalSigned>()
  need(Object.keys(input).every(k => Object.hasOwn(WIRE_SHAPES, k)))
  let grantTime = time
  if (Object.hasOwn(input, 'receipt')) {
    const R = verifier.verify('receipt', bytes(input.receipt), time)
    grantTime = { kind: 'committed', at: R.payload[7] as number }; originals.set('receipt', R)
  }
  const G = input.grant === undefined ? undefined : verifier.verify('grant', bytes(input.grant), grantTime)
  if (G) originals.set('grant', G)
  const prepTime: PersonalTime = G ? { kind: 'committed', at: G.payload[9] as number } : time
  const prep = ['intent', 'local-ticket', 'app-consent', 'local-consent', 'box-challenge', 'box-proof', 'approve', 'local-confirm', 'consume']
  for (const [kind, value] of Object.entries(input)) if (!originals.has(kind as PersonalKind)) originals.set(kind as PersonalKind,
    verifier.verify(kind as PersonalKind, bytes(value), prep.includes(kind) ? prepTime : ['challenge', 'proof'].includes(kind) ? grantTime : time))
  const get = (k: PersonalKind) => { const s = originals.get(k); need(s); return s }
  const I = get('intent'), Q = get('local-ticket'), T = verifier.tuple(I, Q), hT = personalHash(T)
  const p = (k: PersonalKind) => get(k).payload
  const eq = (k: PersonalKind, index: number, expected: unknown) => need(JSON.stringify(p(k)[index]) === JSON.stringify(expected))
  const h = (k: PersonalKind) => get(k).hash
  const preceding = (k: PersonalKind, earlier: PersonalKind[]) => {
    const shape = WIRE_SHAPES[k], value = p(k)
    need((value[shape[2]] as number) >= (T[17] as number) && (value[shape[3]] as number) <= (T[18] as number))
    for (const prev of earlier) {
      const s = WIRE_SHAPES[prev], old = p(prev)
      need((value[shape[2]] as number) >= (old[s[2]] as number) && (value[shape[3]] as number) <= (old[s[3]] as number))
    }
  }
  for (const k of ['app-consent', 'local-consent', 'box-challenge', 'box-proof', 'approve', 'local-confirm', 'consume'] as const) if (originals.has(k)) eq(k, 2, hT)
  if (originals.has('app-consent')) preceding('app-consent', [])
  if (originals.has('local-consent')) { eq('local-consent', 3, h('app-consent')); preceding('local-consent', ['app-consent']) }
  if (originals.has('box-challenge')) { eq('box-challenge', 3, h('app-consent')); eq('box-challenge', 4, h('local-consent')); preceding('box-challenge', ['app-consent', 'local-consent']) }
  if (originals.has('box-proof')) { eq('box-proof', 3, h('box-challenge')); eq('box-challenge', 7, personalHash([personalDomain('box-secret-hash'), p('box-proof')[4]])); preceding('box-proof', ['box-challenge']) }
  if (originals.has('approve')) { eq('approve', 3, h('box-challenge')); eq('approve', 4, h('box-proof')); preceding('approve', ['box-challenge', 'box-proof']) }
  if (originals.has('local-confirm')) { eq('local-confirm', 3, h('approve')); eq('local-confirm', 4, h('box-proof')); preceding('local-confirm', ['approve', 'box-proof', 'local-consent']) }
  if (originals.has('consume')) { eq('consume', 3, h('approve')); eq('consume', 4, h('local-confirm')); eq('consume', 5, T[16]); preceding('consume', ['approve', 'local-confirm']) }
  if (G) {
    eq('grant', 2, T)
    for (const [i, k] of ['box-challenge', 'box-proof', 'approve', 'local-confirm', 'consume'].entries()) eq('grant', i + 3, h(k as PersonalKind))
    need((G.payload[9] as number) >= (p('consume')[7] as number) && (G.payload[10] as number) <= (T[18] as number))
    if (originals.has('challenge')) { eq('challenge', 2, G.hash); eq('challenge', 3, hT); preceding('challenge', ['grant']) }
    if (originals.has('proof')) { eq('proof', 2, G.hash); eq('proof', 3, h('challenge')); eq('proof', 4, hT); eq('proof', 5, h('approve')); eq('proof', 6, h('local-confirm')); preceding('proof', ['challenge']) }
    if (originals.has('receipt')) {
      eq('receipt', 2, hT); eq('receipt', 3, G.hash); eq('receipt', 4, h('challenge')); eq('receipt', 5, h('proof'))
      need((p('receipt')[7] as number) >= (p('proof')[7] as number) && (p('receipt')[7] as number) < (G.payload[10] as number))
      eq('receipt', 11, personalHash([personalDomain('pending'), T[0], hT, p('receipt')[6], G.hash, h('challenge'), h('proof'), p('receipt')[7], p('receipt')[8], 'pendingPublication']))
    }
  }
  for (const k of ['activation', 'host-state'] as const) if (originals.has(k)) {
    eq(k, 2, hT)
    if (k === 'activation') {
      eq(k, 3, p('receipt')[6]); eq(k, 4, h('receipt'))
      for (let i = 5; i <= 9; i++) eq(k, i, T[i === 9 ? 10 : i === 8 ? 11 : i])
      need((p(k)[10] as number) > (p('receipt')[8] as number) && (p(k)[11] as number) > (p('receipt')[9] as number) && (p(k)[13] as number) >= (p('receipt')[7] as number))
    } else {
      eq(k, 3, h('grant')); eq(k, 4, p('receipt')[6]); eq(k, 5, h('receipt')); eq(k, 6, h('publish'))
      for (const [target, source] of [[7, 5], [8, 6], [9, 7], [10, 11], [11, 10], [13, 16]]) eq(k, target, T[source])
      eq(k, 15, p('host-state-request')[6])
      need((p(k)[16] as number) >= (p('host-state-request')[7] as number) && (p(k)[16] as number) < (p('host-state-request')[8] as number) && (p(k)[16] as number) >= (p('receipt')[7] as number))
    }
  }
  if (originals.has('publish')) { eq('publish', 2, T[1]); eq('publish', 3, hT); eq('publish', 4, h('grant')); eq('publish', 5, p('receipt')[6]); eq('publish', 6, h('receipt')) }
  if (originals.has('host-state-request')) { eq('host-state-request', 2, hT); eq('host-state-request', 3, h('grant')); eq('host-state-request', 4, p('receipt')[6]); eq('host-state-request', 5, h('receipt')) }
  if (originals.has('channel-open')) { eq('channel-open', 4, T[11]); eq('channel-open', 5, hT); eq('channel-open', 6, p('receipt')[6]); eq('channel-open', 7, T[7]) }
  if (originals.has('activation') && originals.has('host-state')) need((p('activation')[13] as number) >= (p('host-state')[16] as number) && (p('activation')[14] as number) <= (p('host-state')[17] as number))
  for (const k of ['revoke', 'fence', 'fence-ack', 'revocation'] as const) if (originals.has(k)) {
    if (k === 'revoke') { eq(k, 2, T[1]); eq(k, 3, hT); eq(k, 4, h('grant')) }
    else {
      eq(k, 2, hT)
      if (k === 'fence') eq(k, 3, h('grant'))
      else if (k === 'fence-ack') eq(k, 3, h('fence'))
      if (k === 'revocation') { eq(k, 3, p('receipt')[6]); eq(k, 4, h('receipt')); eq(k, 9, p('fence')[7]) }
      else if (p(k)[4] !== null) { eq(k, 4, p('receipt')[6]); eq(k, 5, h('receipt')) }
    }
  }
  for (const k of ['create', 'begin', 'attach', 'cancel', 'read', 'state'] as const) if (originals.has(k)) {
    eq(k, 2, T[1])
    if (k === 'create') { eq(k, 3, T[8]); need(T[2] === 'command') }
    if (k === 'begin') { eq(k, 3, T[9]); eq(k, 4, Q.hash); eq(k, 5, T[14]); need(T[2] === 'device') }
    if (k === 'attach') { eq(k, 3, Q.hash); eq(k, 4, T[8]); need(T[2] === 'device') }
    if (k === 'cancel') eq(k, 3, T[16])
    if (k === 'read' || k === 'state') { eq(k, 3, T[9]); eq(k, 4, hT) }
    if (k === 'state') {
      for (const [i, kind] of [[8, 'grant'], [9, 'receipt'], [10, 'activation'], [11, 'revocation'], [12, 'fence'], [13, 'fence-ack']] as const) if (p(k)[i] !== null) eq(k, i, h(kind))
      if (p(k)[7] !== null) eq(k, 7, p('receipt')[6])
      if (originals.has('read')) eq(k, 14, p('read')[5])
      // S é observação histórica; nenhum método devolve autoridade a partir dela.
    }
  }
  if (originals.has('time') && originals.has('time-request')) { eq('time', 2, p('time-request')[2]); eq('time', 3, p('time-request')[3]) }
  if (originals.has('fence-ack')) {
    eq('fence-ack', 4, p('fence')[4])
    eq('fence-ack', 5, p('fence')[5])
    need((p('fence-ack')[9] as number) >= (p('fence')[8] as number))
    if (originals.has('receipt')) need((p('fence-ack')[6] as number) > (p('receipt')[8] as number) && (p('fence-ack')[7] as number) > (p('receipt')[9] as number))
  }
  if (originals.has('revocation')) {
    need((p('revocation')[8] as number) >= (p('fence')[8] as number) && (p('revocation')[5] as number) > (p('receipt')[8] as number) && (p('revocation')[6] as number) > (p('receipt')[9] as number))
    if (originals.has('fence-ack')) for (const [a, b] of [[5, 6], [6, 7], [7, 8]]) eq('revocation', a, p('fence-ack')[b])
  }
  // Retornar originais por cópia imutável; Map interna nunca sai para chamador/renderer.
  return Object.freeze({ tuple: T, hash: hT, originals: Object.freeze(Object.fromEntries(originals)) as Readonly<Partial<Record<PersonalKind, PersonalSigned>>> })
}

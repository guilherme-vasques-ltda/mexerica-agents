/** PR-B: arrays fechados e cifra própria, sem hs1/hs2 nem AAD legado. Somente Main. */
import { hkdfSync, randomBytes } from 'node:crypto'
import { chacha20poly1305 } from '@noble/ciphers/chacha.js'
import { b64url, unb64url, ephemeral, type DeviceSigner, type Ephemeral } from './crypto.ts'
import * as w from './personal-link-wire.ts'
export const domain = (name: string) => `donna.personal-link/${name}/v1`
export function refused(): never { throw new Error('personalLinkChannelRefused') }
export function same(a: w.J, b: w.J) { return w.j([a]).equals(w.j([b])) }
const text = (v: w.J): string => typeof v === 'string' ? v : refused()
const key = (v: w.J, n: number) => unb64url(v, n) ?? refused()
export interface Root { readonly H: string; readonly G: string; readonly C: string; readonly T: string; readonly ra: string; readonly chain: string }
export function chain(records: readonly w.J[], T: string, C: string): Root {
  if (records.length !== 7) refused()
  const [raV, rcV, mV, mrV, sealedAccept, pcV, sealedM] = records
  if (!Array.isArray(sealedAccept) || sealedAccept.length !== 2) refused()
  const chV = sealedAccept[0], acV = sealedAccept[1]
  if (!Array.isArray(raV) || !Array.isArray(raV[0])) refused()
  const H = text(raV[0][2])
  key(H, 32)
  const ch = w.verifySigned(chV, w.D430.challenge, 14, H)
  w.checkChallenge(ch, { T, C, nT: text(ch[0][8]) })
  const ac = w.verifySigned(acV, w.D430.accept, 12, T)
  // Recriar apenas payload esperado para conferir, preservando envelopes originais.
  if (!same(ac[0], [w.D430.accept, 1, ...ch[0].slice(2, 10), w.hashJ(ch), w.P0])) refused()
  deadline(ch[0][10], ch[0][5], 30000)
  if (typeof ch[0][12] !== 'number' || ch[0][12] < 1) refused()
  const ra = w.checkAdmission(raV, ch, ac)
  const pc = w.verifySigned(pcV, w.D430.persisted, 12, T), r = ra[0]
  if (!same(pc[0].slice(2, 5), r.slice(2, 5)) || pc[0][6] !== r[6] || pc[0][7] !== T || pc[0][8] !== r[10] || pc[0][9] !== w.hashJ(ra)) refused()
  key(pc[0][5], 32); key(pc[0][10], 32); deadline(pc[0][11], pc[0][5], 30000)
  const rc = w.checkComplete(rcV, ra, pc)
  const m = w.verifySigned(mV, w.D430.management, 14, T)
  w.checkManagement(m[0], ra)
  deadline(m[0][12], m[0][5], 30000)
  if (!same(m, sealedM)) refused()
  const mr = w.checkReceipt(mrV, ra, m)
  return Object.freeze({ H, G: text(r[3]), C, T, ra: w.hashJ(ra), chain: w.chainDigestT(ra, rc, m, mr) })
}
export const open = (r: Root): w.J => [domain('channel-open'), 1, 'manager', r.H, r.G, r.C, r.T, r.ra, r.chain]
function deadline(d: w.J, R: w.J, max: number) {
  if (!Array.isArray(d) || d.length !== 6 || d[0] !== w.D430.deadline || d[1] !== 1 || !/^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/.test(text(d[2])) || d[3] !== R || typeof d[4] !== 'number' || !Number.isSafeInteger(d[4]) || d[4] < 0 || typeof d[5] !== 'number' || !Number.isSafeInteger(d[5]) || d[5] <= 0 || d[5] > max) refused()
  return d[5] as number
}
export function challenge(v: w.J, r: Root): w.Signed {
  const ch = w.verifySigned(v, domain('channel-challenge'), 16, r.H), p = ch[0]
  if (p[2] !== 'manager' || p[3] !== r.H || p[4] !== r.G || p[5] !== r.C || p[10] !== r.T || p[11] !== r.ra || p[12] !== r.chain || !same(p[13], w.P1) || p[14] !== 0) refused()
  for (const i of [6, 7, 8, 9]) key(p[i], 32)
  if (deadline(p[15], p[6], 30000) !== 30000) refused()
  return ch
}
export function response(signer: DeviceSigner, ch: w.Signed, eph: Ephemeral): w.Signed {
  const p = ch[0]
  return w.signT(signer, [domain('channel-response'), 1, w.hashJ(ch), p[3], p[4], p[5], p[6], 'manager', p[10], b64url(eph.pub), b64url(randomBytes(32)), p[11], p[12], p[13], 0])
}
export function welcome(v: w.J, r: Root, ch: w.Signed, cr: w.Signed) {
  const s = w.verifySigned(v, domain('channel-welcome'), 13, r.H), p = s[0]
  if (p[2] !== r.H || p[3] !== r.G || p[4] !== r.C || p[5] !== ch[0][6] || p[6] !== 'manager' || p[7] !== r.T || p[8] !== w.hashJ([domain('channel-transcript'), 1, ch, cr]) || !same(p[11], w.P1) || p[12] !== 0) refused()
  key(p[9], 16)
  return { signed: s, ttl: deadline(p[10], p[5], 300000) }
}
/** Só mantém bytes apagáveis entre awaits; handles nativos de DH são temporários. */
export class ChannelEphemeral implements Ephemeral {
  private readonly secret = randomBytes(32)
  private ended = false
  readonly pub = ephemeral(this.secret).pub
  dh(peer: Uint8Array): Uint8Array {
    if (this.ended) refused()
    try { return ephemeral(this.secret).dh(peer) } finally { this.dispose() }
  }
  dispose() { this.secret.fill(0); this.ended = true }
}
export class Cipher {
  private counter = 0
  private readonly key: Buffer
  private readonly transcript: w.J
  private readonly session: string
  private readonly direction: 'c2h' | 'h2c'
  constructor(shared: Uint8Array, transcript: w.J, session: string, direction: 'c2h' | 'h2c') {
    this.transcript = transcript
    this.session = session
    this.direction = direction
    const th = w.hashJ(transcript)
    this.key = Buffer.from(hkdfSync('sha256', shared, Buffer.from(th, 'hex'), w.j([domain('channel-key'), 1, 'manager', direction, th, session]), 32))
  }
  dispose() { this.key.fill(0); this.counter = 0xffffffff }
  private context() {
    if (this.counter >= 0xffffffff) refused()
    const nonce = Buffer.alloc(12); nonce.writeBigUInt64BE(BigInt(this.counter), 4)
    return { nonce, aad: w.j([domain('frame'), 1, w.hashJ(this.transcript), this.session, 'manager', this.direction, this.counter]) }
  }
  seal(v: w.J): Buffer {
    const { nonce, aad } = this.context()
    const raw = Buffer.from(JSON.stringify([this.session, this.counter, b64url(chacha20poly1305(this.key, nonce, aad).encrypt(w.j(v)))]))
    if (raw.length + 8 > w.LIMIT) refused()
    this.counter++; return raw
  }
  read(raw: Uint8Array): w.J {
    if (raw.byteLength + 4 > w.LIMIT) refused()
    const string = new TextDecoder('utf-8', { fatal: true }).decode(raw)
    // Frame só tem strings e um inteiro: rejeita estruturas aninhadas antes do JSON.parse.
    if (!/^\["[A-Za-z0-9_-]{22}",[0-9]{1,10},"[A-Za-z0-9_-]+"\]$/.test(string)) refused()
    const v: unknown = JSON.parse(string)
    if (!Array.isArray(v) || v.length !== 3 || v[0] !== this.session || v[1] !== this.counter || JSON.stringify(v) !== string) refused()
    const bytes = unb64url(v[2]); if (!bytes) refused()
    const { nonce, aad } = this.context()
    const out = w.readJ(chacha20poly1305(this.key, nonce, aad).decrypt(bytes))
    this.counter++; return out
  }
}
export function status(v: w.J, r: Root, id: string, session: string, th: string, R: w.J, D: w.J) {
  if (!Array.isArray(v) || v.length !== 16 || v[0] !== domain('link-status') || v[1] !== 1 || v[2] !== id || v[3] !== r.H || v[4] !== r.G || v[5] !== r.C || v[6] !== R || v[7] !== r.T || v[8] !== r.ra || v[9] !== r.chain || !same(v[10], w.P1) || v[11] !== 0 || v[12] !== session || v[13] !== th || v[14] !== 'active' || !same(v[15], D)) refused()
}

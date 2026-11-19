/** D430 PR-A, lado T: objetos EXATOS do complemento v2 (sem assinador genérico). Puro, sem Electron. */
import { createHash } from 'node:crypto'
import { b64url, unb64url, verifyEd25519, type DeviceSigner } from './crypto.ts'

export const D430 = {
  challenge: 'donna.personal-link/initial-challenge/v1',
  accept: 'donna.personal-link/initial-accept/v1',
  management: 'donna.personal-link/initial-management/v1',
  admission: 'donna.personal-link/root-admission/v1',
  persisted: 'donna.personal-link/initial-persisted/v1',
  complete: 'donna.personal-link/initial-complete/v1',
  receipt: 'donna.personal-link/initial-management-receipt/v1',
  deadline: 'donna.personal-link/deadline/v1',
  procedure: 'donna.personal-link/procedure/v1',
} as const
/** SHA256 dos bytes exatos de host/src/fork/personal_link/policy-v1.json (LF incluído). */
export const POLICY_DIGEST = '9de2e27e0cc8fafb44190b22e691d6204843fcac3fe1bf4e337c7ea8950b263b'
export const LIMIT = 64 * 1024
export type J = string | number | boolean | readonly J[]
export type Signed = readonly [readonly J[], string]

const policy = (rights: readonly string[], revision: number): J => ['donna.personal-link/core', 1, POLICY_DIGEST, rights, false, revision]
export const P0: J = policy(['link.connect'], 1)
export const P1: J = policy(['devices.manage', 'link.connect'], 2)

function fail(): never { throw new Error('personalLinkWire') }
function valid(v: unknown, depth: number): v is J {
  if (depth > 12) return false
  if (typeof v === 'string') return /^[A-Za-z0-9._/:-]{1,1024}$/.test(v)
  if (typeof v === 'number') return Number.isSafeInteger(v) && v >= 0
  if (typeof v === 'boolean') return true
  return Array.isArray(v) && v.every(x => valid(x, depth + 1))
}
/** Bytes recebidos precisam ser exatamente J(valor). */
export function readJ(raw: Uint8Array): J {
  if (raw.byteLength < 1 || raw.byteLength > LIMIT) fail()
  const text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(raw)
  // Antes de JSON.parse: limite estrutural, sem alocar arrays profundos recebidos da rede.
  let depth = 0, quoted = false, escaped = false
  for (const char of text) {
    if (quoted) { if (escaped) escaped = false; else if (char === '\\') escaped = true; else if (char === '"') quoted = false }
    else if (char === '"') quoted = true
    else if (char === '[' || char === '{') { if (++depth > 13) fail() }
    else if (char === ']' || char === '}') depth = Math.max(0, depth - 1)
  }
  const value: unknown = JSON.parse(text)
  if (!Array.isArray(value) || !valid(value, 0) || JSON.stringify(value) !== text) fail()
  return value
}
export function j(value: J): Buffer {
  if (!Array.isArray(value) || !valid(value, 0)) fail()
  const bytes = Buffer.from(JSON.stringify(value))
  if (bytes.length > LIMIT) fail()
  return bytes
}
export const hashJ = (value: J) => createHash('sha256').update(j(value)).digest('hex')
const same = (a: J, b: J) => j([a]).equals(j([b]))

/** Envelope [payload, assinatura]; confere domínio, aridade e Ed25519 da chave esperada. */
export function verifySigned(value: J, domain: string, arity: number, key: string): Signed {
  if (!Array.isArray(value) || value.length !== 2 || !Array.isArray(value[0])) fail()
  const p = value[0] as readonly J[], text = value[1], sig = unb64url(text, 64), pub = unb64url(key, 32)
  if (typeof text !== 'string' || p.length !== arity || p[0] !== domain || p[1] !== 1 || !sig || !pub || !verifyEd25519(pub, sig, j(p))) fail()
  return [p, text]
}
export function signT(signer: DeviceSigner, payload: readonly J[]): Signed {
  return [payload, b64url(signer.sign(j(payload)))]
}

export interface RootRequest { readonly T: string; readonly C: string; readonly nT: string }
const deadline = (d: J, R: J) => Array.isArray(d) && d.length === 6 && d[0] === D430.deadline && d[1] === 1 && typeof d[2] === 'string' && d[3] === R && Number.isSafeInteger(d[4]) && d[5] === 30000

/** Desafio H: H aprendida só pelo canal administrativo ancorado; T/C/N_T/P0 precisam ser os nossos. */
export function checkChallenge(value: J, req: RootRequest): Signed {
  const p0 = (Array.isArray(value) && Array.isArray(value[0]) ? value[0] : fail()) as readonly J[]
  const H = typeof p0[2] === 'string' ? p0[2] : fail()
  const ch = verifySigned(value, D430.challenge, 14, H), p = ch[0]
  if (!unb64url(p[3], 32) || p[4] !== req.C || !unb64url(p[5], 32) || !unb64url(p[6], 16) || p[7] !== req.T || p[8] !== req.nT || !unb64url(p[9], 32) || !deadline(p[10], p[5]) || !unb64url(p[11], 16) || !Number.isSafeInteger(p[12]) || !same(p[13], P0) || H === req.T) fail()
  return ch
}
export function accept(signer: DeviceSigner, ch: Signed): Signed {
  const p = ch[0]
  return signT(signer, [D430.accept, 1, p[2], p[3], p[4], p[5], p[6], p[7], p[8], p[9], hashJ(ch), P0])
}
export function checkAdmission(value: J, ch: Signed, ac: Signed): Signed {
  const c = ch[0], ra = verifySigned(value, D430.admission, 14, c[2] as string), p = ra[0]
  if (![2, 3, 4, 5, 6, 7].every(i => p[i] === c[i]) || p[8] !== c[11] || p[9] !== c[12] || !unb64url(p[10], 16) || p[11] !== hashJ(ch) || p[12] !== hashJ(ac) || !same(p[13], P0)) fail()
  return ra
}
/** PC: desafio novo de H e runtime atual; mesma admissão/digest. */
export function persisted(signer: DeviceSigner, ra: Signed, response: J): Signed {
  const r = ra[0]
  if (!Array.isArray(response) || response.length !== 3 || !unb64url(response[0], 32) || !unb64url(response[2], 32) || !deadline(response[1], response[2])) fail()
  return signT(signer, [D430.persisted, 1, r[2], r[3], r[4], response[2], r[6], r[7], r[10], hashJ(ra), response[0], response[1]])
}
export function checkComplete(value: J, ra: Signed, pc: Signed): Signed {
  const r = ra[0], rc = verifySigned(value, D430.complete, 10, r[2] as string), p = rc[0]
  if (![2, 3, 4].every(i => p[i] === r[i]) || p[5] !== pc[0][5] || p[6] !== r[10] || p[7] !== hashJ(ra) || p[8] !== hashJ(pc) || !same(p[9], P0)) fail()
  return rc
}
/** Payload de gestão vindo de H: tudo conferido contra a raiz já reaberta antes do gesto. */
export function checkManagement(value: J, ra: Signed): readonly J[] {
  const r = ra[0]
  if (!Array.isArray(value) || value.length !== 14 || value[0] !== D430.management || value[1] !== 1) fail()
  const p = value as readonly J[]
  if (![2, 3, 4].every(i => p[i] === r[i]) || !unb64url(p[5], 32) || !unb64url(p[6], 16) || p[7] !== r[7] || p[8] !== hashJ(ra) || p[9] !== 1 || !same(p[10], P1) || !unb64url(p[11], 32) || !deadline(p[12], p[5]) || p[13] !== r[9]) fail()
  return p
}
export function checkReceipt(value: J, ra: Signed, m: Signed): Signed {
  const r = ra[0], g = m[0], mr = verifySigned(value, D430.receipt, 12, r[2] as string), p = mr[0]
  if (![2, 3, 4].every(i => p[i] === r[i]) || p[5] !== g[5] || p[6] !== g[6] || p[7] !== r[7] || p[8] !== r[8] || p[9] !== hashJ(ra) || p[10] !== hashJ(m) || !same(p[11], P1)) fail()
  return mr
}
/** chainDigestT = SHA256(J([signedRA,signedRC,signedManagement,signedMR])), sem J duplo. */
export const chainDigestT = (ra: Signed, rc: Signed, m: Signed, mr: Signed) => hashJ([ra, rc, m, mr])
/** Impressão de 128 bits (32 hex em 8 grupos de 4) para o dono conferir H/T; a chave completa também é mostrada. */
export const fingerprint = (key: string) => createHash('sha256').update(unb64url(key, 32) ?? fail()).digest('hex').slice(0, 32).replace(/(.{4})/g, '$1 ').trim()

import { ed25519 } from '@noble/curves/ed25519.js'
import { bytesToNumberLE } from '@noble/curves/utils.js'

/**
 * Política estrita de verificação Ed25519, comum a todos os backends (TS puro, `@noble/curves`):
 * o que o OpenSSL/BoringSSL aceitam varia por versão (OpenSSL 3.0 aceita chave de ordem baixa;
 * o BoringSSL do Electron pode aceitar), então a checagem roda na camada do protocolo, ANTES de
 * qualquer backend. Recusa:
 *  - tamanhos errados;
 *  - `S >= L`;
 *  - `A` ou `R` com encoding não canônico (`y >= p`, sinal em `x = 0`) ou fora da curva;
 *  - `A` ou `R` de ordem baixa.
 * A equação em si (sem cofator, `R` torcido recusado) fica com o backend; o noble a faz em
 * `backend-noble.ts`. Casos cruzados em `ed25519-strict.test.ts`.
 */

const { Point } = ed25519
const L = Point.Fn.ORDER

export interface StrictPoints {
  a: InstanceType<typeof Point>
  r: InstanceType<typeof Point>
  s: bigint
}

/** Decodifica e valida `A`, `R` e `S`; `null` quando a política estrita recusa. */
export function strictInputs(pub: Uint8Array, sig: Uint8Array): StrictPoints | null {
  if (pub.length !== 32 || sig.length !== 64) return null
  try {
    const s = bytesToNumberLE(sig.subarray(32, 64))
    if (s >= L) return null
    const a = Point.fromBytes(pub, false)
    const r = Point.fromBytes(sig.subarray(0, 32), false)
    if (a.isSmallOrder() || r.isSmallOrder()) return null
    return { a, r, s }
  } catch {
    return null
  }
}

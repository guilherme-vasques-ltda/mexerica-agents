import { ed25519, x25519 } from '@noble/curves/ed25519.js'
import { hkdf } from '@noble/hashes/hkdf.js'
import { hmac } from '@noble/hashes/hmac.js'
import { bytesToNumberLE } from '@noble/curves/utils.js'
import { sha256, sha512 } from '@noble/hashes/sha2.js'
import type { CryptoBackend } from './backend.ts'
import { strictInputs } from './ed25519-strict.ts'

/**
 * Backend do protocolo da ponte para a web: `@noble/curves` (Ed25519/X25519),
 * `@noble/hashes` (SHA-256/HMAC/HKDF) e `crypto.getRandomValues`. Os vetores
 * compartilhados conferem a equivalência com `backend-node.ts` em `protocol.test.ts`.
 */

const randomBytes = (n: number) => crypto.getRandomValues(new Uint8Array(n))

const { Point } = ed25519
const L = Point.Fn.ORDER

/**
 * Equação Ed25519 SEM cofator (`[S]B − [k]A == R`), como o OpenSSL: o `ed25519.verify` do noble usa
 * ZIP-215 e a equação com cofator — `{ zip215: false }` sozinho ainda aceita `R` com componente de
 * torção. A política estrita (A/R canônicos e de ordem não baixa, S < L) vem de `strictInputs`, que
 * o protocolo também aplica antes de qualquer backend; aqui ela vale de novo para o backend isolado.
 * `A` com torção que NÃO seja de ordem baixa passa, como no OpenSSL.
 */
function strictVerify(pub: Uint8Array, sig: Uint8Array, message: Uint8Array): boolean {
  const inputs = strictInputs(pub, sig)
  if (!inputs) return false
  try {
    const k = bytesToNumberLE(sha512.create().update(sig.subarray(0, 32)).update(pub).update(message).digest()) % L
    return Point.BASE.multiplyUnsafe(inputs.s).subtract(inputs.a.multiplyUnsafe(k)).equals(inputs.r)
  } catch {
    return false
  }
}

export const nobleBackend: CryptoBackend = {
  sha256: (bytes) => sha256(bytes),
  hmacSha256: (key, data) => hmac(sha256, key, data),
  hkdfSha256: (ikm, salt, info, len) => hkdf(sha256, ikm, salt, info, len),
  random: randomBytes,
  ed25519: {
    signerFromSeed(seed) {
      if (seed.length !== 32) throw new Error('ed25519.seedLength')
      // Cópia própria: o chamador pode zerar `seed` depois (o noble reaproveita o buffer recebido).
      const priv = Uint8Array.from(seed)
      return {
        publicKey: ed25519.getPublicKey(priv),
        sign: (message) => ed25519.sign(message, priv),
      }
    },
    newSeed: () => randomBytes(32),
    verify(pub, sig, message) {
      if (pub.length !== 32 || sig.length !== 64) return false
      return strictVerify(pub, sig, message)
    },
  },
  x25519: {
    ephemeral(priv?: Uint8Array) {
      if (priv && priv.length !== 32) throw new Error('x25519.invalidKey')
      const privateKey = priv ? Uint8Array.from(priv) : randomBytes(32)
      return {
        pub: x25519.getPublicKey(privateKey),
        dh(peer) {
          if (peer.length !== 32) throw new Error('x25519.invalidKey')
          const ss = x25519.getSharedSecret(privateKey, peer)
          if (ss.every((b) => b === 0)) throw new Error('x25519.zeroSecret')
          return ss
        },
      }
    },
  },
}

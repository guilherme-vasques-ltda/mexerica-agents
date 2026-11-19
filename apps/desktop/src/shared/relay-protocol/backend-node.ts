import {
  createHash,
  createHmac,
  createPrivateKey,
  createPublicKey,
  diffieHellman,
  generateKeyPairSync,
  hkdfSync,
  randomBytes,
  sign as edSign,
  verify as edVerify,
  type KeyObject,
} from 'node:crypto'
import { unb64url } from './protocol.ts'
import type { CryptoBackend, DeviceSigner, Ephemeral } from './backend.ts'

/**
 * Backend do protocolo da ponte sobre `node:crypto` — o comportamento que o app desktop
 * sempre teve (era `main/fork-cloud/crypto.ts`). Só o processo principal do Electron
 * (e testes Node) podem ligá-lo; a web usa `backend-noble.ts`.
 */

// Prefixos DER fixos para importar chaves cruas de 32 bytes.
const ED_PKCS8 = Buffer.from('302e020100300506032b657004220420', 'hex')
const ED_SPKI = Buffer.from('302a300506032b6570032100', 'hex')
const X_PKCS8 = Buffer.from('302e020100300506032b656e04220420', 'hex')
const X_SPKI = Buffer.from('302a300506032b656e032100', 'hex')

const rawPublic = (key: KeyObject) => new Uint8Array(Buffer.from((key.export({ format: 'jwk' }) as { x: string }).x, 'base64url'))

const edSignerFromKey = (key: KeyObject): DeviceSigner => {
  const publicKey = rawPublic(createPublicKey(key))
  return { publicKey, sign: (message) => new Uint8Array(edSign(null, message, key)) }
}

export const nodeBackend: CryptoBackend = {
  sha256: (bytes) => new Uint8Array(createHash('sha256').update(bytes).digest()),
  hmacSha256: (key, data) => new Uint8Array(createHmac('sha256', key).update(data).digest()),
  hkdfSha256: (ikm, salt, info, len) => new Uint8Array(hkdfSync('sha256', ikm, salt, info, len)),
  random: (n) => new Uint8Array(randomBytes(n)),
  ed25519: {
    signerFromSeed(seed) {
      if (seed.length !== 32) throw new Error('ed25519.seedLength')
      const key = createPrivateKey({ key: Buffer.concat([ED_PKCS8, seed]), format: 'der', type: 'pkcs8' })
      return edSignerFromKey(key)
    },
    newSeed() {
      const { privateKey } = generateKeyPairSync('ed25519')
      return unb64url((privateKey.export({ format: 'jwk' }) as { d: string }).d, 32)!
    },
    verify(pub, sig, message) {
      if (pub.length !== 32 || sig.length !== 64) return false
      try {
        return edVerify(null, message, createPublicKey({ key: Buffer.concat([ED_SPKI, pub]), format: 'der', type: 'spki' }), sig)
      } catch {
        return false
      }
    },
  },
  x25519: {
    ephemeral(priv?: Uint8Array): Ephemeral {
      const privateKey = priv
        ? createPrivateKey({ key: Buffer.concat([X_PKCS8, priv]), format: 'der', type: 'pkcs8' })
        : generateKeyPairSync('x25519').privateKey
      const pub = rawPublic(createPublicKey(privateKey))
      return {
        pub,
        dh(peer) {
          if (peer.length !== 32) throw new Error('x25519.invalidKey')
          const publicKey = createPublicKey({ key: Buffer.concat([X_SPKI, peer]), format: 'der', type: 'spki' })
          const ss = new Uint8Array(diffieHellman({ privateKey, publicKey }))
          if (ss.every((b) => b === 0)) throw new Error('x25519.zeroSecret')
          return ss
        },
      }
    },
  },
}

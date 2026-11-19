import { createPublicKey, sign as edSign, type KeyObject } from 'node:crypto'
import { nodeBackend } from '../../shared/relay-protocol/backend-node.ts'
import { createProtocol } from '../../shared/relay-protocol/protocol.ts'
import type { DeviceSigner } from '../../shared/relay-protocol/backend.ts'

/**
 * Calço do protocolo E2E isomórfico (`src/shared/relay-protocol/`): a API pública histórica
 * deste módulo, ligada ao backend `node:crypto`. A implementação vive no pacote
 * compartilhado; o desktop não muda de comportamento.
 */

const wire = createProtocol(nodeBackend)

export const b64url = wire.b64url
export const unb64url = wire.unb64url
export const concat = wire.concat
export const sha256 = wire.sha256
export const random = wire.random
export const cidRaw = wire.cidRaw
export const computerIdOf = wire.computerIdOf
export const signerFromSeed = wire.signerFromSeed
export const newDeviceSeed = wire.newDeviceSeed
export const verifyEd25519 = wire.verifyEd25519
export const ephemeral = wire.ephemeral
export const sigCanonical = wire.sigCanonical
export const signatureHeader = wire.signatureHeader
export const hs1Input = wire.hs1Input
export const transcriptHash = wire.transcriptHash
export const hkdfExtract = wire.hkdfExtract
export const channelKeys = wire.channelKeys
export const deviceHello = wire.deviceHello
export const sealFrame = wire.sealFrame
export const openFrame = wire.openFrame
export const sasCommit = wire.sasCommit
export const sasCode = wire.sasCode
export const CHUNK = wire.CHUNK
export { HandshakeError } from '../../shared/relay-protocol/protocol.ts'

export type { ChannelKeys, HelloMessage } from '../../shared/relay-protocol/protocol.ts'
export type { DeviceSigner, Ephemeral } from '../../shared/relay-protocol/backend.ts'

const rawPublic = (key: KeyObject) => new Uint8Array(Buffer.from((key.export({ format: 'jwk' }) as { x: string }).x, 'base64url'))

/** Chave Ed25519 a partir de um `KeyObject` do `node:crypto` (só o processo principal). */
export function signerFromKey(key: KeyObject): DeviceSigner {
  const publicKey = rawPublic(createPublicKey(key))
  return { publicKey, sign: (message) => new Uint8Array(edSign(null, message, key)) }
}

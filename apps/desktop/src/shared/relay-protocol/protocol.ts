import type { CryptoBackend } from './backend.ts'
import { createProtocolInternals } from './protocol-internal.ts'

export { b64url, unb64url, concat, HandshakeError, CHUNK } from './protocol-internal.ts'
export type { CryptoBackend, DeviceSigner, Ephemeral, ChannelKeys, HelloMessage } from './protocol-internal.ts'

export function createProtocol(backend: CryptoBackend) {
  return createProtocolInternals(backend).protocol
}

export type Protocol = ReturnType<typeof createProtocol>

/**
 * Backend de cripto do protocolo da ponte (docs/reference/remote-relay.md §1, §5, §6;
 * contrato `donna-acesso/v1` §2–§3). Tudo síncrono: o app desktop usa `node:crypto`
 * (`backend-node.ts`) e a web usa `@noble/*` + `crypto.getRandomValues` (`backend-noble.ts`).
 * Os vetores compartilhados (`docs/reference/fixtures/remote-relay-vectors.json`) conferem
 * os dois backends em `protocol.test.ts`.
 */

export interface DeviceSigner {
  readonly publicKey: Uint8Array
  sign(message: Uint8Array): Uint8Array
}

export interface Ephemeral {
  readonly pub: Uint8Array
  dh(peer: Uint8Array): Uint8Array
}

export interface CryptoBackend {
  sha256(bytes: Uint8Array): Uint8Array
  /** HMAC-SHA256 com `key` como chave e `data` como mensagem. */
  hmacSha256(key: Uint8Array, data: Uint8Array): Uint8Array
  /** HKDF-SHA256 (extract+expand) com `ikm`, `salt` e `info`, devolvendo `len` bytes. */
  hkdfSha256(ikm: Uint8Array, salt: Uint8Array, info: Uint8Array, len: number): Uint8Array
  random(n: number): Uint8Array
  ed25519: {
    /** Semente de 32 bytes → assinante; lança se a semente não tiver 32 bytes. */
    signerFromSeed(seed: Uint8Array): DeviceSigner
    newSeed(): Uint8Array
    verify(pub: Uint8Array, sig: Uint8Array, message: Uint8Array): boolean
  }
  x25519: {
    ephemeral(priv?: Uint8Array): Ephemeral
  }
}

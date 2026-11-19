export type KeychainFailure = 'unavailable' | 'mismatch' | 'unsupported'
export class KeychainBlocked extends Error {
  readonly reason: KeychainFailure
  constructor(reason: KeychainFailure) { super(reason); this.reason = reason }
}
export function keychainFailure(error: unknown): KeychainFailure {
  return error instanceof KeychainBlocked ? error.reason : 'unavailable'
}

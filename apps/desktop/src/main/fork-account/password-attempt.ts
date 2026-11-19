import { safeStorage } from 'electron'
import { KeychainBlocked } from '../fork-keychain/failure'
import { AccountStorage } from './storage'

// Never store the identifier or password. The encrypted reference is deployment-bound
// by AccountStorage's guard and exists before each authentication effect.
export interface PasswordAttempt {
  version: 2
  scope: 'normal' | 'restricted'
  owner: string | null
  phase: 'starting' | 'ready' | 'sent' | 'rejected'
  factorSent: boolean
  exitRequested: boolean
  credential: string | null
  id: string | null
  capture?: { sessionId: string; userId: string; credential: string }
}

function valid(value: unknown): value is PasswordAttempt {
  if (!value || typeof value !== 'object') return false
  const r = value as Record<string, unknown>
  const keys = Object.keys(r).sort().join(',')
  if (keys !== 'credential,exitRequested,factorSent,id,owner,phase,scope,version' && keys !== 'capture,credential,exitRequested,factorSent,id,owner,phase,scope,version') return false
  if (r.capture !== undefined) {
    if (r.scope !== 'restricted' || r.phase !== 'sent' || !r.capture || typeof r.capture !== 'object') return false
    const c = r.capture as Record<string, unknown>
    if (Object.keys(c).sort().join(',') !== 'credential,sessionId,userId' || typeof c.sessionId !== 'string' || !c.sessionId || typeof c.userId !== 'string' || !c.userId || typeof c.credential !== 'string' || !c.credential || c.credential !== c.credential.trim() || c.credential.toLowerCase() === 'bearer' || !/^[\x09\x20-\x7e\x80-\xff]+$/.test(c.credential)) return false
  }
  if (r.version !== 2 || (r.scope !== 'normal' && r.scope !== 'restricted') || !['starting', 'ready', 'sent', 'rejected'].includes(String(r.phase))) return false
  if (r.scope === 'normal' ? r.owner !== null : typeof r.owner !== 'string' || !r.owner || r.owner.length > 512) return false
  if (typeof r.factorSent !== 'boolean' || typeof r.exitRequested !== 'boolean' || (r.phase === 'starting' && r.factorSent) || ((r.phase === 'sent' || r.phase === 'rejected') && !r.factorSent)) return false
  const credential = r.credential
  const id = r.id
  if (r.phase === 'starting') return credential === null && id === null
  return typeof id === 'string' && id.length > 0 && id.length <= 512 && typeof credential === 'string' && credential.length > 0 && credential.length <= 16384 && credential === credential.trim() && credential.toLowerCase() !== 'bearer' && /^[\x09\x20-\x7e\x80-\xff]+$/.test(credential)
}

export class PasswordAttemptStore {
  private storage: AccountStorage
  constructor(userData: string, namespace: string) {
    this.storage = new AccountStorage(`${userData}/account-password-attempt`, namespace, sealed => { this.decode(sealed) })
  }
  private decode(sealed: Buffer): PasswordAttempt {
    if (!safeStorage.isEncryptionAvailable() || (process.platform === 'linux' && safeStorage.getSelectedStorageBackend() === 'basic_text')) throw new KeychainBlocked('unavailable')
    try {
      const value: unknown = JSON.parse(safeStorage.decryptString(sealed))
      if (!valid(value)) throw new KeychainBlocked('mismatch')
      return value
    } catch { throw new KeychainBlocked('mismatch') }
  }
  async load(): Promise<PasswordAttempt | null> {
    const saved = await this.storage.load()
    if (saved.kind === 'empty') return null
    if (saved.kind !== 'active' || saved.exitRequested) throw new Error('Attempt storage unavailable.')
    return this.decode(saved.sealed)
  }
  /** An explicit exit may archive an unbound record, but never bypass a failed decrypt. */
  async loadForExit(): Promise<PasswordAttempt | null> {
    const saved = await this.storage.load()
    if (saved.kind === 'empty' || !saved.sealed) return null
    return this.decode(saved.sealed)
  }
  async save(attempt: PasswordAttempt, current: () => void): Promise<void> {
    if (!valid(attempt) || !safeStorage.isEncryptionAvailable() || (process.platform === 'linux' && safeStorage.getSelectedStorageBackend() === 'basic_text')) throw new Error('Attempt storage unavailable.')
    const sealed = safeStorage.encryptString(JSON.stringify(attempt))
    await this.storage.retain(sealed, 'active', current)
  }
  async clear(current: () => void, explicitExit = false): Promise<void> { current(); await this.storage.clear('blocked', explicitExit); current() }
}

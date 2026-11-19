import { createHash, randomBytes } from 'node:crypto'
import { promises as fs } from 'node:fs'
import { join } from 'node:path'

const digest = (data: string | Buffer) => createHash('sha256').update(data).digest('hex')

/** A committed guard binds an encrypted file to the current public deployment configuration. */
export class AccountStorage {
  constructor(private readonly dir: string, private readonly namespace: string, private readonly verifyBeforeClear?: (sealed: Buffer) => void) {}
  private get dataPath() { return join(this.dir, 'account.bin') }
  private get guardPath() { return join(this.dir, 'account.guard') }
  private async syncDir() {
    const dir = await fs.open(this.dir, 'r')
    try { await dir.sync() } finally { await dir.close() }
  }
  private async replace(path: string, data: string | Buffer) {
    await fs.mkdir(this.dir, { recursive: true })
    const tmp = `${path}.${randomBytes(16).toString('hex')}.tmp`
    const file = await fs.open(tmp, 'wx', 0o600)
    try { await file.writeFile(data); await file.sync() } finally { await file.close() }
    try { await fs.rename(tmp, path); await this.syncDir() }
    catch (error) { await fs.rm(tmp, { force: true }).catch(() => {}); throw error }
  }
  async load(): Promise<{ kind: 'empty' } | { kind: 'revocation_unknown'; sealed?: Buffer; recoverable?: boolean; exitRequested?: true } | { kind: 'active'; sealed: Buffer; exitRequested?: true }> {
    let raw: string
    try { raw = await fs.readFile(this.guardPath, 'utf8') }
    catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') return { kind: 'revocation_unknown' }
      try { await fs.readFile(this.dataPath); return { kind: 'revocation_unknown' } }
      catch (dataError) {
        if ((dataError as NodeJS.ErrnoException).code !== 'ENOENT') return { kind: 'revocation_unknown' }
        try { return (await fs.readdir(this.dir)).some((name) => /^account-[a-f0-9]{32}\.bin$/.test(name)) ? { kind: 'revocation_unknown' } : { kind: 'empty' } }
        catch (dirError) { return (dirError as NodeJS.ErrnoException).code === 'ENOENT' ? { kind: 'empty' } : { kind: 'revocation_unknown' } }
      }
    }
    try {
      const guard = JSON.parse(raw) as { version?: unknown; namespace?: unknown; hash?: unknown; state?: unknown; file?: unknown; resumeState?: unknown; exitRequested?: unknown }
      if (guard.exitRequested !== undefined && guard.exitRequested !== true) return { kind: 'revocation_unknown' }
      const exit = guard.exitRequested === true ? { exitRequested: true as const } : {}
      const dataPath = guard.file === undefined ? this.dataPath : typeof guard.file === 'string' && /^account-[a-f0-9]{32}\.bin$/.test(guard.file) ? join(this.dir, guard.file) : null
      if (!dataPath) return { kind: 'revocation_unknown' }
      if (guard.version === 1 && guard.state === 'revocation_unknown') {
        if (guard.namespace === digest(this.namespace) && typeof guard.hash === 'string') {
          const sealed = await fs.readFile(dataPath)
          if (digest(sealed) === guard.hash) return { kind: 'revocation_unknown', sealed, ...(guard.resumeState === 'active' ? { recoverable: true } : {}), ...exit }
        }
        return { kind: 'revocation_unknown' }
      }
      if (guard.version === 1 && guard.state === 'blocked') return { kind: 'empty' }
      if (guard.version !== 1 || guard.state !== 'active' || guard.namespace !== digest(this.namespace) || typeof guard.hash !== 'string') return { kind: 'revocation_unknown' }
      const sealed = await fs.readFile(dataPath)
      return digest(sealed) === guard.hash ? { kind: 'active', sealed, ...exit } : { kind: 'revocation_unknown' }
    } catch { return { kind: 'revocation_unknown' } }
  }
  async block(state: 'blocked' | 'revocation_unknown' = 'blocked'): Promise<void> {
    await this.replace(this.guardPath, JSON.stringify({ version: 1, state }))
  }
  async save(sealed: Buffer, current: () => void): Promise<void> {
    await this.block()
    current()
    await this.replace(this.dataPath, sealed)
    current()
    await this.replace(this.guardPath, JSON.stringify({ version: 1, state: 'active', namespace: digest(this.namespace), hash: digest(sealed) }))
    current()
  }
  async quarantine(sealed: Buffer, current: () => void): Promise<void> {
    await this.block('revocation_unknown')
    current()
    await this.replace(this.dataPath, sealed)
    current()
    await this.replace(this.guardPath, JSON.stringify({ version: 1, state: 'revocation_unknown', namespace: digest(this.namespace), hash: digest(sealed) }))
    current()
  }
  /** Copy-on-write transition: the old guard remains authoritative until the new
   * encrypted payload and its directory entry are durable. Never remove the old
   * payload on failure; this also preserves a restricted reference on restart. */
  async retain(sealed: Buffer, state: 'active' | 'revocation_unknown', current: () => void): Promise<void> {
    await fs.mkdir(this.dir, { recursive: true })
    current()
    let oldPath: string | null = null
    try {
      const old = JSON.parse(await fs.readFile(this.guardPath, 'utf8')) as { file?: unknown; state?: unknown }
      if (old.state === 'active' || old.state === 'revocation_unknown') {
        if (old.file === undefined) oldPath = this.dataPath
        else if (typeof old.file === 'string' && /^account-[a-f0-9]{32}\.bin$/.test(old.file)) oldPath = join(this.dir, old.file)
      }
    } catch {}
    const file = `account-${randomBytes(16).toString('hex')}.bin`
    const path = join(this.dir, file)
    const out = await fs.open(path, 'wx', 0o600)
    try { await out.writeFile(sealed); await out.sync() } finally { await out.close() }
    await this.syncDir()
    current()
    await this.replace(this.guardPath, JSON.stringify({ version: 1, state, namespace: digest(this.namespace), hash: digest(sealed), file }))
    current()
    if (oldPath && oldPath !== path) { await fs.rm(oldPath, { force: true }).catch(() => {}); await this.syncDir().catch(() => {}) }
  }
  /** Fence the existing encrypted payload without discarding its reference. */
  async fence(exitRequested = false): Promise<void> {
    const guard = JSON.parse(await fs.readFile(this.guardPath, 'utf8')) as { version?: unknown; state?: unknown; namespace?: unknown; hash?: unknown; file?: unknown }
    if (guard.version !== 1 || guard.namespace !== digest(this.namespace) || typeof guard.hash !== 'string' || (guard.state !== 'active' && guard.state !== 'revocation_unknown')) throw new Error('Cannot fence account guard.')
    await this.replace(this.guardPath, JSON.stringify({ ...guard, state: 'revocation_unknown', ...(guard.state === 'active' ? { resumeState: 'active' } : {}), ...(exitRequested ? { exitRequested: true } : {}) }))
  }
  /** Explicit local recovery: re-open and verify the exact published guard and payload. */
  async resumeFence(current: () => void): Promise<void> {
    current()
    const raw = await fs.readFile(this.guardPath, 'utf8')
    const guard = JSON.parse(raw) as { version?: unknown; state?: unknown; namespace?: unknown; hash?: unknown; file?: unknown; resumeState?: unknown; exitRequested?: unknown }
    if (guard.exitRequested !== undefined || guard.version !== 1 || !((guard.state === 'revocation_unknown' && guard.resumeState === 'active') || guard.state === 'active') || guard.namespace !== digest(this.namespace) || typeof guard.hash !== 'string') throw new Error('Guard is not recoverable.')
    const path = guard.file === undefined ? this.dataPath : typeof guard.file === 'string' && /^account-[a-f0-9]{32}\.bin$/.test(guard.file) ? join(this.dir, guard.file) : null
    if (!path || digest(await fs.readFile(path)) !== guard.hash) throw new Error('Guard digest mismatch.')
    const handle = await fs.open(path, 'r')
    try { await handle.sync() } finally { await handle.close() }
    await this.syncDir()
    if (await fs.readFile(this.guardPath, 'utf8') !== raw) throw new Error('Guard changed.')
    current()
    if (guard.state === 'active') return
    const { resumeState: _ignored, ...rest } = guard
    await this.replace(this.guardPath, JSON.stringify({ ...rest, state: 'active' }))
    current()
  }
  /** Confirm locally that no active or uncertain session was published before login. */
  async confirmEmpty(current: () => void): Promise<void> {
    current()
    if ((await this.load()).kind !== 'empty') throw new Error('Account state is not empty.')
    await fs.mkdir(this.dir, { recursive: true })
    let raw: string | null = null
    try { raw = await fs.readFile(this.guardPath, 'utf8') }
    catch (error) { if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error }
    if (raw !== null) {
      const guard = JSON.parse(raw) as { version?: unknown; state?: unknown }
      if (guard.version !== 1 || guard.state !== 'blocked') throw new Error('Account guard is not blocked.')
      const file = await fs.open(this.guardPath, 'r')
      try { await file.sync() } finally { await file.close() }
    }
    await this.syncDir(); current()
    if ((await this.load()).kind !== 'empty') throw new Error('Account state changed.')
    if (raw !== null && await fs.readFile(this.guardPath, 'utf8') !== raw) throw new Error('Account guard changed.')
    current()
  }
  /** Explicit exit only: preserve unbound/corrupt records, including interrupted writes.
   * Move the guard last so interrupted recovery remains fenced until the next exit. */
  private async quarantineUnbound(): Promise<void> {
    const names = (await fs.readdir(this.dir)).filter(name => name === 'account.guard' || name === 'account.bin' || /^account-[a-f0-9]{32}\.bin$/.test(name) || /^account(?:\.guard|\.bin|-[a-f0-9]{32}\.bin)\.[a-f0-9]{32}\.tmp$/.test(name))
    const archive = await fs.mkdtemp(join(this.dir, 'account-quarantine-'))
    await fs.chmod(archive, 0o700)
    await this.syncDir()
    for (const name of names.filter(name => name !== 'account.guard')) await fs.rename(join(this.dir, name), join(archive, name))
    const syncArchive = async () => {
      const handle = await fs.open(archive, 'r')
      try { await handle.sync() } finally { await handle.close() }
      await this.syncDir()
    }
    await syncArchive()
    if (names.includes('account.guard')) await fs.rename(this.guardPath, join(archive, 'account.guard'))
    await syncArchive()
  }
  /** D185: block first, then remove every canonical local payload and the guard.
   * On an I/O failure the tombstone remains and cannot restore the old credential. */
  async clear(_state: 'blocked' | 'revocation_unknown' = 'blocked', explicitExit = false): Promise<void> {
    // A failed decrypt must never turn into a tombstone or erase a recoverable guard.
    if (this.verifyBeforeClear) {
      const saved = await this.load()
      if (saved.kind !== 'empty') {
        if (!saved.sealed) {
          if (!explicitExit) throw new Error('unbound_account_record')
          await this.quarantineUnbound(); return
        }
        this.verifyBeforeClear(saved.sealed)
      }
    }
    await this.block('blocked')
    const names = await fs.readdir(this.dir)
    for (const name of names) {
      if (name === 'account.bin' || /^account-[a-f0-9]{32}\.bin$/.test(name) || /^account(?:\.guard|\.bin|-[a-f0-9]{32}\.bin)\.[a-f0-9]{32}\.tmp$/.test(name)) await fs.rm(join(this.dir, name), { force: true })
    }
    await this.syncDir()
    await fs.rm(this.guardPath, { force: true }); await this.syncDir()
  }
}

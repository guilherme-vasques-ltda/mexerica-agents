import { randomBytes } from 'node:crypto'
import * as fs from 'node:fs'
import { join } from 'node:path'

// 1: legacy; 2: same secret, Donnabot item retained; 3: Donnabot item absent or removed (unsupported).
export const STORAGE_GENERATION = 1
const CANARY = 'mexerica/keychain-canary/v1'
export { KeychainBlocked, keychainFailure } from './failure.ts'
import { KeychainBlocked, type KeychainFailure } from './failure.ts'

export interface KeychainCodec {
  isEncryptionAvailable(): boolean
  encryptString(value: string): Buffer
  decryptString(value: Buffer): string
}

function blocked(reason: KeychainFailure = 'unavailable'): never { throw new KeychainBlocked(reason) }
function read(path: string, limit: number): Buffer | null {
  let fd: number
  try { fd = fs.openSync(path, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW) }
  catch (e) { if ((e as NodeJS.ErrnoException).code === 'ENOENT') return null; blocked() }
  try {
    const st = fs.fstatSync(fd)
    if (!st.isFile() || st.size > limit || st.nlink !== 1 || (process.getuid && (st.uid !== process.getuid() || (st.mode & 0o077) !== 0))) blocked()
    return fs.readFileSync(fd)
  } finally { fs.closeSync(fd) }
}

/** Synchronous: environment must refuse newer/unknown data before Electron fixes its crypto name. */
export function readStorageGeneration(dir: string, platform: NodeJS.Platform): 0 | 1 | 2 {
  if (platform !== 'darwin') return 0
  const raw = read(join(dir, 'storage-generation'), 16_384)
  if (raw === null) return 0
  try {
    const marker: unknown = JSON.parse(raw.toString('utf8'))
    if (!marker || typeof marker !== 'object' || Array.isArray(marker)) blocked('unsupported')
    // Phase/counters/suspension and future fields never change the committed generation.
    // pending-2 is { generation: 1, phase: 'pending', targetGeneration: 2 }.
    const generation = (marker as { generation?: unknown }).generation
    if (generation === 1 || generation === 2) return generation
  } catch { blocked('unsupported') }
  blocked('unsupported')
}

function syncDirectory(dir: string) {
  const fd = fs.openSync(dir, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW)
  try { fs.fsyncSync(fd) } finally { fs.closeSync(fd) }
}
/** Called only under the app single-instance lock, for an absent destination. */
function publish(dir: string, name: string, bytes: Buffer) {
  const path = join(dir, name), tmp = `${path}.${randomBytes(16).toString('hex')}.tmp`
  const fd = fs.openSync(tmp, 'wx', 0o600)
  try {
    try { fs.writeFileSync(fd, bytes); fs.fsyncSync(fd) } finally { fs.closeSync(fd) }
    // Atomic no-replace publication; remove our extra link before readers require nlink=1.
    fs.linkSync(tmp, path)
    fs.unlinkSync(tmp)
    syncDirectory(dir)
  } finally {
    // Only our unpublished temporary; never remove a canary, marker or user payload.
    try { fs.unlinkSync(tmp) } catch (e) { if ((e as NodeJS.ErrnoException).code !== 'ENOENT') throw e }
  }
}

/** After ready + single-instance lock, before any secret consumer. Mac only.
 * Canary is durable before the marker: a crash between them is resumable.
 * Generation 1 with a missing/invalid canary is never reinitialized. */
export function prepareKeychain(dir: string, platform: NodeJS.Platform, codec: KeychainCodec): void {
  if (platform !== 'darwin') return
  try {
    const generation = readStorageGeneration(dir, platform)
    if (!codec.isEncryptionAvailable()) blocked()
    let canaryOrigin: 'empty' | 'existing-data' = 'existing-data'
    let canary = read(join(dir, 'keychain-canary.bin'), 4096)
    if (canary === null) {
      if (generation !== 0) blocked('mismatch')
      fs.mkdirSync(dir, { recursive: true, mode: 0o700 })
      const st = fs.lstatSync(dir)
      if (!st.isDirectory() || st.isSymbolicLink() || (process.getuid && st.uid !== process.getuid())) blocked()
      canaryOrigin = fs.readdirSync(dir).length === 0 ? 'empty' : 'existing-data'
      canary = codec.encryptString(`${CANARY}:${canaryOrigin}`)
      try { if (codec.decryptString(canary) !== `${CANARY}:${canaryOrigin}`) blocked('mismatch') }
      catch { blocked('mismatch') }
      publish(dir, 'keychain-canary.bin', canary)
    }
    let plain: string
    try { plain = codec.decryptString(canary) } catch { blocked('mismatch') }
    if (plain === `${CANARY}:empty`) canaryOrigin = 'empty'
    else if (plain === CANARY || plain === `${CANARY}:existing-data`) canaryOrigin = 'existing-data'
    else blocked('mismatch')
    // Re-sync on recovery after a crash/failing directory fsync.
    const fd = fs.openSync(join(dir, 'keychain-canary.bin'), fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW)
    try { fs.fsyncSync(fd) } finally { fs.closeSync(fd) }
    syncDirectory(dir)
    if (generation === 0) publish(dir, 'storage-generation', Buffer.from(JSON.stringify({ generation: STORAGE_GENERATION, phase: 'ready', canaryOrigin }) + '\n'))
    const marker = fs.openSync(join(dir, 'storage-generation'), fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW)
    try { fs.fsyncSync(marker) } finally { fs.closeSync(marker) }
    syncDirectory(dir)
  } catch (error) { if (error instanceof KeychainBlocked) throw error; blocked() }
}

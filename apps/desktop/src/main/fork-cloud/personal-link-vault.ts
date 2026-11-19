/** Cofre do T D430 no Main: safeStorage da plataforma. Ausente/bloqueado/basic_text/corrompido
 * FECHA; não há arquivo plaintext, env, flag ou debug alternativo. Registros nunca são substituídos. */
import { createHash } from 'node:crypto'
import { constants, lstatSync, promises as fs } from 'node:fs'
import { join } from 'node:path'
import { app, safeStorage } from 'electron'
import { newDeviceSeed, signerFromSeed, unb64url, b64url, type DeviceSigner } from './crypto'

/** Finais nunca substituídos; `prefixo.<32hex>` guarda o material ANTES do envio (resposta perdida). */
export type PersonalLinkRecord = 'device' | 'admission' | 'complete' | 'management' | 'receipt' | `${'accept' | 'persisted' | 'management'}.${string}`
const NAME = /^(device|admission|complete|management|receipt|(accept|persisted|management)\.[a-f0-9]{32})$/
/** Backends Linux do safeStorage (Electron fixado) admitidos; `basic_text`, `unknown` ou outro nome FECHAM. */
const LINUX = new Set(['gnome_libsecret', 'kwallet', 'kwallet5', 'kwallet6'])
const available = () => {
  if (!safeStorage.isEncryptionAvailable()) return false
  if (process.platform === 'linux') return LINUX.has(safeStorage.getSelectedStorageBackend())
  return process.platform === 'darwin' || process.platform === 'win32'
}
function closed(): never { throw new Error('personalLinkVaultUnavailable') }

async function privateDir(dir: string, create: 'ifMissing' | 'exclusive'): Promise<void> {
  try { await fs.mkdir(dir, { mode: 0o700 }) } catch (e) { if (create === 'exclusive' || (e as NodeJS.ErrnoException).code !== 'EEXIST') throw e }
  const st = await fs.lstat(dir)
  if (!st.isDirectory() || st.uid !== process.getuid?.() || (st.mode & 0o777) !== 0o700) closed()
}

/** D456: cada nova admissão local depois de uma reabertura usa uma GERAÇÃO nova (`gN`, criada exclusivamente);
 * as anteriores nunca são sobrescritas nem apagadas. */
export class PersonalLinkVault {
  private readonly base: string
  private readonly generation: number
  private readonly dir: string
  private constructor(base: string, generation: number) { this.base = base; this.generation = generation; this.dir = join(base, `g${generation}`) }
  private stamp: readonly { path: string; dev: number; ino: number }[] | null = null
  /** B: entrada realmente não criadora. Ausência/geração incompleta nunca seleciona anterior. */
  static async reopen(context: string, anchorId: string): Promise<PersonalLinkVault> {
    if (!available()) closed()
    const base = join(app.getPath('userData'), `personal-link-${createHash('sha256').update(JSON.stringify(['d430', context, anchorId])).digest('hex').slice(0, 24)}`)
    const checked = async (path: string) => {
      const st = await fs.lstat(path)
      if (!st.isDirectory() || st.isSymbolicLink() || st.uid !== process.getuid?.() || (st.mode & 0o777) !== 0o700) closed()
      return { path, dev: st.dev, ino: st.ino }
    }
    const parent = await checked(base)
    const names = await fs.readdir(base)
    if (names.some(n => n.startsWith('g') && !/^g[1-9][0-9]{0,5}$/.test(n))) closed()
    const generations = names.filter(n => /^g[1-9][0-9]{0,5}$/.test(n)).map(n => Number(n.slice(1)))
    if (!generations.length) closed()
    const generation = Math.max(...generations)
    const child = await checked(join(base, `g${generation}`))
    const vault = new PersonalLinkVault(base, generation)
    vault.stamp = [parent, child]
    vault.checkExisting()
    return vault
  }
  /** Identidade da geração original atravessa awaits; nenhuma pasta nova ou substituída é admitida. */
  private checkExisting() {
    if (!available()) closed()
    for (const captured of this.stamp ?? []) {
      const st = lstatSync(captured.path)
      if (!st.isDirectory() || st.isSymbolicLink() || st.dev !== captured.dev || st.ino !== captured.ino || st.uid !== process.getuid?.() || (st.mode & 0o777) !== 0o700) closed()
    }
  }
  ready() { this.checkExisting() }
  /** Escopo por contexto C autorizado e âncora do procedimento; nunca a custódia de conta/motores. */
  static async open(context: string, anchorId: string): Promise<PersonalLinkVault> {
    if (!available()) closed()
    const base = join(app.getPath('userData'), `personal-link-${createHash('sha256').update(JSON.stringify(['d430', context, anchorId])).digest('hex').slice(0, 24)}`)
    await privateDir(base, 'ifMissing')
    const generations = (await fs.readdir(base)).filter(n => /^g[1-9][0-9]{0,5}$/.test(n)).map(n => Number(n.slice(1)))
    const current = generations.length ? Math.max(...generations) : 1
    await privateDir(join(base, `g${current}`), 'ifMissing')
    return new PersonalLinkVault(base, current)
  }
  /** Só o consumidor D456, no gesto explícito do dono, abre a geração seguinte. */
  async next(): Promise<PersonalLinkVault> {
    if (this.stamp) closed()
    if (!available()) closed()
    await privateDir(join(this.base, `g${this.generation + 1}`), 'exclusive')
    return new PersonalLinkVault(this.base, this.generation + 1)
  }
  async read(name: PersonalLinkRecord): Promise<Buffer | null> {
    if (!available() || !NAME.test(name)) closed()
    this.checkExisting()
    let file: import('node:fs/promises').FileHandle
    try { file = await fs.open(join(this.dir, name), constants.O_RDONLY | constants.O_NOFOLLOW) } catch (e) {
      if ((e as NodeJS.ErrnoException).code === 'ENOENT') return null
      throw e
    }
    try {
      const st = await file.stat()
      if (!st.isFile() || st.uid !== process.getuid?.() || (st.mode & 0o777) !== 0o600 || st.nlink !== 1 || st.size > 256 * 1024) closed()
      const plain = safeStorage.decryptString(await file.readFile())
      const bytes = unb64url(plain)
      if (!bytes) closed()
      return Buffer.from(bytes)
    } finally { await file.close() }
  }
  /** Exclusivo + fsync de arquivo e pasta; depois REABRE e confere os mesmos bytes. */
  async seal(name: PersonalLinkRecord, bytes: Uint8Array): Promise<Buffer> {
    if (this.stamp) closed()
    if (!available() || !NAME.test(name)) closed()
    const file = await fs.open(join(this.dir, name), constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600)
    try { await file.writeFile(safeStorage.encryptString(b64url(bytes))); await file.sync() } finally { await file.close() }
    const dir = await fs.open(this.dir, 'r')
    try { await dir.sync() } finally { await dir.close() }
    const reopened = await this.read(name)
    if (!reopened || !reopened.equals(Buffer.from(bytes))) closed()
    return reopened
  }
  /** Existe material pendente selado antes de um envio cuja resposta não chegou? */
  async has(prefix: 'accept' | 'management'): Promise<boolean> {
    if (!available()) closed()
    return (await fs.readdir(this.dir)).some(n => n.startsWith(`${prefix}.`) && NAME.test(n))
  }
  /** T nasce aqui só no primeiro ato; depois sempre reabre. O signer devolvido recheca o cofre a cada uso. */
  async signer(create: boolean): Promise<DeviceSigner> {
    let seed = await this.read('device')
    if (!seed) {
      if (!create || this.stamp) closed()
      seed = await this.seal('device', newDeviceSeed())
    }
    if (seed.length !== 32) closed()
    const inner = signerFromSeed(seed)
    // SEG-A-R1-04: confere o cofre IMEDIATAMENTE antes de cada assinatura; perdido → recusa, sem fallback.
    const check = () => this.checkExisting()
    return Object.freeze({ publicKey: inner.publicKey, sign(message: Uint8Array) { check(); return inner.sign(message) } })
  }
}

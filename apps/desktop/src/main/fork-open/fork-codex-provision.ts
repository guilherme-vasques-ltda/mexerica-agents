import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { constants } from 'node:fs'
import { chmod, lstat, mkdir, mkdtemp, open, realpath, rm } from 'node:fs/promises'
import { basename, dirname, isAbsolute, join, resolve, sep } from 'node:path'
import { Readable } from 'node:stream'
import { pipeline } from 'node:stream/promises'
import { privateDirectory, validatePrivateFile } from '../../shared/fork-private-files.ts'

const version = '0.164.0'
const maxArchive = 128 * 1024 * 1024
const maxExecutable = 384 * 1024 * 1024
const readFlags = constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK
const createFlags = constants.O_CREAT | constants.O_EXCL | constants.O_WRONLY | constants.O_NOFOLLOW
const assets = {
  arm64: {
    member: 'codex-aarch64-apple-darwin',
    archiveHash: 'aa8299c169d80111aac05543ad2621c9bdbedb9bc252c905fe3f5a5baf9a4142',
    binaryHash: 'd8990e3ee7acfbc26f91265b12a1a83f0fee40d0e0f8f1f40a37d4ec21f499cf',
  },
  x64: {
    member: 'codex-x86_64-apple-darwin',
    archiveHash: 'de6bbc013c56337f6ddd3f26dfbfd62aad5924cb68cd5ef6430538922bfa0e75',
    binaryHash: 'd7e19691e0c1945612475ee8ab32e59ead55472e976b5cea1682d827d34dbba1',
  },
}

type Asset = (typeof assets)[keyof typeof assets]
type Candidate = { path: string; linkRoots?: string[] }
type ProvisionCode = 'open_codex_selection' | 'open_codex_storage' | 'open_codex_download' |
  'open_codex_integrity' | 'open_codex_declined' | 'open_codex_platform' | 'open_codex_timeout'

export class CodexPreparationError extends Error {
  readonly code: ProvisionCode
  constructor(code: ProvisionCode) { super(code); this.code = code }
}

export async function beforeDeviceLogin(method: unknown, body: unknown, signal: AbortSignal,
  prepare: (signal: AbortSignal) => Promise<void>) {
  if (method !== 'donnaEngineSetupStart' || !body || typeof body !== 'object') return
  const request = body as Record<string, unknown>
  if (request.action === 'login' && typeof request.engineAccountId === 'string' &&
    typeof request.clientRequestId === 'string' && Number.isSafeInteger(request.expectedRevision)) {
    await prepare(signal)
    signal.throwIfAborted()
  }
}

const missing = (error: unknown) => (error as NodeJS.ErrnoException).code === 'ENOENT'
const within = (path: string, root: string) => path.startsWith(root + sep)
const brew = (prefix: string): Candidate => ({
  path: join(prefix, 'bin/codex'),
  linkRoots: [join(prefix, 'Cellar/codex'), join(prefix, 'Caskroom/codex')],
})

async function checkDirectories(path: string, create = false) {
  if (!isAbsolute(path) || resolve(path) !== path) throw new Error()
  let current: string = sep
  const uid = process.getuid?.()
  for (const part of path.slice(1).split(sep)) {
    current = join(current, part)
    const stat = await lstat(current).catch(async (error: unknown) => {
      if (!create || !missing(error)) throw error
      await mkdir(current, { mode: 0o700 }).catch((issue: NodeJS.ErrnoException) => {
        if (issue.code !== 'EEXIST') throw issue
      })
      return lstat(current)
    })
    if (!stat.isDirectory() || (stat.uid !== 0 && stat.uid !== uid) ||
      ((stat.mode & 0o022) !== 0 && !(stat.uid === 0 && (stat.mode & 0o1000) !== 0))) throw new Error()
  }
}

async function verifiedBytes(source: Readable, limit: number, expected: string, signal: AbortSignal,
  destination?: { path: string; mode: number }) {
  const fd = destination ? await open(destination.path, createFlags, 0o600).catch((error: unknown) => {
    source.destroy()
    throw error
  }) : null
  try {
    await pipeline(source, async (chunks) => {
      let size = 0
      const hash = createHash('sha256')
      for await (const chunk of chunks as AsyncIterable<Buffer>) {
        size += chunk.length
        if (size > limit) throw new CodexPreparationError('open_codex_integrity')
        hash.update(chunk)
        await fd?.writeFile(chunk)
      }
      if (!size || hash.digest('hex') !== expected) throw new CodexPreparationError('open_codex_integrity')
    }, { signal })
    if (fd && destination) {
      await fd.chmod(destination.mode)
      await fd.sync()
    }
  } finally { await fd?.close() }
}

async function verifyExecutable(candidate: Candidate, digest: string, signal: AbortSignal, destination?: string) {
  let path = candidate.path
  await checkDirectories(dirname(path))
  if ((await lstat(path)).isSymbolicLink()) {
    if (!candidate.linkRoots) throw new Error()
    path = await realpath(path)
    if (basename(path) !== 'codex' && !/^codex-(?:aarch64|x86_64)-apple-darwin$/.test(basename(path))) throw new Error()
    if (!candidate.linkRoots.some((root) => within(path, root))) throw new Error()
    await checkDirectories(dirname(path))
  }
  const fd = await open(path, readFlags)
  try {
    const stat = await fd.stat()
    if (!stat.isFile() || stat.nlink !== 1 || (stat.uid !== 0 && stat.uid !== process.getuid?.()) ||
      (stat.mode & 0o7022) !== 0 || (stat.mode & 0o100) === 0 || stat.size < 1 || stat.size > maxExecutable) throw new Error()
    await verifiedBytes(fd.createReadStream(), stat.size, digest, signal,
      destination ? { path: destination, mode: 0o700 } : undefined)
  } finally { await fd.close() }
}

function managedPaths(home: string) {
  return [join(home, '.local/bin'), join(home, '.local/share'), '/opt/homebrew', '/usr/local/bin', '/Applications/Codex.app']
}

async function checkSelection(settings: string, home: string, signal: AbortSignal) {
  const fd = await open(settings, readFlags).catch((error: unknown) => {
    if (missing(error)) return null
    throw error
  })
  if (!fd) return false
  try {
    const stat = await fd.stat()
    validatePrivateFile(stat)
    if (stat.size < 1 || stat.size > 4096) throw new Error()
    const bytes = Buffer.alloc(4097)
    const { bytesRead } = await fd.read(bytes)
    if (bytesRead !== stat.size) throw new Error()
    const value = JSON.parse(bytes.subarray(0, bytesRead).toString('utf8'))
    if (!value || Object.keys(value).length !== 3 || value.version !== version ||
      typeof value.sha256 !== 'string' || !/^[0-9a-f]{64}$/.test(value.sha256) ||
      typeof value.executable !== 'string' || resolve(value.executable) !== value.executable ||
      !['codex', 'codex-aarch64-apple-darwin'].includes(basename(value.executable)) ||
      !managedPaths(home).some((root) => within(value.executable, root))) throw new Error()
    await verifyExecutable({ path: value.executable }, value.sha256, signal)
    return true
  } finally { await fd.close() }
}

export function releaseURL(url: URL) {
  return url.protocol === 'https:' && !url.username && !url.password && !url.port &&
    (url.hostname === 'github.com' || url.hostname === 'release-assets.githubusercontent.com')
}

async function downloadRelease(asset: Asset, signal: AbortSignal): Promise<Readable> {
  let url = new URL(`https://github.com/openai/codex/releases/download/rust-v${version}/${asset.member}.tar.gz`)
  for (let redirects = 0; redirects <= 3 && releaseURL(url); redirects++) {
    const response = await fetch(url, { redirect: 'manual', credentials: 'omit', signal })
    if (response.status === 200 && response.body) {
      return Readable.fromWeb(response.body as import('node:stream/web').ReadableStream)
    }
    await response.body?.cancel()
    const location = response.headers.get('location')
    if (response.status < 300 || response.status >= 400 || !location) break
    url = new URL(location, url)
  }
  throw new CodexPreparationError('open_codex_download')
}

async function extractMember(archive: string, asset: Asset, executable: string, signal: AbortSignal) {
  const child = spawn('/usr/bin/tar', ['-xOzf', archive, asset.member], {
    signal, env: { PATH: '/usr/bin:/bin', LC_ALL: 'C' }, stdio: ['ignore', 'pipe', 'ignore'],
  })
  const exit = new Promise<boolean>((done) => {
    child.once('error', () => done(false))
    child.once('close', (code) => done(code === 0))
  })
  try {
    await verifiedBytes(child.stdout, maxExecutable, asset.binaryHash, signal, { path: executable, mode: 0o700 })
    if (!await exit) throw new CodexPreparationError('open_codex_integrity')
  } finally {
    if (child.exitCode === null) child.kill('SIGKILL')
    await exit
  }
}

async function publishSelection(settings: string, executable: string, sha256: string) {
  const fd = await open(settings, createFlags, 0o600).catch((error: NodeJS.ErrnoException) => {
    if (error.code === 'EEXIST') throw new CodexPreparationError('open_codex_selection')
    throw error
  })
  let written = false
  try {
    await fd.chmod(0o600)
    validatePrivateFile(await fd.stat())
    await fd.writeFile(JSON.stringify({ executable, version, sha256 }) + '\n')
    await fd.sync()
    written = true
  } finally {
    await fd.close()
    if (!written) await rm(settings, { force: true })
  }
}

export function createCodexPreparer(profile: string, arch: string, confirm: (signal: AbortSignal) => Promise<boolean>,
  deps: { asset?: Asset; candidates?: Candidate[]; download?: typeof downloadRelease } = {}) {
  const home = dirname(profile)
  const settings = join(profile, 'codex-local.json')
  const candidates = deps.candidates ?? [
    { path: join(home, '.local/bin/codex') },
    brew(arch === 'arm64' ? '/opt/homebrew' : '/usr/local'),
    { path: '/Applications/Codex.app/Contents/Resources/codex' },
  ]

  async function prepare(lifetime: AbortSignal) {
    const asset = deps.asset ?? (arch === 'arm64' || arch === 'x64' ? assets[arch] : null)
    if (!asset) throw new CodexPreparationError('open_codex_platform')
    try { privateDirectory(profile, false) } catch { throw new CodexPreparationError('open_codex_storage') }
    try {
      if (await checkSelection(settings, home, lifetime)) return
    } catch {
      lifetime.throwIfAborted()
      throw new CodexPreparationError('open_codex_selection')
    }
    const consent = AbortSignal.timeout(180_000)
    lifetime.throwIfAborted()
    const accepted = await confirm(AbortSignal.any([lifetime, consent])).catch((error: unknown) => {
      if (consent.aborted) throw new CodexPreparationError('open_codex_timeout')
      throw error
    })
    lifetime.throwIfAborted()
    if (consent.aborted) throw new CodexPreparationError('open_codex_timeout')
    if (!accepted) throw new CodexPreparationError('open_codex_declined')
    const deadline = AbortSignal.timeout(180_000)
    const signal = AbortSignal.any([lifetime, deadline])
    const share = join(home, '.local/share')
    let install: string | undefined
    let installed = false
    try {
      await checkDirectories(share, true)
      install = await mkdtemp(join(share, 'mexerica-open-codex-'))
      await chmod(install, 0o700)
      const executable = join(install, 'codex')
      let found = false
      for (const candidate of candidates) {
        signal.throwIfAborted()
        try {
          await verifyExecutable(candidate, asset.binaryHash, signal, executable)
          found = true
          break
        } catch {
          await rm(executable, { force: true })
        }
      }
      if (!found) {
        const archive = join(install, 'release.tar.gz')
        const stream = await (deps.download ?? downloadRelease)(asset, signal)
          .catch(() => { throw new CodexPreparationError('open_codex_download') })
        try {
          await verifiedBytes(stream, maxArchive, asset.archiveHash, signal, { path: archive, mode: 0o600 })
        } catch (error) {
          if (error instanceof CodexPreparationError) throw error
          const code = (error as NodeJS.ErrnoException).code
          if (code === 'ENOSPC' || code === 'EACCES' || code === 'EPERM' || code === 'EROFS') throw error
          throw new CodexPreparationError('open_codex_download')
        }
        await extractMember(archive, asset, executable, signal)
        await rm(archive)
      }
      signal.throwIfAborted()
      privateDirectory(profile, false)
      await publishSelection(settings, executable, asset.binaryHash)
      installed = true
    } catch (error) {
      lifetime.throwIfAborted()
      if (deadline.aborted) throw new CodexPreparationError('open_codex_timeout')
      if (error instanceof CodexPreparationError) throw error
      throw new CodexPreparationError('open_codex_storage')
    } finally {
      if (install && !installed) await rm(install, { recursive: true, force: true }).catch(() => {})
    }
  }

  let inFlight: Promise<void> | undefined
  return (signal: AbortSignal) => inFlight ??= prepare(signal).finally(() => { inFlight = undefined })
}

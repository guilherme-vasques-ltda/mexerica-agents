import fs from 'node:fs'
import { randomUUID } from 'node:crypto'
import { dirname, isAbsolute, parse, resolve, sep } from 'node:path'
import { SSH_STORAGE_STATUS, type SSHProfile } from '../shared/ssh.ts'

const invalid = () => new Error('Unverified Desktop profile storage')
const missing = (error: unknown) => (error as NodeJS.ErrnoException).code === 'ENOENT'
const same = (a: fs.Stats, b: fs.Stats) => a.dev === b.dev && a.ino === b.ino && a.uid === b.uid
const mode = (meta: fs.Stats) => meta.mode & 0o7777
const { O_RDONLY, O_DIRECTORY, O_NOFOLLOW, O_NONBLOCK, O_WRONLY, O_CREAT, O_EXCL } = fs.constants

function owner() {
  if (!process.geteuid || !O_NOFOLLOW || !O_DIRECTORY || !O_NONBLOCK) throw invalid()
  return process.geteuid()
}

function statIfPresent(path: string) {
  try { return fs.lstatSync(path) }
  catch (error) { if (!missing(error)) throw error; return null }
}

// Node lacks descriptor-relative traversal and rename. These checks rely on trusted
// ancestors and do not isolate other processes running as the same UID.
function openRoot(path: string, createLeaf = false) {
  const uid = owner()
  const { root } = parse(path)
  if (!isAbsolute(path) || resolve(path) !== path || path === root) throw invalid()
  let current = root
  const parts = path.slice(root.length).split(sep)
  for (const [index, part] of parts.entries()) {
    current = resolve(current, part)
    const leaf = index === parts.length - 1
    let before = statIfPresent(current)
    if (!before && leaf && createLeaf) {
      fs.mkdirSync(current, { mode: 0o700 })
      before = fs.lstatSync(current)
    }
    if (!before) throw invalid()
    if (!before.isDirectory() || (before.uid !== uid && (leaf || before.uid !== 0))) throw invalid()
    if ((mode(before) & 0o022) && !(!leaf && before.uid === 0 && (mode(before) & 0o1000))) throw invalid()
    const fd = fs.openSync(current, O_RDONLY | O_DIRECTORY | O_NOFOLLOW)
    let keep = false
    try {
      const opened = fs.fstatSync(fd)
      if (!opened.isDirectory() || !same(before, opened) || mode(before) !== mode(opened)) throw invalid()
      if (!leaf) continue
      checkRoot(path, fd)
      keep = true
      return fd
    } finally { if (!keep) fs.closeSync(fd) }
  }
  throw invalid()
}

function checkRoot(path: string, fd: number) {
  const meta = fs.fstatSync(fd)
  const visible = fs.lstatSync(path)
  if (!meta.isDirectory() || !visible.isDirectory() || meta.uid !== owner() ||
      (mode(meta) & 0o022) || (mode(visible) & 0o022) || !same(meta, visible)) throw invalid()
}

function regular(meta: fs.Stats) {
  if (!meta.isFile() || meta.uid !== owner() || meta.nlink !== 1) throw invalid()
}

function parseProfiles(bytes: Buffer): SSHProfile[] {
  const value: unknown = JSON.parse(new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes))
  if (!Array.isArray(value)) throw invalid()
  const ids = new Set<string>()
  return value.map((entry: unknown) => {
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) throw invalid()
    const { id, host, port, user, identityFile, remotePort, computerId, name } = entry as Partial<SSHProfile>
    if (typeof id !== 'string' || id === SSH_STORAGE_STATUS || ids.has(id) || typeof host !== 'string' ||
        !(name === undefined || typeof name === 'string') ||
        !(port === null || Number.isInteger(port)) || !Number.isInteger(remotePort) ||
        !(user === null || typeof user === 'string') || !(identityFile === null || typeof identityFile === 'string') ||
        !(computerId === undefined || computerId === null || typeof computerId === 'string')) throw invalid()
    ids.add(id)
    return { ...entry, name: name ?? '', computerId: computerId ?? null } as SSHProfile
  })
}

function read(path: string, root: number, tighten: boolean) {
  const parent = dirname(path)
  checkRoot(parent, root)
  const before = statIfPresent(path)
  if (!before) { checkRoot(parent, root); return null }
  regular(before)
  const fd = fs.openSync(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK)
  try {
    let meta = fs.fstatSync(fd)
    regular(meta)
    if (!same(before, meta)) throw invalid()
    if (tighten && mode(meta) !== 0o600) fs.fchmodSync(fd, 0o600)
    meta = fs.fstatSync(fd)
    regular(meta)
    if (mode(meta) !== 0o600) throw invalid()
    checkRoot(parent, root)
    const bytes = fs.readFileSync(fd)
    const after = fs.fstatSync(fd)
    const visible = fs.lstatSync(path)
    regular(after)
    regular(visible)
    if (!same(meta, visible) || mode(after) !== 0o600 || mode(visible) !== 0o600 ||
        after.size !== bytes.length || after.size !== meta.size || after.mtimeMs !== meta.mtimeMs ||
        visible.ctimeMs !== after.ctimeMs || after.ctimeMs !== meta.ctimeMs) throw invalid()
    checkRoot(parent, root)
    return { meta: after, bytes, profiles: parseProfiles(bytes) }
  } finally { fs.closeSync(fd) }
}

type Snapshot = ReturnType<typeof read>
type Publication = { kind: 'saved' } | { kind: 'refused' } | { kind: 'uncertain'; profiles: SSHProfile[] | undefined }

function unchanged(expected: Snapshot, current: Snapshot) {
  return expected === null ? current === null :
    current !== null && same(expected.meta, current.meta) && expected.bytes.equals(current.bytes)
}

export class DesktopSSHProfiles {
  private expected: Snapshot | undefined
  private loadedRoot: fs.Stats | undefined
  private readonly path: string

  constructor(path: string) { this.path = path }

  load(createRoot = false): SSHProfile[] | null {
    this.expected = undefined
    const root = openRoot(dirname(this.path), createRoot)
    try {
      this.loadedRoot = fs.fstatSync(root)
      this.expected = read(this.path, root, true)
      return this.expected?.profiles ?? null
    } finally { fs.closeSync(root) }
  }

  save(profiles: SSHProfile[]): Publication {
    if (this.expected === undefined || !this.loadedRoot) return { kind: 'refused' }
    const parent = dirname(this.path)
    const tmp = `${this.path}.${randomUUID()}.tmp`
    let root: number | undefined
    let temp: fs.Stats | undefined
    let published = false
    try {
      root = openRoot(parent)
      if (!same(this.loadedRoot, fs.fstatSync(root))) throw invalid()
      const expected = this.expected
      if (!unchanged(expected, read(this.path, root, false))) throw invalid()
      const bytes = Buffer.from(JSON.stringify(profiles))
      const parsed = parseProfiles(bytes)
      checkRoot(parent, root)
      const fd = fs.openSync(tmp, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o600)
      try {
        temp = fs.fstatSync(fd)
        regular(temp)
        if (mode(temp) !== 0o600) fs.fchmodSync(fd, 0o600)
        fs.writeFileSync(fd, bytes)
        fs.fsyncSync(fd)
        temp = fs.fstatSync(fd)
        regular(temp)
        if (mode(temp) !== 0o600) throw invalid()
      } finally { fs.closeSync(fd) }
      const current = read(this.path, root, false)
      if (!unchanged(expected, current)) throw invalid()
      checkRoot(parent, root)
      const visible = fs.lstatSync(tmp)
      regular(visible)
      if (!same(temp, visible) || mode(visible) !== 0o600 || temp.ctimeMs !== visible.ctimeMs) throw invalid()
      if (current) fs.renameSync(tmp, this.path)
      else fs.linkSync(tmp, this.path)
      published = true
      if (current) temp = undefined
      else { fs.unlinkSync(tmp); temp = undefined }
      checkRoot(parent, root)
      fs.fsyncSync(root)
      this.expected = { meta: visible, bytes, profiles: parsed }
      return { kind: 'saved' }
    } catch {
      if (!published) return { kind: 'refused' }
      this.expected = undefined
      if (root !== undefined) {
        try { this.expected = read(this.path, root, false) ?? undefined } catch {}
      }
      return { kind: 'uncertain', profiles: this.expected?.profiles }
    } finally {
      if (root !== undefined) {
        try {
          if (temp && !published) {
            checkRoot(parent, root)
            const visible = statIfPresent(tmp)
            if (visible?.isFile() && same(temp, visible) && visible.nlink === 1) fs.unlinkSync(tmp)
          }
        } catch {}
        fs.closeSync(root)
      }
    }
  }
}

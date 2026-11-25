import { createHash } from 'node:crypto'
import { closeSync, fstatSync, lstatSync, openSync, readFileSync, realpathSync } from 'node:fs'
import { isAbsolute, join, normalize, parse, sep } from 'node:path'
import type { Stats } from 'node:fs'
import { rcPaths } from '../shared/fork-rc-profile.ts'
import { rc2Paths, type Rc2Component, type Rc2ComponentRole } from '../shared/fork-rc2-profile.ts'
import { verifyRc2AdHocSignature } from './fork-rc2-cdhash.ts'

const invalid = () => new Error('invalid_rc2_artifact')

function machoArm64(bytes: Buffer): boolean {
  return bytes.length >= 32 && bytes.readUInt32LE(0) === 0xfeedfacf && bytes.readUInt32LE(4) === 0x0100000c
}

const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex')

export interface Rc2VerifiedComponent {
  role: Rc2ComponentRole
  path: string
  // Descritor fixado na verificação: checagem e lançamento futuro usam este objeto, nunca o caminho.
  fd: number
}

export interface Rc2VerifiedArtifact {
  components: Rc2VerifiedComponent[]
  close(): void
}

type VerifySignature = (bytes: Buffer, component: Rc2Component) => void

function verifyComponent(root: string, component: Rc2Component, verifySignature: VerifySignature): number {
  const segments = component.path.split('/')
  let resolvedRoot: string
  let path = root
  let inspected: Stats | undefined
  try {
    resolvedRoot = realpathSync(root)
    for (const [index, segment] of segments.entries()) {
      path = join(path, segment)
      const stat: Stats = lstatSync(path)
      if (stat.isSymbolicLink()) throw invalid()
      if (index < segments.length - 1 && !stat.isDirectory()) throw invalid()
      if (index === segments.length - 1) {
        if (!stat.isFile()) throw invalid()
        inspected = stat
      }
    }
    if (!inspected || realpathSync(path) !== join(resolvedRoot, ...segments)) throw invalid()
    // Fixa o objeto: o descritor aberto tem de ser o mesmo inode inspecionado no caminho.
    const fd = openSync(path, 'r')
    try {
      const pinned = fstatSync(fd)
      if (!pinned.isFile() || pinned.dev !== inspected.dev || pinned.ino !== inspected.ino ||
          pinned.size !== inspected.size) throw invalid()
      const bytes = readFileSync(fd)
      if (!machoArm64(bytes) || sha256(bytes) !== component.sha256) throw invalid()
      verifySignature(bytes, component)
    } catch (error) {
      // Nenhuma recusa deixa descritor aberto: o objeto fixado morre com a verificação que falhou.
      try { closeSync(fd) } catch { /* já fechado */ }
      throw error
    }
    return fd
  } catch { throw invalid() }
}

export function verifyRc2Artifact(root: string, components: Rc2Component[],
  verifySignature: VerifySignature = (bytes, { cdhash }) => verifyRc2AdHocSignature(bytes, cdhash)): Rc2VerifiedArtifact {
  if (!Array.isArray(components) || !components.some(component => component.role === 'hub')) throw invalid()
  const verified: Rc2VerifiedComponent[] = []
  const close = () => { for (const component of verified) { try { closeSync(component.fd) } catch { /* já fechado */ } } }
  try {
    for (const component of components) {
      verified.push({ role: component.role, path: component.path, fd: verifyComponent(root, component, verifySignature) })
    }
  } catch (error) {
    close()
    throw error
  }
  return { components: verified, close }
}

export const rc2PathOverlaps = (path: string, root: string): boolean => {
  const caseless = (value: string) => value.normalize('NFD').toLowerCase()
  const target = caseless(path)
  const base = caseless(root)
  return target === base || target.startsWith(`${base}${sep}`) || base.startsWith(`${target}${sep}`)
}

export function verifyRc2StorageRoots(home: string, appData: string): void {
  if (!isAbsolute(home) || !isAbsolute(appData)) throw invalid()
  const resolveExisting = (target: string): string => {
    const absolute = normalize(target)
    let existing = parse(absolute).root
    const segments = absolute.slice(existing.length).split(sep).filter(Boolean)
    let depth = 0
    for (const segment of segments) {
      const next = join(existing, segment)
      let stat: Stats
      try { stat = lstatSync(next) } catch (error) {
        if ((error as NodeJS.ErrnoException).code === 'ENOENT') break
        throw invalid()
      }
      if (!stat.isDirectory() || stat.isSymbolicLink()) throw invalid()
      existing = next
      depth++
    }
    try { return join(realpathSync(existing), ...segments.slice(depth)) } catch { throw invalid() }
  }
  const rc1 = Object.values(rcPaths(home, appData)).map(path => resolveExisting(`${path}`))
  for (const target of Object.values(rc2Paths(home, appData))) {
    const resolved = resolveExisting(target)
    if (rc1.some(other => rc2PathOverlaps(resolved, other))) throw invalid()
  }
}

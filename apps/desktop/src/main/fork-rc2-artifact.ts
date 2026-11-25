import { createHash } from 'node:crypto'
import { lstatSync, readFileSync, realpathSync } from 'node:fs'
import { isAbsolute, join, normalize, parse, sep } from 'node:path'
import type { Stats } from 'node:fs'
import { rcPaths } from '../shared/fork-rc-profile.ts'
import { rc2Paths, type Rc2Component } from '../shared/fork-rc2-profile.ts'

const invalid = () => new Error('invalid_rc2_artifact')

function machoArm64(bytes: Buffer): boolean {
  return bytes.length >= 8 && bytes.readUInt32LE(0) === 0xfeedfacf && bytes.readUInt32LE(4) === 0x0100000c
}

const sha256 = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex')

// Cada componente do caminho é conferido por `lstat`: nenhum pode ser link, os
// intermediários têm de ser diretórios e o último, arquivo regular. O caminho
// resolvido ainda tem de continuar dentro da raiz resolvida.
export function verifyRc2Artifact(root: string, components: Rc2Component[]): string[] {
  if (!Array.isArray(components) || !components.some(component => component.role === 'hub')) throw invalid()
  const verified: string[] = []
  for (const component of components) {
    const segments = component.path.split('/')
    let resolvedRoot: string
    let path = root
    try {
      resolvedRoot = realpathSync(root)
      for (const [index, segment] of segments.entries()) {
        path = join(path, segment)
        const stat: Stats = lstatSync(path)
        if (stat.isSymbolicLink()) throw invalid()
        if (index < segments.length - 1 && !stat.isDirectory()) throw invalid()
        if (index === segments.length - 1 && !stat.isFile()) throw invalid()
      }
      if (realpathSync(path) !== join(resolvedRoot, ...segments)) throw invalid()
      const bytes = readFileSync(path)
      if (!machoArm64(bytes) || sha256(bytes) !== component.sha256) throw invalid()
    } catch { throw invalid() }
    verified.push(component.path)
  }
  return verified
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

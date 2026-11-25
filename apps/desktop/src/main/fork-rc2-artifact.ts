import { createHash } from 'node:crypto'
import { lstatSync, readFileSync, realpathSync } from 'node:fs'
import { join, sep } from 'node:path'
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

// Volumes insensíveis a caixa tratam nomes diferentes como o mesmo caminho:
// a comparação de sobreposição usa a forma canônica com caixa dobrada.
export const rc2PathOverlaps = (path: string, root: string): boolean => {
  const caseless = (value: string) => value.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase()
  const target = caseless(path)
  const base = caseless(root)
  return target === base || target.startsWith(`${base}/`)
}

// As raízes da rc.2 são conferidas no que já existe no disco: cada componente
// existente entre a âncora (home ou appData) e a raiz é resolvido dos dois
// lados, nenhum pode ser link e nenhum caminho resolvido pode cair dentro de
// raiz da rc.1 resolvida. Erro de acesso que não seja ausência recusa; o que
// ainda não existe fica para a criação de armazenamento, atrás desta guarda.
export function verifyRc2StorageRoots(home: string, appData: string): void {
  const anchors = [home, appData]
  const resolveExisting = (target: string): string => {
    const anchor = anchors.find(candidate => target === candidate || target.startsWith(`${candidate}${sep}`))
    if (!anchor) throw invalid()
    const segments = target === anchor ? [] : target.slice(anchor.length + sep.length).split(sep).filter(Boolean)
    let existing = anchor
    let depth = 0
    for (const segment of segments) {
      let stat: Stats
      try { stat = lstatSync(join(existing, segment)) } catch (error) {
        const code = (error as { code?: string }).code
        if (code === 'ENOENT' && depth === 0) return target
        if (code === 'ENOENT' || code === 'ENOTDIR') break
        throw invalid()
      }
      if (stat.isSymbolicLink()) throw invalid()
      existing = join(existing, segment)
      depth++
    }
    return join(realpathSync(existing), ...segments.slice(depth))
  }
  const rc1 = Object.values(rcPaths(home, appData)).map(path => resolveExisting(`${path}`))
  for (const target of Object.values(rc2Paths(home, appData))) {
    const resolved = resolveExisting(target)
    if (rc1.some(other => rc2PathOverlaps(resolved, other))) throw invalid()
  }
}

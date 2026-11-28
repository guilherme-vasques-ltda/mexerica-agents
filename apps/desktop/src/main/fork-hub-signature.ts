import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { lstatSync, readFileSync, realpathSync, type Stats } from 'node:fs'
import { basename, dirname, join } from 'node:path'

const failure = () => new Error('hub_bundle_signature_invalid')

export function verifyBundledHub(
  resources: string,
  name: 'donnabot-host' | 'mexerica-open-hub',
) {
  if (process.platform !== 'darwin' || basename(resources) !== 'Resources') throw failure()
  const contents = dirname(resources)
  const bundle = dirname(contents)
  if (basename(contents) !== 'Contents' || !basename(bundle).endsWith('.app')) throw failure()
  const binary = join(resources, name)
  const inspect = () => {
    const parents = [bundle, contents, resources].map(path => {
      const stat = lstatSync(path)
      if (!stat.isDirectory() || (stat.mode & 0o022) !== 0) throw failure()
      return stat
    })
    const stat = lstatSync(binary)
    if (!stat.isFile() || stat.nlink !== 1 || (stat.mode & 0o7022) !== 0 ||
        (stat.mode & 0o100) === 0 || realpathSync(binary) !== binary) throw failure()
    return [...parents, stat]
  }
  const unchanged = (a: Stats, b: Stats) =>
    a.dev === b.dev && a.ino === b.ino && a.size === b.size && a.mtimeMs === b.mtimeMs && a.ctimeMs === b.ctimeMs
  const codesign = (args: string[]) => {
    const result = spawnSync('/usr/bin/codesign', args, {
      encoding: 'utf8', env: {}, timeout: 8000, maxBuffer: 256 * 1024, stdio: ['ignore', 'pipe', 'pipe'],
    })
    if (result.status !== 0 || result.error) throw failure()
    return result.stderr
  }
  try {
    const before = inspect()
    const cdhash = () => {
      const hashes = codesign(['--display', '--verbose=4', binary]).match(/^CDHash=[0-9a-f]{40}$/gm)
      if (hashes?.length !== 1) throw failure()
      return hashes[0]
    }
    const expected = cdhash()
    // O codesign usa Security.framework. O selo do app vincula os recursos assinados ao bundle.
    codesign(['--verify', '--deep', '--strict', bundle])
    codesign(['--verify', '--strict', binary])
    if (cdhash() !== expected) throw failure()
    const sha256 = createHash('sha256').update(readFileSync(binary)).digest('hex')
    const after = inspect()
    if (!before.every((stat, index) => unchanged(stat, after[index]!))) throw failure()
    return { binary, sha256 }
  } catch {
    throw failure()
  }
}

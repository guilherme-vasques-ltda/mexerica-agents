import { join } from 'node:path'
import { rcManifestHash, rcPaths } from './fork-rc-profile.ts'

const distribution = Object.freeze({
  profile: 'rc2-local',
  version: '0.0.1-rc.2',
  appId: 'app.mexerica.mac.rc2',
  name: 'Mexerica RC 2', // textos-ok: identidade fixa no manifesto e no diretório do perfil.
  scheme: 'mexerica-rc2',
  packageName: 'mexerica-rc2',
} as const)

export const rc2ManifestPath = 'rc2-manifest.json'

export type Rc2ComponentRole = 'hub' | 'helper'

export interface Rc2Component {
  role: Rc2ComponentRole
  path: string
  arch: 'arm64'
  sha256: string
  cdhash: string
}

interface Target { platform: string; arch: string }

const invalid = () => new Error('invalid_rc2_profile')
const componentPath = /^[A-Za-z0-9][A-Za-z0-9._-]*(?:\/[A-Za-z0-9][A-Za-z0-9._-]*)*$/
const sha256Hex = /^[0-9a-f]{64}$/
const cdhashHex = /^[0-9a-f]{40}$/
const componentKeys = ['role', 'path', 'arch', 'sha256', 'cdhash']

export function rc2Components(input: unknown): Rc2Component[] {
  if (!Array.isArray(input) || input.length === 0) throw invalid()
  const components = input.map((entry): Rc2Component => {
    if (!entry || typeof entry !== 'object' || Array.isArray(entry)) throw invalid()
    const candidate = entry as Record<string, unknown>
    if (Object.keys(candidate).length !== componentKeys.length || componentKeys.some(key => !Object.hasOwn(candidate, key))) throw invalid()
    const { role, path, sha256, cdhash } = candidate
    if (typeof role !== 'string' || typeof path !== 'string' || typeof sha256 !== 'string' || typeof cdhash !== 'string') throw invalid()
    if (role !== 'hub' && role !== 'helper') throw invalid()
    if (candidate.arch !== 'arm64' ||
        !componentPath.test(path) || path.includes('..') || !sha256Hex.test(sha256) || !cdhashHex.test(cdhash)) throw invalid()
    return { role, path, arch: 'arm64' as const, sha256, cdhash }
  })
  if (components.filter(component => component.role === 'hub').length !== 1) throw invalid()
  if (new Set(components.map(component => component.role)).size !== components.length) throw invalid()
  if (new Set(components.map(component => component.path)).size !== components.length) throw invalid()
  return components
}

function checkTarget(version: unknown, target: Target) {
  if (version !== distribution.version || target.platform !== 'darwin' || target.arch !== 'arm64') {
    throw new Error('invalid_rc2_profile')
  }
}

export interface Rc2Artifact {
  profile: string
  version: string
  appId: string
  name: string
  scheme: string
  packageName: string
  platform: 'darwin'
  arch: 'arm64'
  components: Rc2Component[]
}

const manifestBytes = (components: Rc2Component[]) =>
  Buffer.from(`${JSON.stringify({ ...distribution, platform: 'darwin', arch: 'arm64', components })}\n`)

export function compileRc2Profile(mode: string, version: unknown, target: Target, components: unknown) {
  if (mode !== 'official-rc2') return null
  checkTarget(version, target)
  const inventory = rc2Components(components)
  const bytes = manifestBytes(inventory)
  return { distribution, bytes, pin: rcManifestHash(bytes) }
}

interface Rc2Bootstrap extends Target {
  packaged: boolean
  name: string
  version: string
  compiledProfile: string | undefined
  compiledPin: string | undefined
  readMetadata(): unknown
  readManifest(): Buffer
}

export function validateRc2Bootstrap(input: Rc2Bootstrap): Rc2Artifact {
  checkTarget(input.version, input)
  if (!input.packaged || input.name !== distribution.name || input.compiledProfile !== distribution.profile ||
      typeof input.compiledPin !== 'string' || !/^[0-9a-f]{64}$/.test(input.compiledPin)) {
    throw new Error('invalid_rc2_profile')
  }
  const metadata = input.readMetadata()
  if (!metadata || typeof metadata !== 'object' || Array.isArray(metadata)) throw new Error('invalid_rc2_profile')
  const expected = { mexericaProfile: distribution.profile, mexericaAppId: distribution.appId, name: distribution.packageName,
    productName: distribution.name, version: distribution.version }
  if (Object.entries(expected).some(([key, value]) => !Object.hasOwn(metadata, key) || (metadata as Record<string, unknown>)[key] !== value)) {
    throw new Error('invalid_rc2_profile')
  }
  const bytes = input.readManifest()
  if (rcManifestHash(bytes) !== input.compiledPin) throw new Error('invalid_rc2_profile')
  let manifest: Record<string, unknown>
  try { manifest = JSON.parse(bytes.toString('utf8')) } catch { throw new Error('invalid_rc2_profile') }
  if (!manifest || typeof manifest !== 'object' || Array.isArray(manifest)) throw new Error('invalid_rc2_profile')
  const keys = [...Object.keys(distribution), 'platform', 'arch', 'components']
  if (Object.keys(manifest).length !== keys.length || keys.some(key => !Object.hasOwn(manifest, key)) ||
      Object.entries(distribution).some(([key, value]) => manifest[key] !== value) ||
      manifest.platform !== 'darwin' || manifest.arch !== 'arm64') throw new Error('invalid_rc2_profile')
  return { ...distribution, platform: 'darwin', arch: 'arm64', components: rc2Components(manifest.components) }
}

export function rc2Paths(home: string, appData: string) {
  const userData = join(appData, distribution.name)
  return {
    userData,
    sessionData: join(userData, 'session'),
    cache: join(userData, 'cache'),
    logs: join(userData, 'logs'),
    crashDumps: join(userData, 'crashDumps'),
    temp: join(userData, 'tmp'),
    hubRoot: join(home, '.mexerica-rc2'),
  }
}

const nested = (path: string, root: string) => path.startsWith(`${root}/`)

export function rc2PathsIsolated(home: string, appData: string): boolean {
  const rc2 = Object.values(rc2Paths(home, appData)).map(path => `${path}`)
  const rc1 = Object.values(rcPaths(home, appData)).map(path => `${path}`)
  return rc2.every(path => rc1.every(other => path !== other && !nested(path, other) && !nested(other, path)))
}

import { join } from 'node:path'
import { rcManifestHash } from './fork-rc-profile.ts'

const distribution = Object.freeze({
  profile: 'rc2-local',
  version: '0.0.1-rc.2',
  appId: 'app.mexerica.mac.rc2',
  name: 'Mexerica RC 2', // textos-ok: identidade fixa no manifesto e no diretório do perfil.
  scheme: 'mexerica-rc2',
  packageName: 'mexerica-rc2',
} as const)

export const rc2ManifestPath = 'rc2-manifest.json'

interface Target { platform: string; arch: string }

function checkTarget(version: unknown, target: Target) {
  if (version !== distribution.version || target.platform !== 'darwin' || target.arch !== 'arm64') {
    throw new Error('invalid_rc2_profile')
  }
}

const manifestBytes = () => Buffer.from(`${JSON.stringify(distribution)}\n`)

export function compileRc2Profile(mode: string, version: unknown, target: Target) {
  if (mode !== 'official-rc2') return null
  checkTarget(version, target)
  const bytes = manifestBytes()
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

export function validateRc2Bootstrap(input: Rc2Bootstrap) {
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
  if (!bytes.equals(manifestBytes()) || rcManifestHash(bytes) !== input.compiledPin) throw new Error('invalid_rc2_profile')
  return distribution
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

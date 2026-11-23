import { createHash } from 'node:crypto'
import { join } from 'node:path'

export const rcDistribution = Object.freeze({
  profile: 'rc',
  appId: 'app.donnabot.mac.rc',
  name: 'Mexerica RC', // textos-ok: identidade do pacote, diretório e Chaveiro.
  scheme: 'mexerica-rc',
  packageName: 'mexerica-rc',
} as const)

export const rcIdentity = Object.freeze({
  ...rcDistribution,
  environment: 'rc',
  cryptoName: rcDistribution.name,
  userDataFolder: rcDistribution.name,
  dataFolder: '.mexerica-rc',
  port: 19525,
  releaseUpdates: false,
} as const)

const rcVersion = /^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)-rc\.[1-9][0-9]*(?![\s\S])/
const invalid = () => new Error('invalid_rc_identity')

type ObjectFields = Record<string, unknown>
function record(value: unknown): ObjectFields {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw invalid()
  return value as ObjectFields
}

export function rcManifestHash(bytes: Uint8Array) {
  return createHash('sha256').update(bytes).digest('hex')
}

export function readRcManifest(bytes: Buffer) {
  const manifest = record(JSON.parse(bytes.toString('utf8')))
  const { version } = manifest
  if (Object.keys(manifest).length !== Object.keys(rcDistribution).length + 1 ||
      Object.entries(rcDistribution).some(([key, value]) => !Object.hasOwn(manifest, key) || manifest[key] !== value) ||
      typeof version !== 'string' || !rcVersion.test(version)) throw invalid()
  return { ...rcDistribution, version }
}

interface RcBootstrap {
  compiledProfile: string | undefined
  compiledManifestHash: string | undefined
  official: boolean
  packaged: boolean
  name: string
  version: string
  readMetadata: () => unknown
  readManifest: () => Buffer
}

export function validateRcBootstrap(input: RcBootstrap) {
  if (!input.packaged && input.compiledProfile === undefined && input.compiledManifestHash === undefined) return false
  const metadata = input.packaged ? record(input.readMetadata()) : {}
  const candidate = input.compiledProfile !== undefined || input.compiledManifestHash !== undefined ||
    metadata.mexericaProfile !== undefined || metadata.mexericaAppId === rcDistribution.appId ||
    metadata.name === rcDistribution.packageName || metadata.productName === rcDistribution.name ||
    input.name === rcDistribution.name || /-rc(?:[.\-+]|$)/i.test(input.version)
  if (!candidate) return false
  if (!input.packaged || !input.official || input.compiledProfile !== rcDistribution.profile || !input.compiledManifestHash ||
      metadata.mexericaProfile !== rcDistribution.profile || metadata.mexericaAppId !== rcDistribution.appId ||
      metadata.productName !== rcDistribution.name || input.name !== rcDistribution.name || metadata.name !== rcDistribution.packageName ||
      metadata.version !== input.version || !rcVersion.test(input.version)) throw invalid()
  const bytes = input.readManifest()
  if (rcManifestHash(bytes) !== input.compiledManifestHash || readRcManifest(bytes).version !== input.version) throw invalid()
  return true
}

export function rcPaths(home: string, appData: string) {
  const userData = join(appData, rcIdentity.userDataFolder)
  const sshData = join(home, rcIdentity.dataFolder)
  return {
    userData,
    sessionData: join(userData, 'session'),
    logs: join(userData, 'logs'),
    crashDumps: join(userData, 'crashDumps'),
    temp: join(userData, 'tmp'),
    sshData,
  }
}

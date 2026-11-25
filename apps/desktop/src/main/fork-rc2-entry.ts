import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { app } from 'electron'
import { rc2ManifestPath, rc2PathsIsolated, validateRc2Bootstrap } from '../shared/fork-rc2-profile.ts'
import { verifyRc2Artifact, verifyRc2StorageRoots } from './fork-rc2-artifact.ts'
import { nativeRc2Transport } from './fork-rc2-local-hub.ts'

declare const __MEXERICA_RC2_PROFILE__: string | undefined
declare const __MEXERICA_RC2_MANIFEST_HASH__: string | undefined

function boot(): never {
  const artifact = validateRc2Bootstrap({
    platform: process.platform, arch: process.arch, packaged: app.isPackaged, name: app.getName(), version: app.getVersion(),
    compiledProfile: typeof __MEXERICA_RC2_PROFILE__ === 'undefined' ? undefined : __MEXERICA_RC2_PROFILE__,
    compiledPin: typeof __MEXERICA_RC2_MANIFEST_HASH__ === 'undefined' ? undefined : __MEXERICA_RC2_MANIFEST_HASH__,
    readMetadata: () => JSON.parse(readFileSync(join(app.getAppPath(), 'package.json'), 'utf8')),
    readManifest: () => readFileSync(join(process.resourcesPath, rc2ManifestPath)),
  })
  if (!rc2PathsIsolated(app.getPath('home'), app.getPath('appData'))) throw new Error('invalid_rc2_profile')
  verifyRc2StorageRoots(app.getPath('home'), app.getPath('appData'))
  verifyRc2Artifact(process.resourcesPath, artifact.components)
  if (!nativeRc2Transport()) throw new Error('rc2_native_transport_unavailable')
  throw new Error('rc2_runtime_disabled')
}

try { boot() } catch { app.exit(1) }

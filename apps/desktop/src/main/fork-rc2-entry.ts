import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { app } from 'electron'
import { rc2ManifestPath, validateRc2Bootstrap } from '../shared/fork-rc2-profile.ts'
import { nativeRc2Transport } from './fork-rc2-local-hub.ts'

declare const __MEXERICA_RC2_PROFILE__: string | undefined
declare const __MEXERICA_RC2_MANIFEST_HASH__: string | undefined

function boot(): never {
  validateRc2Bootstrap({
    platform: process.platform, arch: process.arch, packaged: app.isPackaged, name: app.getName(), version: app.getVersion(),
    compiledProfile: typeof __MEXERICA_RC2_PROFILE__ === 'undefined' ? undefined : __MEXERICA_RC2_PROFILE__,
    compiledPin: typeof __MEXERICA_RC2_MANIFEST_HASH__ === 'undefined' ? undefined : __MEXERICA_RC2_MANIFEST_HASH__,
    readMetadata: () => JSON.parse(readFileSync(join(app.getAppPath(), 'package.json'), 'utf8')),
    readManifest: () => readFileSync(join(process.resourcesPath, rc2ManifestPath)),
  })
  if (!nativeRc2Transport()) throw new Error('rc2_native_transport_unavailable')
  throw new Error('rc2_runtime_disabled')
}

try { boot() } catch { app.exit(1) }

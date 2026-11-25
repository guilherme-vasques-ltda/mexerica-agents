import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { app } from 'electron'
import { rc2ManifestPath, rc2PathsIsolated, validateRc2Bootstrap } from '../shared/fork-rc2-profile.ts'
import { verifyRc2Artifact, verifyRc2StorageRoots } from './fork-rc2-artifact.ts'
import { Rc2HubLifecycle } from './fork-rc2-local-hub.ts'
import { rc2NativeTransport } from './fork-rc2-transport.ts'

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
  const verified = verifyRc2Artifact(process.resourcesPath, artifact.components)
  let transport: ReturnType<typeof rc2NativeTransport>
  try { transport = rc2NativeTransport(verified) } catch { verified.close(); throw new Error('rc2_native_transport_unavailable') }
  if (!transport) { verified.close(); throw new Error('rc2_native_transport_unavailable') }
  // Encerrar o Electron encerra o Hub: o ciclo de vida morre com o processo no will-quit,
  // escalonando end, SIGTERM e SIGKILL ao grupo com sonda de sobreviventes.
  const lifecycle = new Rc2HubLifecycle(() => null, transport)
  app.on('will-quit', () => { void lifecycle.stop() })
  // Nenhuma ativação ainda: sem sessão o start é recusado e o descritor fixado morre com este
  // processo que se recusa. Ligar exige a prova nativa do executável por descritor no macOS.
  throw new Error('rc2_runtime_disabled')
}

try { boot() } catch { app.exit(1) }

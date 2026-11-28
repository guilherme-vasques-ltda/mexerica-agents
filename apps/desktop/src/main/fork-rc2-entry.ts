import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { app, dialog } from 'electron'
import { traduzir } from '../renderer/lib/fork-i18n/index.ts'
import { FALLBACK_LANGUAGE, sugerirIdioma } from '../shared/fork-idioma.ts'
import { rc2ManifestPath, rc2PathsIsolated, validateRc2Bootstrap } from '../shared/fork-rc2-profile.ts'
import { verifyRc2Artifact, verifyRc2StorageRoots } from './fork-rc2-artifact.ts'
import { rc2DeveloperIdVerifier } from './fork-rc2-developer-id.ts'
import { Rc2HubLifecycle } from './fork-rc2-local-hub.ts'
import { rc2NativeTransport } from './fork-rc2-transport.ts'

declare const __MEXERICA_RC2_PROFILE__: string | undefined
declare const __MEXERICA_RC2_MANIFEST_HASH__: string | undefined

function verifyStartup(): void {
  const artifact = validateRc2Bootstrap({
    platform: process.platform, arch: process.arch, packaged: app.isPackaged, name: app.getName(), version: app.getVersion(),
    compiledProfile: typeof __MEXERICA_RC2_PROFILE__ === 'undefined' ? undefined : __MEXERICA_RC2_PROFILE__,
    compiledPin: typeof __MEXERICA_RC2_MANIFEST_HASH__ === 'undefined' ? undefined : __MEXERICA_RC2_MANIFEST_HASH__,
    readMetadata: () => JSON.parse(readFileSync(join(app.getAppPath(), 'package.json'), 'utf8')),
    readManifest: () => readFileSync(join(process.resourcesPath, rc2ManifestPath)),
  })
  if (!rc2PathsIsolated(app.getPath('home'), app.getPath('appData'))) throw new Error('invalid_rc2_profile')
  verifyRc2StorageRoots(app.getPath('home'), app.getPath('appData'))
  const verified = verifyRc2Artifact(process.resourcesPath, artifact.components, rc2DeveloperIdVerifier(process.resourcesPath))
  try {
    const transport = rc2NativeTransport(verified)
    if (transport) {
      const lifecycle = new Rc2HubLifecycle(() => null, transport)
      app.on('will-quit', () => { void lifecycle.stop() })
    }
  } finally {
    verified.close()
  }
}

let verified = false
try {
  verifyStartup()
  verified = true
} catch {}

try {
  const code = verified ? 'RC2-INACTIVE' : 'RC2-VERIFY'
  console.error(code)
  const idioma = sugerirIdioma(app.getPreferredSystemLanguages()) ?? FALLBACK_LANGUAGE
  const mensagem = traduzir(idioma, verified ? 'avisoLocal.rc2Inativa' : 'avisoLocal.rc2Verificacao')
  dialog.showErrorBox(traduzir(idioma, 'avisoLocal.rc2Titulo'), `${mensagem}\n\n${code}`)
} finally {
  app.exit(1)
}

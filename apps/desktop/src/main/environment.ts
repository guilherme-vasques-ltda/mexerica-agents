// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { idiomaDoApp, suggestedLanguage } from './fork-idioma'
import { traduzir } from '../renderer/lib/fork-i18n/index.ts'
import { privateDirectory } from '../shared/fork-private-files'
import { homedir } from 'node:os'
import { mexericaHome } from '../shared/fork-data-home'
import { lstatSync, mkdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { app, dialog } from 'electron'
import { keychainFailure, readStorageGeneration } from './fork-keychain/generation'
import { sandboxLabel } from '../shared/dev-sandbox'
import { environmentProfile } from '../shared/environment'
import { officialDesktop } from '../shared/fork-features'
import { officialAccount, officialConfigHash } from '../shared/fork-official-account'
import { rcIdentity, rcPaths, validateRcBootstrap } from '../shared/fork-rc-profile'

declare const __MEXERICA_OFFICIAL_CONFIG_HASH__: string
declare const __MEXERICA_COMPILED_PROFILE__: string
declare const __MEXERICA_RC_MANIFEST_HASH__: string

interface BuildConfig {
  environment: 'main' | 'dev'
  clerkPublishableKey?: string | null
  cloudURL?: string | null
  clerkFrontendOrigin?: string | null
  clerkProxyOrigin?: string | null
  clerkTokenOrigin?: string | null
  clerkIssuer?: string | null
  deploymentId?: string | null
  deploymentEnv?: 'main' | 'dev' | null
  manifestHash?: string | null
  deploymentManifest?: unknown
}

const path = app.isPackaged
  ? join(process.resourcesPath, 'account-config.json')
  : join(__dirname, '../../resources/account-config.json')
function readOfficialConfig() {
  try {
    const config = officialAccount(JSON.parse(readFileSync(path, 'utf8')))
    return config && officialConfigHash(config) === __MEXERICA_OFFICIAL_CONFIG_HASH__ ? config : null
  } catch { return null }
}

export const isRc = validateRcBootstrap({
  compiledProfile: typeof __MEXERICA_COMPILED_PROFILE__ === 'undefined' ? undefined : __MEXERICA_COMPILED_PROFILE__,
  compiledManifestHash: typeof __MEXERICA_RC_MANIFEST_HASH__ === 'undefined' ? undefined : __MEXERICA_RC_MANIFEST_HASH__,
  official: officialDesktop,
  packaged: app.isPackaged,
  name: app.getName(),
  version: app.getVersion(),
  readMetadata: () => JSON.parse(readFileSync(join(app.getAppPath(), 'package.json'), 'utf8')),
  readManifest: () => readFileSync(join(process.resourcesPath, 'rc-manifest.json')),
})
export const officialLoginConfig = officialDesktop ? readOfficialConfig() : null
if (isRc && !officialLoginConfig) throw new Error(traduzir('en', 'marca.rc.contaInvalida'))
export const buildConfig: BuildConfig = officialDesktop
  ? { environment: 'main' }
  : JSON.parse(readFileSync(path, 'utf8')) as BuildConfig
if (buildConfig.environment !== 'main' && buildConfig.environment !== 'dev') throw new Error('The app build environment is missing.')
export const localIdentity = environmentProfile(buildConfig.environment)
export const identity = isRc ? rcIdentity : localIdentity
const isolatedPaths = isRc ? rcPaths(homedir(), app.getPath('appData')) : null
export const hostDataDir = isolatedPaths?.sshData ?? mexericaHome(homedir(), identity.dataFolder, process.env)
if (!isRc && process.platform !== 'win32') privateDirectory(hostDataDir, false)

// Before the single-instance lock, preferences, account restore or Chromium session is created.
// A development run against its own host data (`MEXERICA_HOME`, a worktree's sandbox) keeps all of
// that there too, under its own name, so it runs beside the main checkout's Donnabot Dev.
if (isolatedPaths) {
  app.setName(identity.cryptoName)
  if (process.platform === 'win32') app.setAppUserModelId(identity.appId)
  for (const name of ['user-data-dir', 'disk-cache-dir', 'log-file', 'crash-dumps-dir']) app.commandLine.removeSwitch(name)
  const { sshData, ...appPaths } = isolatedPaths
  for (const path of [sshData, ...Object.values(appPaths)]) {
    if (process.platform === 'win32') {
      mkdirSync(path, { recursive: true })
      const stat = lstatSync(path)
      if (!stat.isDirectory() || stat.isSymbolicLink()) throw new Error(traduzir('en', 'marca.dados.paiInvalido'))
    } else privateDirectory(path)
  }
  for (const [name, path] of Object.entries(appPaths)) app.setPath(name, path)
  app.commandLine.appendSwitch('user-data-dir', appPaths.userData)
} else if (identity.environment === 'dev') {
  const sandbox = app.isPackaged || process.env.MEXERICA_HOME === undefined ? undefined : hostDataDir
  app.setName(sandbox ? sandboxLabel(identity.cryptoName, sandbox) : identity.cryptoName)
  app.setPath('userData', sandbox ? join(sandbox, 'desktop') : join(app.getPath('appData'), identity.userDataFolder))
} else {
  // Donnabot: its own name, userData, single-instance lock and safeStorage key, never Codync's.
  app.setName(identity.cryptoName)
  app.setPath('userData', join(app.getPath('appData'), identity.userDataFolder))
}

// No await: stop module evaluation before any consumer can open a newer generation.
try { readStorageGeneration(app.getPath('userData'), process.platform) }
catch (error) {
  const idioma = suggestedLanguage() ?? idiomaDoApp()
  dialog.showErrorBox(traduzir(idioma, 'sistema.barra.cofre.titulo'), traduzir(idioma, `sistema.barra.cofre.${keychainFailure(error)}`))
  app.exit(1)
  throw error
}

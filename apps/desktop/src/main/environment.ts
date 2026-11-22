// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { idiomaDoApp, suggestedLanguage } from './fork-idioma'
import { traduzir } from '../renderer/lib/fork-i18n/index.ts'
import { privateDirectory } from '../shared/fork-private-files'
import { homedir } from 'node:os'
import { mexericaHome } from '../shared/fork-data-home'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { app, dialog } from 'electron'
import { keychainFailure, readStorageGeneration } from './fork-keychain/generation'
import { sandboxLabel } from '../shared/dev-sandbox'
import { environmentProfile } from '../shared/environment'
import { officialDesktop } from '../shared/fork-features'
import { officialAccount, officialConfigHash } from '../shared/fork-official-account'

declare const __MEXERICA_OFFICIAL_CONFIG_HASH__: string

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

export const officialLoginConfig = officialDesktop ? readOfficialConfig() : null
export const buildConfig: BuildConfig = officialDesktop
  ? { environment: 'main' }
  : JSON.parse(readFileSync(path, 'utf8')) as BuildConfig
if (buildConfig.environment !== 'main' && buildConfig.environment !== 'dev') throw new Error('The app build environment is missing.')
export const identity = environmentProfile(buildConfig.environment)
export const hostDataDir = mexericaHome(homedir(), identity.dataFolder, process.env)
if (process.platform !== 'win32') privateDirectory(hostDataDir, false)

// Before the single-instance lock, preferences, account restore or Chromium session is created.
// A development run against its own host data (`MEXERICA_HOME`, a worktree's sandbox) keeps all of
// that there too, under its own name, so it runs beside the main checkout's Donnabot Dev.
if (identity.environment === 'dev') {
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

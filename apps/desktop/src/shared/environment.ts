// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { traduzir } from '../renderer/lib/fork-i18n/index.ts'
/**
 * Installed identities. Donnabot owns its own resources (`app.donnabot.*`, `~/.mexerica*`, ports
 * 194xx) so it runs beside an installed Codync and Codync Dev (docs/guides/mexerica-recursos.md).
 */
export function environmentProfile(environment: 'main' | 'dev') {
  const dev = environment === 'dev'
  return {
    environment,
    name: dev ? 'Donnabot Dev' : 'Donnabot',
    // Persistent identities: never derive these from the visible product name.
    // textos-ok: identidade técnica do Chaveiro, não é nome visível.
    cryptoName: dev ? 'Donnabot Dev' : 'Donnabot',
    // textos-ok: pasta legada de dados, não é texto de interface.
    userDataFolder: dev ? 'Donnabot Dev' : 'Donnabot',
    appId: dev ? 'app.donnabot.mac.dev' : 'app.donnabot.mac',
    scheme: dev ? 'donnabot-dev' : 'donnabot',
    dataFolder: dev ? '.mexerica-dev' : '.mexerica',
    port: dev ? 19423 : 19422,
    hostLabel: dev ? 'app.donnabot.dev.host' : 'app.donnabot.host',
    systemdUnit: dev ? 'donnabot-dev-host.service' : 'donnabot-host.service',
    windowsService: dev ? 'DonnabotDevHost' : 'DonnabotHost',
    // Windows is out of the fork's plan: only the binary names follow Donnabot.
    supervisor: dev ? 'donnabot-dev-hostw.exe' : 'donnabot-hostw.exe',
    screenLabel: dev ? 'app.donnabot.dev.screen' : 'app.donnabot.screen',
    screenName: dev ? 'DonnabotDevScreen' : 'DonnabotScreen',
    releaseUpdates: !dev,
  }
}

/** Codync's ports: 19222 (Codync), 19223 (Codync Dev) and 19300–19399 (its dev sandboxes). */
export const isCodyncPort = (port: number) => port === 19222 || port === 19223 || (port >= 19300 && port <= 19399)

/** Codync's data folders under the home folder: `.codync`, `.codync-dev`, `.codync-dev-<sandbox>`, in any case. */
export function isCodyncDataDir(home: string, dir: string) {
  const strip = (path: string) => path.replace(/[\\/]+$/, '').toLowerCase()
  const base = strip(home)
  const target = strip(dir)
  if (!target.startsWith(`${base}/`) && !target.startsWith(`${base}\\`)) return false
  return target.slice(base.length + 1).split(/[\\/]/)[0]?.startsWith('.codync') ?? false
}

/** Refuse to manage a different environment's daemon, including pre-isolation dev hosts. */
export function hostEnvironmentError(expected: 'main' | 'dev', actual?: string): string | null {
  if (actual === expected || (expected === 'main' && actual === undefined)) return null
  return traduzir('pt-BR', 'marca.hub.ambiente', { app: traduzir('pt-BR', expected === 'dev' ? 'marca.app.nomeDev' : 'marca.app.nome'), ambiente: actual ?? traduzir('pt-BR', 'marca.hub.ambienteDesconhecido') })
}

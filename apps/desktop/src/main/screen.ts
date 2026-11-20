// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { existsSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { app, ipcMain } from 'electron'
import { prefs } from './host-controller'
import { identity } from './environment'
import { shouldSyncScreenAgent } from '../shared/computer-access'

// Codync Screen (capture + input for Remote screen): on macOS a launchd agent inside the app
// bundle (apps/screen-macos, registered through SMAppService), which owns the Screen Recording
// and Accessibility grants. Linux starts its helper from the host, so nothing happens here.

const serviceName = `${identity.screenLabel}.plist`
const isMac = process.platform === 'darwin'

/** Identifies the helper binary inside this app: its path and modification date. */
function helperStamp() {
  const path = join(process.resourcesPath, '..', `Library/LoginItems/${identity.screenName}.app/Contents/MacOS/${identity.screenName}`)
  try {
    return `${path}@${statSync(path).mtimeMs}`
  } catch {
    return `${path}@0`
  }
}

function status(): string {
  if (!isMac || !app.isPackaged) return 'not-registered'
  return app.getLoginItemSettings({ type: 'agentService', serviceName }).status ?? 'not-registered'
}

/** Fork D331 (caixa D277): status bruto do agente deste app; nulo quando não há consulta ao sistema. */
export const screenAgentRawStatus = (): string | null =>
  isMac && app.isPackaged ? app.getLoginItemSettings({ type: 'agentService', serviceName }).status ?? null : null

/** Registers (or removes) the agent; returns whether macOS wants the user to allow it. */
export function setScreenAgent(on: boolean): { needsApproval: boolean } {
  if (!isMac || !app.isPackaged) return { needsApproval: false }
  // Registering an enabled agent is a no-op, so a helper launchd gave up on would never start again.
  if (on && status() === 'enabled') app.setLoginItemSettings({ type: 'agentService', serviceName, openAtLogin: false })
  app.setLoginItemSettings({ type: 'agentService', serviceName, openAtLogin: on })
  prefs.set('screenHelperStamp', on ? helperStamp() : null)
  prefs.set('screenHelperResumeAfterUpdate', false)
  return { needsApproval: status() === 'requires-approval' }
}

/**
 * Keeps Codync Screen registered while the host has Remote screen on, and registers it again
 * when this app ships a different helper binary (launchd keeps running the old one).
 */
export function syncScreenAgent(enabled: boolean): { needsApproval: boolean } {
  if (!isMac || !app.isPackaged || !enabled) return { needsApproval: false }
  // A first registration belongs to explicit setup, never application startup.
  if (!shouldSyncScreenAgent(enabled, status(), prefs.get('screenHelperResumeAfterUpdate') === true)) {
    return { needsApproval: status() === 'requires-approval' }
  }
  const stale = prefs.get('screenHelperStamp') !== helperStamp()
  if (!stale && status() === 'enabled') return { needsApproval: false }
  return setScreenAgent(true)
}

/** Before the app is replaced: the old helper stops along with its job. */
export function unregisterScreenAgent() {
  if (!isMac || !app.isPackaged || status() !== 'enabled') return
  prefs.set('screenHelperResumeAfterUpdate', true)
  app.setLoginItemSettings({ type: 'agentService', serviceName, openAtLogin: false })
  prefs.set('screenHelperStamp', null)
}

export function registerScreenIPC() {
  ipcMain.handle('screen:state', () => ({
    available: !isMac || (app.isPackaged && existsSync(join(process.resourcesPath, '..', `Library/LoginItems/${identity.screenName}.app/Contents/MacOS/${identity.screenName}`))),
    needsApproval: status() === 'requires-approval',
    registered: status() === 'enabled',
  }))
  ipcMain.handle('screen:setAgent', (_e, on: boolean) => setScreenAgent(on))
  ipcMain.handle('screen:syncAgent', (_e, enabled: boolean) => syncScreenAgent(enabled))
}

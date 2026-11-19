import { randomBytes } from 'node:crypto'
import { BrowserWindow, dialog, ipcMain, type IpcMainInvokeEvent } from 'electron'
import { authenticate, AuthenticationEnded } from '../auth'
import { identity } from '../environment'
import { accountCallback } from './callback'
import { AccountProfileController, recoveryFile, rulesFromEnvironment } from './profile'
import type { AccountService } from './service'

/**
 * E6b: canais `accountProfile:*`, só da janela principal e um pedido por vez. O controlador vale
 * para uma sessão: outra conta começa sem TOTP pela metade.
 */
export function registerAccountProfile(account: AccountService, windows: () => BrowserWindow[]) {
  let current: { session: string; controller: AccountProfileController } | null = null
  let running = false
  const controller = () => {
    const session = account.profileSessionId
    if (!session) return null
    if (current?.session !== session) {
      current = {
        session,
        controller: new AccountProfileController({
          call: (method, path, body) => account.profileCall(method, path, body),
          sessionId: () => account.profileSessionId,
          socialProviders: () => account.state.socialProviders,
          passwordRules: () => rulesFromEnvironment(account.environment),
          callbackURL: () => `${identity.appId}://callback?attempt=${randomBytes(24).toString('base64url')}`,
          oauth: async (redirect, callback) => {
            try { return accountCallback(await authenticate(account.browserURL(redirect), identity.appId, callback), callback) }
            catch (error) { if (error instanceof AuthenticationEnded) throw { cancelled: true }; throw error }
          },
        }),
      }
    }
    return current.controller
  }
  const unavailable = { ok: false, failure: 'unavailable', message: 'Entre na conta para mudar estes ajustes.' } as const
  const handle = (channel: string, work: (c: AccountProfileController, ...args: unknown[]) => Promise<unknown>) => {
    ipcMain.handle(`accountProfile:${channel}`, async (event: IpcMainInvokeEvent, ...args: unknown[]) => {
      const c = account.isAccountWindow(event, windows()) ? controller() : null
      if (!c) return unavailable
      if (running) return { ok: false, failure: 'unavailable', message: 'Espere a mudança anterior terminar.' }
      running = true
      try { return await work(c, ...args) } finally { running = false }
    })
  }
  handle('load', c => c.load())
  handle('saveName', (c, first, last) => c.saveName(first, last))
  handle('setPassword', (c, cur, next) => c.setPassword(cur, next))
  handle('link', (c, provider) => c.link(provider))
  handle('unlink', (c, id) => c.unlink(id))
  handle('startTotp', c => c.startTotp())
  handle('confirmTotp', (c, code) => c.confirmTotp(code))
  handle('disableTotp', c => c.disableTotp())
  handle('regenerateCodes', c => c.regenerateCodes())
  handle('startReverify', c => c.startReverify())
  handle('sendReverifyCode', c => c.sendReverifyCode())
  handle('attemptReverify', (c, attempt) => c.attemptReverify(attempt))
  ipcMain.handle('accountProfile:saveCodes', async (event, codes: unknown) => {
    const content = account.isAccountWindow(event, windows()) && account.profileSessionId ? recoveryFile(codes) : null
    if (!content) return 'error'
    const window = BrowserWindow.fromWebContents(event.sender)
    const options = { defaultPath: 'donnabot-codigos-de-recuperacao.txt', buttonLabel: 'Salvar', filters: [{ name: 'Texto', extensions: ['txt'] }] }
    const picked = window ? await dialog.showSaveDialog(window, options) : await dialog.showSaveDialog(options)
    if (picked.canceled || !picked.filePath) return 'cancelled'
    try { const { writeFile } = await import('node:fs/promises'); await writeFile(picked.filePath, content, { encoding: 'utf8', mode: 0o600 }); return 'saved' } catch { return 'error' }
  })
}

// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { computerAccessStep, computerPermissionSettings, type ComputerPermission, type ScreenAgentState } from '@shared/computer-access'
import { normalizeScreen, screenWithoutAgent, type ScreenState } from '@shared/models'
import { Observable } from '../lib/observable'
import type { AppModel } from './app-model'
import { traduzir } from '../lib/fork-i18n/index.ts'

/** Read-only checks are safe on launch. Requests happen only after an explicit setup action. */
export class ComputerAccess extends Observable {
  screen: ScreenState | null = null
  agent: ScreenAgentState | null = null
  busy: ComputerPermission | 'enable' | null = null
  private checkError: string | null = null
  private actionError: string | null = null
  private lastRequest: ComputerPermission | null = null
  requested = new Set<ComputerPermission>()
  private refreshing = false
  private app: AppModel

  constructor(app: AppModel) { super(); this.app = app }

  get step() {
    return computerAccessStep(window.codync.platform, this.app.local?.connection.kind === 'online', this.screen, this.agent)
  }

  // D454: a store não lê a preferência de idioma (sem DOM); a reserva sem nome sai no inglês do catálogo.
  // O nome do auxiliar é técnico (DonnabotScreen.app, como aparece nos Ajustes do Sistema) e fica fora do catálogo (D443).
  get helperName() { return this.screen?.permissionApp || 'Donnabot Screen' /* textos-ok: nome técnico do app auxiliar de tela, igual em todo idioma */ }
  get computerName() { return this.app.local?.hostName || window.codync.computerName || traduzir('en', 'ajustes.acesso.seuComputador') }
  get error() { return this.checkError ?? this.actionError }

  async refresh() {
    if (this.refreshing) return
    this.refreshing = true
    try {
      const local = this.app.local
      this.agent = await window.codync.app.screenAgentState()
      // `local.client` passa pelo guarda de versão: sob mismatch some e nenhum `screenStatus` sai.
      if (local?.client && local.connection.kind === 'online') {
        const epoch = local.screenEpoch
        const screen = normalizeScreen(await local.client.call<ScreenState>('screenStatus', {}))
        // R1: o estado da tela é do Space (e do Hub) da store que perguntou; uma resposta atrasada não vale para outra.
        // R2/R3/R5: se o link caiu, a época mudou ou ainda não foi observada pelo stream, não guarda quem usa.
        const stale = local.connection.kind !== 'online' || !local.screenObserved(epoch)
        this.screen = this.app.local !== local ? null : screen && stale ? screenWithoutAgent(screen) : screen
      } else this.screen = null
      this.checkError = null
      const granted = this.lastRequest === 'portal'
        ? this.screen?.capture && this.screen.input
        : this.lastRequest && this.screen?.[this.lastRequest]
      if (granted || this.step === 'ready') this.actionError = null
    } catch (error) {
      this.screen = null
      this.checkError = message(error)
    } finally {
      this.refreshing = false
      this.changed()
    }
  }

  async enable() {
    if (this.busy) return
    this.busy = 'enable'
    this.actionError = null
    this.changed()
    try {
      await this.app.setRemoteScreen(true)
      if (this.app.screenError) throw new Error(this.app.screenError)
      await this.refresh()
    } catch (error) { this.actionError = message(error) }
    finally { this.busy = null; this.changed() }
  }

  async request(permission: ComputerPermission) {
    const local = this.app.local
    // Como antes: sem link não há o que pedir. Só um mismatch conhecido segue, para a recusa tipada.
    if (!local || (!local.client && !local.mismatch) || this.busy) return
    this.busy = permission
    this.actionError = null
    this.lastRequest = permission
    this.requested.add(permission)
    this.changed()
    try {
      const client = local.client ?? await local.ready()
      await client.call('requestScreenPermission', { permission }, 125_000)
      await this.refresh()
    } catch (error) {
      this.actionError = message(error)
      return { ok: false as const, error }
    } finally { this.busy = null; this.changed() }
    return { ok: true as const }
  }

  openSettings(permission: keyof typeof computerPermissionSettings) {
    window.codync.app.openSettings(computerPermissionSettings[permission])
  }
}

function message(error: unknown): string {
  if (error instanceof Error) return error.message
  if (error && typeof error === 'object' && 'message' in error) return String(error.message)
  return String(error)
}

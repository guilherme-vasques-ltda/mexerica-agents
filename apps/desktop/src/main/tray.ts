import { join } from 'node:path'
import { app, Menu, nativeImage, shell, Tray as ElectronTray } from 'electron'
import type { TraySummary, WindowCommand } from '../shared/ipc'
import type { HostController } from './host-controller'
import type { Updates } from './updates'
import { updateAction } from '../shared/update-action'
import { showsComputerAccess } from '../shared/fork-features'
import { setShowInDock, showInDock } from './dock'
import { identity } from './environment'
import { islandSupported, type Island } from './island'
import { islandLabel } from '../shared/island'
import { launchAtLogin, setLaunchAtLogin } from './login-item'
import { idiomaDoApp } from './fork-idioma'
import { nomeVisivel } from '../shared/fork-nome-visivel'
import { formatarData, tradutor } from '../renderer/lib/fork-i18n/index.ts'

interface Deps {
  openChat: () => unknown
  openPairing: () => void
  send: (command: WindowCommand, show?: boolean) => void
  host: HostController
  updates: Updates
  island: Island
  canPresent?: () => boolean
}

const resources = () => (app.isPackaged ? process.resourcesPath : join(__dirname, '../../resources'))
const TEXT_SIZES = [11, 12, 13, 14, 15, 16, 18]
// D454/D495 (fatia 10b): cada rótulo sai do catálogo no idioma efetivo do app (sem escolha, inglês); o main/index.ts
// refaz o menu quando ele muda. Substitui a tabela pt-BR de shared/fork-textos-barra.ts (#563): um mecanismo só.
const nome = () => nomeVisivel(app.getName())

/** The menu bar icon and its system menu: macOS owns layout, selection and submenus. */
export class Tray {
  private tray: ElectronTray
  private summary: TraySummary | null = null
  private lastJson: string | null = null
  private launchAtLogin = launchAtLogin()

  constructor(private deps: Deps) {
    this.tray = new ElectronTray(this.icon(false))
    this.tray.setToolTip(nomeVisivel(app.getName()))
    // Windows opens a tray icon's menu on right-click; a left-click opens the chat.
    if (process.platform === 'win32') this.tray.on('click', () => this.deps.openChat())
    this.rebuild()
  }

  private icon(alert: boolean) {
    // Black template glyphs vanish on Windows' dark taskbar: the app icon reads on either theme.
    if (process.platform === 'win32') return nativeImage.createFromPath(join(resources(), 'icon-tray.png'))
    const image = nativeImage.createFromPath(join(resources(), 'tray', alert ? 'trayAlertTemplate.png' : 'trayTemplate.png'))
    image.setTemplateImage(true)
    return image
  }

  update(summary: TraySummary) {
    if (this.deps.canPresent?.() === false) { this.clear(); return }
    // Resumo idêntico ao último: nem ícone nem menu precisam refazer.
    const json = JSON.stringify(summary)
    if (json === this.lastJson) return
    const attention = summary.needsAttention !== this.summary?.needsAttention
    this.lastJson = json
    this.summary = summary
    if (attention) this.tray.setImage(this.icon(summary.needsAttention))
    this.rebuild()
  }

  clear() {
    if (!this.summary) return
    this.summary = null
    this.lastJson = null
    this.tray.setImage(this.icon(false))
    this.rebuild()
  }

  hostChanged() {
    this.rebuild()
  }

  /** The renderer draws menu images at 2× (`tray-summary.ts`): shown at half their pixel size. */
  private image(dataURL: string | null) {
    if (!dataURL) return undefined
    const image = nativeImage.createEmpty()
    image.addRepresentation({ scaleFactor: 2, dataURL })
    return image.isEmpty() ? undefined : image
  }

  private rebuild() {
    const tr = tradutor(idiomaDoApp())
    const { host, send } = this.deps
    const s = this.summary
    const state = host.state
    const items: Electron.MenuItemConstructorOptions[] = []
    const status =
      state.kind === 'missingBinary' ? tr('sistema.barra.hub.naoEncontrado')
      : state.kind === 'notInstalled' ? tr('sistema.barra.hub.naoInstalado')
      : state.kind === 'starting' ? tr('sistema.barra.conectando')
      : state.kind === 'failed' ? tr('sistema.barra.hub.problema')
      : state.kind === 'disabled' ? (s?.status ?? tr('sistema.barra.conectando'))
      : (s?.status ?? tr('sistema.barra.conectado'))
    items.push({ label: status, enabled: false })
    items.push({ label: tr('sistema.barra.abrir', { nome: nome() }), accelerator: 'CmdOrCtrl+O', click: () => this.deps.openChat() })
    if (state.kind === 'running') {
      items.push({ label: tr('sistema.barra.parearIphone'), enabled: s?.canPair ?? false, click: () => this.deps.openPairing() })
    }
    items.push({ type: 'separator' })

    switch (state.kind) {
      case 'missingBinary':
        if (identity.environment === 'dev') {
          items.push({ label: tr('sistema.barra.recompileDev', { nome: nome() }), enabled: false })
          break
        }
        items.push({ label: tr('marca.barra.recompile'), enabled: false })
        break
      case 'notInstalled':
        items.push({ label: tr('sistema.barra.instalarHub'), click: () => void host.install() })
        break
      case 'starting':
        items.push({ label: tr('sistema.barra.iniciandoHub'), enabled: false })
        break
      case 'failed':
        items.push({ label: state.message, enabled: false })
        items.push({ label: tr('sistema.barra.reiniciarHub'), click: () => host.restart() })
        items.push({ label: tr('sistema.barra.abrirRegistro'), click: () => void shell.openPath(host.logPath) })
        break
      // D76: sem host local, o menu segue os bots dos computadores SSH (o Hub).
      case 'disabled':
      case 'running':
        if (s?.approval) {
          items.push({ label: tr('sistema.barra.revisarPedido', { aparelho: s.approval }), click: () => send({ kind: 'reviewApprovals' }) })
          items.push({ type: 'separator' })
        }
        if (!s || s.bots.length === 0) {
          items.push({ label: tr('sistema.barra.nenhumBot'), enabled: false })
        } else {
          for (const bot of s.bots) {
            const submenu: Electron.MenuItemConstructorOptions[] = [
              { label: bot.detail, enabled: false },
              { label: tr('sistema.barra.abrirConversa'), click: () => send({ kind: 'openBot', ref: bot.ref }) },
            ]
            if (bot.working) submenu.push({ label: tr('sistema.barra.pararTarefa'), click: () => send({ kind: 'stopBot', ref: bot.ref }, false) })
            items.push({ label: bot.name, icon: this.image(bot.avatar), submenu })
          }
        }
        if (s?.screen) {
          items.push({ type: 'separator' })
          items.push({ label: tr('sistema.barra.telaRemota'), submenu: this.screenMenu(s.screen) })
        }
        if (s && s.usage.length) {
          items.push({ type: 'separator' })
          for (const provider of s.usage) {
            // Buttons, not text: the menu dims a disabled item's image. They open the app.
            items.push({ label: provider.name, icon: this.image(provider.icon), click: () => this.deps.openChat() })
            for (const line of provider.lines) {
              items.push({ label: line.title, icon: this.image(line.bar), click: () => this.deps.openChat() })
            }
          }
        }
        break
    }

    items.push({ type: 'separator' })
    items.push({ label: tr('sistema.barra.ajustes'), submenu: this.settingsMenu() })
    if (s?.version) items.push({ label: tr('sistema.barra.versao', { versao: s.version }), enabled: false })
    items.push({ label: tr('sistema.barra.sair', { nome: nome() }), accelerator: 'CmdOrCtrl+Q', click: () => app.quit() })
    this.tray.setContextMenu(Menu.buildFromTemplate(items))
  }

  private screenMenu(screen: NonNullable<TraySummary['screen']>): Electron.MenuItemConstructorOptions[] {
    const tr = tradutor(idiomaDoApp())
    const items: Electron.MenuItemConstructorOptions[] = [
      {
        label: tr('sistema.barra.ativarTela'),
        type: 'checkbox',
        checked: screen.enabled,
        click: () => this.deps.send({ kind: 'setRemoteScreen', on: !screen.enabled }, false),
      },
      { label: screen.subtitle, enabled: false },
    ]
    // Fork: sem "Computer access" (sem host local, D76), o item some do menu.
    if (showsComputerAccess(this.deps.host.state)) items.push({ label: tr('sistema.barra.acessoComputador'), click: () => this.deps.send({ kind: 'computerAccess' }, true) })
    if (screen.error) items.push({ label: screen.error, enabled: false })
    return items
  }

  private updatesMenu(): Electron.MenuItemConstructorOptions[] {
    const tr = tradutor(idiomaDoApp())
    const { updates } = this.deps
    const u = updates.state
    const items: Electron.MenuItemConstructorOptions[] = [
      { ...updateAction(u, idiomaDoApp()), click: () => updates.check() },
      { label: tr('sistema.barra.autoProcurar'), type: 'checkbox', checked: u.autoCheck, enabled: u.supported, click: () => updates.emit('setAutoCheck', !u.autoCheck) },
      { label: tr('sistema.barra.autoBaixar'), type: 'checkbox', checked: u.autoDownload, enabled: u.supported, click: () => updates.emit('setAutoDownload', !u.autoDownload) },
    ]
    if (!u.supported) items.push({ label: tr('sistema.barra.soPublicadas'), enabled: false })
    if (u.lastCheck) items.push({ label: tr('sistema.barra.ultimaProcura', { quando: formatarData(idiomaDoApp(), u.lastCheck, { dateStyle: 'medium', timeStyle: 'short' }) }), enabled: false })
    if (u.error) {
      items.push({ label: u.error, enabled: false })
      items.push({ label: tr('sistema.barra.tentarAtualizar'), click: () => updates.check() })
    }
    return items
  }

  private settingsMenu(): Electron.MenuItemConstructorOptions[] {
    const tr = tradutor(idiomaDoApp())
    const { host, send, island } = this.deps
    const s = this.summary
    const localHostItems: Electron.MenuItemConstructorOptions[] = host.state.kind === 'disabled' ? [] : [
      { label: tr('sistema.barra.reiniciarHub'), click: () => host.restart() },
      { label: tr('sistema.barra.abrirRegistro'), click: () => void shell.openPath(host.logPath) },
      { type: 'separator' },
      { label: tr('sistema.barra.desinstalarHub'), click: () => void host.uninstall() },
      { type: 'separator' },
    ]
    const style = s?.usageIconStyle ?? 'character'
    const size = s?.textSize ?? 12
    return [
      {
        label: tr('sistema.barra.iconesUso'),
        submenu: [
          { label: tr('sistema.barra.personagem'), type: 'radio', checked: style === 'character', click: () => send({ kind: 'setUsageIconStyle', style: 'character' }, false) },
          { label: tr('sistema.barra.original'), type: 'radio', checked: style === 'original', click: () => send({ kind: 'setUsageIconStyle', style: 'original' }, false) },
        ],
      },
      {
        label: tr('sistema.barra.tamanhoTexto'),
        submenu: TEXT_SIZES.map((pt) => ({
          label: tr(pt === 12 ? 'sistema.barra.tamanhoPadrao' : 'sistema.barra.tamanhoPt', { n: pt }),
          type: 'radio' as const,
          checked: pt === size,
          click: () => send({ kind: 'setTextSize', size: pt }, false),
        })),
      },
      {
        label: tr('sistema.barra.abrirLogin'),
        type: 'checkbox',
        checked: this.launchAtLogin,
        click: () => {
          this.launchAtLogin = setLaunchAtLogin(!this.launchAtLogin)
          this.rebuild()
        },
      },
      ...(process.platform === 'darwin' ? [{ label: tr('sistema.barra.mostrarDock'), type: 'checkbox' as const, checked: showInDock(), click: () => { setShowInDock(!showInDock()); this.rebuild() } }] : []),
      ...(islandSupported ? [{ label: tr(islandLabel(process.platform)), type: 'checkbox' as const, checked: island.enabled, click: () => island.setEnabled(!island.enabled) }] : []),
      { type: 'separator' },
      { label: tr('sistema.barra.atualizacoes'), submenu: this.updatesMenu(), visible: this.deps.updates.state.supported },
      { type: 'separator' },
      ...localHostItems,
      { label: tr('sistema.barra.apagarDados'), click: () => send({ kind: 'confirmReset' }) },
    ]
  }
}

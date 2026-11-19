import { app } from 'electron'
import { prefs } from './host-controller'

export const showInDock = () => prefs.get('showInDock') !== false

export function setShowInDock(on: boolean) {
  prefs.set('showInDock', on)
  applyDockVisibility()
  return showInDock()
}

/** Um pedido de visibilidade: `pending` antes do show; `settled` quando ele resolve ou o hide volta; `failed` se lançam ou rejeitam. */
export type DockVisibilityChange = 'pending' | 'settled' | 'failed'
const watchers = new Set<(change: DockVisibilityChange) => void>()

/** MX-9: o ícone escolhido segue cada pedido, porque mostrar recria o item com a imagem do pacote. Só o último pedido avisa. */
export function onDockVisibility(watcher: (change: DockVisibilityChange) => void): () => void {
  watchers.add(watcher)
  return () => { watchers.delete(watcher) }
}

function tell(request: number, change: DockVisibilityChange) {
  // Um show atrasado de um pedido antigo não fala por cima do mais novo.
  if (request !== latest) return
  for (const watcher of [...watchers]) { try { watcher(change) } catch {} }
}

/** Quantos pedidos de visibilidade já houve (0: o início ainda não pediu). */
let latest = 0
/** O último `dock.show()` pedido (relógio de parede, como o do Electron) e quantos ainda não terminaram. */
let lastShow = -Infinity
let showing = 0
/** Quando o app foi ativado pela última vez (`did-become-active`), anotado a partir do primeiro pedido do início. */
let activatedAt = -Infinity
let watching = false

export function applyDockVisibility() {
  const dock = app.dock
  if (process.platform !== 'darwin' || !dock) return
  watchActivation()
  const request = ++latest
  if (!showInDock()) {
    try { dock.hide() } catch (error) { tell(request, 'failed'); throw error }
    tell(request, 'settled')
    return
  }
  tell(request, 'pending')
  lastShow = Date.now()
  showing++
  let shown: Promise<void>
  try { shown = Promise.resolve(dock.show()) } catch (error) { shown = Promise.reject(error) }
  // A promessa do Electron resolve quando o item aparece; uma rejeição vira `failed`, nunca unhandledrejection.
  void shown.then(() => { showing--; tell(request, 'settled') }, () => { showing--; tell(request, 'failed') })
}

/**
 * D169 N3: abrir pelo LaunchServices o app já aberto (Finder, Spotlight, `open`, o item fixado) volta a política para
 * regular antes do `activate`, sem pedido do app (fixture nativa R10, também com o activate vazio). Com "Mostrar no
 * Dock" desligado, oculta de novo pelo pedido de sempre, que avisa o fato. Só oculta: nunca mostra, não grava nada e
 * não age antes do primeiro pedido do início. Chamar antes de abrir a janela: ocultar com o app ativo o desativa.
 */
export function keepDockHidden() { hideAgain() }

/** A reconciliação acima; devolve o pedido quando o hide de fato tirou o item do Dock, senão 0. */
function hideAgain(): number {
  const dock = app.dock
  if (process.platform !== 'darwin' || !dock || latest === 0 || showInDock()) return 0
  try {
    if (!dock.isVisible()) return 0
    applyDockVisibility()
    return dock.isVisible() ? 0 : latest
  } catch { return 0 } // Um hide que lança não impede a janela de abrir.
}

/** O Electron 44.5.1 ignora um `hide` até 1 s depois de um `show` (`DockHide`, `browser_mac.mm`); folga para o relógio. */
const hideIgnoredMs = 1500
/**
 * Ativação recente: o LaunchServices ativa o app e então entrega o link (fixture R13: 29–36 ms antes do evento). É só um
 * sinal temporal: a hora do `did-become-active` não diz quem ativou o app, e uma volta da pessoa à Mexerica menos de
 * `recentActivationMs` antes de um link também é cedida (como o hide do pacote comum cede).
 */
const recentActivationMs = 1000

/** Só anota a hora da última ativação; nunca ativa, mostra ou foca nada. Sem a anotação, o link nunca cede. */
function watchActivation() {
  if (watching) return
  watching = true
  try { app.on('did-become-active', () => { activatedAt = Date.now() }) } catch {}
}

/**
 * D169 N4/S13: o fim do tratador do link (`open-url`), depois do login; não recebe o URL. Com "Mostrar no Dock"
 * desligado, devolve a ativação que o LaunchServices costuma dar ao app junto com o link, como no R11, e o app que estava
 * à frente volta à frente. Pacote comum: o LaunchServices pôs o app no Dock e ocultar de novo já cede a ativação. Pacote
 * agente (`LSUIElement`): o app não entra no Dock, mas é ativado; então cede: o `hide` de sempre (no Electron fixado,
 * `DockHide` marca as janelas abertas `canHide:NO`, e elas continuam na tela) e `app.hide()` ("o próximo app da fila é
 * ativado"). Só quando esse `hide` pode valer (item fora do Dock, nenhum `show` em curso nem há menos de
 * `hideIgnoredMs`, senão as janelas sumiriam com o app) e com o app ativo e ativado há no máximo `recentActivationMs`
 * (um sinal temporal, não prova de que a ativação veio do link; quem está na Mexerica há mais tempo continua nela). Uma
 * vez por ativação; fora disso não faz nada e não tenta de novo. Nunca abre, mostra ou foca janela nem grava nada.
 */
export function keepDockHiddenAfterLink() {
  if (hideAgain()) return
  const dock = app.dock
  if (process.platform !== 'darwin' || !dock || latest === 0 || showInDock()) return
  try {
    if (dock.isVisible() || showing > 0 || Date.now() - lastShow < hideIgnoredMs) return
    if (!app.isActive() || Date.now() - activatedAt > recentActivationMs) return
    activatedAt = -Infinity
    applyDockVisibility()
    if (dock.isVisible()) return
    app.hide()
  } catch {} // O login já resolveu; uma falha aqui deixa o app como o LaunchServices o deixou.
}

import type { MenuItemConstructorOptions } from 'electron'
import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'
import type { AppLanguage } from '../../shared/fork-idioma.ts'
import type { WindowCommand } from '../../shared/ipc'

export interface OpenTrayBot { ref: string; name: string; detail: string; working: boolean }
export interface OpenTraySummary { status: string; bots: OpenTrayBot[] }

export interface OpenTrayActions {
  show(): void
  send(command: WindowCommand, show: boolean): void
  quit(): void
}

const MAX_BOTS = 50
const MAX_TEXT = 200
const isText = (value: unknown): value is string => typeof value === 'string'
const short = (value: string) => value.length > MAX_TEXT ? `${value.slice(0, MAX_TEXT - 1)}…` : value

/** O resumo vem do renderer: só o que o menu usa passa, e qualquer forma fora do esperado é descartada (fail-closed). */
export function openTraySummary(value: unknown): OpenTraySummary | null {
  const raw = value as { status?: unknown; bots?: unknown } | null
  if (!raw || typeof raw !== 'object' || !isText(raw.status) || !Array.isArray(raw.bots)) return null
  const bots: OpenTrayBot[] = []
  for (const item of raw.bots.slice(0, MAX_BOTS) as Record<string, unknown>[]) {
    if (!item || typeof item !== 'object' || !isText(item.ref) || item.ref.length > MAX_TEXT || !isText(item.name) || !isText(item.detail) || typeof item.working !== 'boolean') return null
    bots.push({ ref: item.ref, name: short(item.name), detail: short(item.detail), working: item.working })
  }
  return { status: short(raw.status), bots }
}

/**
 * Menu da bandeja da Open: só o que a Open tem de verdade. Pareamento, tela remota, uso por conta, aprovações,
 * atualizações e o cofre ficam de fora porque o perfil não os oferece; nada aqui os finge desativados.
 */
export function openTrayMenu(summary: OpenTraySummary | null, name: string, idioma: AppLanguage, actions: OpenTrayActions): MenuItemConstructorOptions[] {
  const items: MenuItemConstructorOptions[] = [
    { label: summary?.status ?? traduzir(idioma, 'sistema.barra.conectando'), enabled: false },
    { label: traduzir(idioma, 'sistema.barra.abrir', { nome: name }), accelerator: 'CmdOrCtrl+O', click: () => actions.show() },
    { type: 'separator' },
  ]
  if (!summary || summary.bots.length === 0) items.push({ label: traduzir(idioma, 'sistema.barra.nenhumBot'), enabled: false })
  else for (const bot of summary.bots) {
    const submenu: MenuItemConstructorOptions[] = [
      { label: bot.detail, enabled: false },
      { label: traduzir(idioma, 'sistema.barra.abrirConversa'), click: () => actions.send({ kind: 'openBot', ref: bot.ref }, true) },
    ]
    if (bot.working) submenu.push({ label: traduzir(idioma, 'sistema.barra.pararTarefa'), click: () => actions.send({ kind: 'stopBot', ref: bot.ref }, false) })
    items.push({ label: bot.name, submenu })
  }
  items.push(
    { type: 'separator' },
    { label: traduzir(idioma, 'sistema.barra.ajustes'), click: () => actions.send({ kind: 'openSettings' }, true) },
    { label: traduzir(idioma, 'sistema.barra.sair', { nome: name }), accelerator: 'CmdOrCtrl+Q', click: () => actions.quit() },
  )
  return items
}

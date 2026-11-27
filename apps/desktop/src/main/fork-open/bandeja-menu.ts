import type { MenuItemConstructorOptions } from 'electron'
import { traduzir } from '../../renderer/lib/fork-i18n/index.ts'
import { SOURCE_LANGUAGE } from '../../shared/fork-idioma.ts'
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
const text = (value: unknown): value is string => typeof value === 'string' && value.length <= MAX_TEXT

/** O resumo vem do renderer: só o que o menu usa passa, e qualquer forma fora do esperado é descartada (fail-closed). */
export function openTraySummary(value: unknown): OpenTraySummary | null {
  const raw = value as { status?: unknown; bots?: unknown } | null
  if (!raw || typeof raw !== 'object' || !text(raw.status) || !Array.isArray(raw.bots) || raw.bots.length > MAX_BOTS) return null
  const bots: OpenTrayBot[] = []
  for (const item of raw.bots as Record<string, unknown>[]) {
    if (!item || typeof item !== 'object' || !text(item.ref) || !text(item.name) || !text(item.detail) || typeof item.working !== 'boolean') return null
    bots.push({ ref: item.ref, name: item.name, detail: item.detail, working: item.working })
  }
  return { status: raw.status, bots }
}

/**
 * Menu da bandeja da Open: só o que a Open tem de verdade. Pareamento, tela remota, uso por conta, aprovações,
 * atualizações e o cofre ficam de fora porque o perfil não os oferece; nada aqui os finge desativados.
 */
export function openTrayMenu(summary: OpenTraySummary | null, name: string, actions: OpenTrayActions): MenuItemConstructorOptions[] {
  const tr = (chave: Parameters<typeof traduzir>[1], params?: { nome: string }) => (traduzir as (i: typeof SOURCE_LANGUAGE, c: string, p?: object) => string)(SOURCE_LANGUAGE, chave, params)
  const items: MenuItemConstructorOptions[] = [
    { label: summary?.status ?? tr('sistema.barra.conectando'), enabled: false },
    { label: tr('sistema.barra.abrir', { nome: name }), accelerator: 'CmdOrCtrl+O', click: () => actions.show() },
    { type: 'separator' },
  ]
  if (!summary || summary.bots.length === 0) items.push({ label: tr('sistema.barra.nenhumBot'), enabled: false })
  else for (const bot of summary.bots) {
    const submenu: MenuItemConstructorOptions[] = [
      { label: bot.detail, enabled: false },
      { label: tr('sistema.barra.abrirConversa'), click: () => actions.send({ kind: 'openBot', ref: bot.ref }, true) },
    ]
    if (bot.working) submenu.push({ label: tr('sistema.barra.pararTarefa'), click: () => actions.send({ kind: 'stopBot', ref: bot.ref }, false) })
    items.push({ label: bot.name, submenu })
  }
  items.push(
    { type: 'separator' },
    { label: tr('sistema.barra.ajustes'), click: () => actions.send({ kind: 'openSettings' }, true) },
    { label: tr('sistema.barra.sair', { nome: name }), accelerator: 'CmdOrCtrl+Q', click: () => actions.quit() },
  )
  return items
}

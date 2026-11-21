// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useSyncExternalStore } from 'react'
import { isStringList, SIDEBAR_WIDTH } from './sidebar-layout'

/**
 * A per-window preference in localStorage (kit's `@AppStorage`), observable by views. `valid` checks the
 * shape of what was stored (valid JSON of the wrong type falls back too, without touching other keys).
 */
export function pref<T>(key: string, fallback: T, valid?: (value: unknown) => value is T) {
  const listeners = new Set<() => void>()
  const read = (raw: string | null): T => {
    if (raw === null) return fallback
    try {
      const parsed: unknown = JSON.parse(raw)
      return !valid || valid(parsed) ? (parsed as T) : fallback
    } catch {
      return fallback
    }
  }
  let value: T = (() => {
    try {
      return read(localStorage.getItem(key))
    } catch {
      return fallback
    }
  })()
  window.addEventListener('storage', (e) => {
    if (e.key !== key) return
    value = read(e.newValue)
    for (const l of listeners) l()
  })
  return {
    get: () => value,
    set(next: T) {
      value = next
      try {
        localStorage.setItem(key, JSON.stringify(next))
      } catch {}
      for (const l of listeners) l()
    },
    subscribe(l: () => void) {
      listeners.add(l)
      return () => void listeners.delete(l)
    },
  }
}

export type Pref<T> = ReturnType<typeof pref<T>>

export function usePref<T>(p: Pref<T>): [T, (v: T) => void] {
  const value = useSyncExternalStore(p.subscribe, p.get, p.get)
  return [value, p.set]
}

export const TEXT_SIZES = [11, 12, 13, 14, 15, 16, 18]
export const DEFAULT_TEXT_SIZE = 13

/** The next larger (`up`) or smaller size for ⌘+ / ⌘−, staying at either end. */
export function stepTextSize(size: number, up: boolean) {
  return (up ? TEXT_SIZES.find((s) => s > size) : [...TEXT_SIZES].reverse().find((s) => s < size)) ?? size
}

const isNumberMap = (v: unknown): v is Record<string, number> =>
  typeof v === 'object' && v !== null && !Array.isArray(v) && Object.values(v).every((n) => typeof n === 'number' && Number.isFinite(n))

export const prefs = {
  /** D548: opt-in local, false inclusive para valores persistidos inválidos. */
  commandSpaceBerga: pref<boolean>('commandSpaceBerga', false, (v): v is boolean => typeof v === 'boolean'),
  textSize: pref('conversationFontSize', DEFAULT_TEXT_SIZE),
  /** D165: the sidebar collapsed to the rail (the old compact, icon-only column is gone). */
  sidebarCompact: pref('sidebarCompact', false),
  sidebarWidth: pref<number>('desktopSidebarWidth', SIDEBAR_WIDTH.default),
  detailsPanelWidth: pref('detailsPanelWidth', 292),
  hiddenComputers: pref('hiddenComputers', ''),
  computerOrder: pref<string[]>('computerOrder', []),
  collapsedComputers: pref<string[]>('collapsedComputerSections', []),
  usageIconStyle: pref<'character' | 'original'>('usageIconStyle', 'character'),
  onboardingDone: pref('macAccountOnboardingCompleted', false),
  computerAccessSetup: pref<'ready' | 'later' | null>('computerAccessSetup', null),
  /** D165 fase C: Ajustes → Barra lateral (lib/sidebar-layout.ts). */
  sidebarSectionOrder: pref<string[]>('sidebarSectionOrder', [], isStringList),
  sidebarHiddenSections: pref<string[]>('sidebarHiddenSections', [], isStringList),
  sidebarFoldedSections: pref<string[]>('sidebarFoldedSections', [], isStringList),
  /** A última aba aberta em Extensões. */
  /** `agents` only as an old saved value: Extensões has no Agentes tab any more and opens Aplicativos (D169, 28/10). */
  marketplaceTab: pref<'apps' | 'connectors' | 'agents' | 'skills'>('marketplaceTab', 'apps'),
  /** A última aba interna escolhida em Aplicativos. */
  marketplaceApplicationTab: pref<'apps' | 'connectors'>('marketplaceApplicationTab', 'apps'),
  /** D432: o "lido até aqui" de cada conversa neste aparelho (bot:sessão → createdAt da última do bot vista). */
  chatReadMarks: pref<Record<string, number>>('chatReadMarks', {}, isNumberMap),
}

/** O corpo respeita a escolha; a escala dos detalhes preserva o piso de 11px do sistema. */
export function applyTextSize(size: number) {
  const clamped = Number.isFinite(size) ? Math.min(Math.max(size, 11), 18) : DEFAULT_TEXT_SIZE
  const scale = Math.max(1, clamped / DEFAULT_TEXT_SIZE)
  document.documentElement.style.setProperty('--scale', String(scale))
  document.documentElement.style.setProperty('--conversation', `${clamped}px`)
  // Match the reference chat at the selected size; keep larger text an explicit preference.
  document.documentElement.style.setProperty('--chat', `${clamped}px`)
  document.documentElement.style.setProperty('--chat-scale', String(scale))
}

prefs.textSize.subscribe(() => applyTextSize(prefs.textSize.get()))

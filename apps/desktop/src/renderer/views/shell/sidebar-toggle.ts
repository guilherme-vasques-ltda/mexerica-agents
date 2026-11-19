// D165 fase B (revisão B1): what the sidebar toggle (button, ⌃⌘S) does. In a narrow window the sidebar
// stays collapsed and the toggle shows or hides it floating; otherwise it flips the saved preference.
export interface SidebarToggle {
  narrow: boolean
  peek: boolean
  collapsed: boolean
  /** Rotinas or Ajustes covers the window (ajustes-tela-01): nothing floats behind it, the preference still flips. */
  covered?: boolean
}

export function toggledSidebar({ narrow, peek, collapsed, covered = false }: SidebarToggle): { peek: boolean; collapsed: boolean } {
  if (covered) return { peek: false, collapsed: narrow ? collapsed : !collapsed }
  if (narrow) return { peek: !peek, collapsed }
  return { peek: false, collapsed: !collapsed }
}

/**
 * D165 fase B (revisão B5/B6): whether a floating sidebar leaves. Pointer and focus come from the aside's
 * DOM node at that moment (`:hover`, `contains(activeElement)`), never from remembered React events: menus
 * in portals aren't inside it, and an aside that unmounted can't leave stale state.
 */
export function leavesFloating({ collapsed, layersOpen, pointerInside, focusInside }: {
  collapsed: boolean
  layersOpen: boolean
  pointerInside: boolean
  focusInside: boolean
}): boolean {
  return collapsed && !layersOpen && !pointerInside && !focusInside
}

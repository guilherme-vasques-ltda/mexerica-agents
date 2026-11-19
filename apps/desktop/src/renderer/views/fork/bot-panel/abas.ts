// As abas do painel direito (bot e canal) como lista declarativa (D335: uma aba extra condicional, como a futura
// aba do WhatsApp, entra só na lista). Puro, para testar sem DOM.

export interface PanelTabDef<C> {
  id: string
  /** A chave do catálogo com o rótulo da aba (sem parâmetros). */
  chave: string
  /** Ausente: sempre à vista. */
  visible?: (ctx: C) => boolean
}

export const visibleTabs = <C>(tabs: readonly PanelTabDef<C>[], ctx: C) => tabs.filter((t) => !t.visible || t.visible(ctx))

/** A aba guardada, se ainda estiver à vista; senão a primeira (Sessões). */
export function currentTab<C>(tabs: readonly PanelTabDef<C>[], saved: string): string {
  return tabs.some((t) => t.id === saved) ? saved : (tabs[0]?.id ?? saved)
}

/** Teclado do tablist: ←/→ andam em volta, Home e End vão às pontas; outras teclas, nada. */
export function tabKeyTarget(ids: readonly string[], current: string, key: string): string | null {
  if (!ids.length) return null
  const at = Math.max(0, ids.indexOf(current))
  switch (key) {
    case 'ArrowRight': return ids[(at + 1) % ids.length]!
    case 'ArrowLeft': return ids[(at - 1 + ids.length) % ids.length]!
    case 'Home': return ids[0]!
    case 'End': return ids[ids.length - 1]!
    default: return null
  }
}

/**
 * A aba guardada que sumiu (a do WhatsApp com o plugin desligado ou sem número): a primeira, para GRAVAR como seleção.
 * Assim, quando a aba volta, a seleção não troca sozinha. `null`: a guardada continua à vista.
 */
export function abaQueSumiu<C>(tabs: readonly PanelTabDef<C>[], saved: string): string | null {
  return tabs.some((t) => t.id === saved) ? null : (tabs[0]?.id ?? null)
}

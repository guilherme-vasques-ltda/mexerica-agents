// Lição da revisão de UI do Marco 2 (times-02, ui-r4): quando a resposta do Hub chega, o foco só volta ao cartão se
// ele se perdeu. Se a pessoa foi para outro controle, para uma caixa de texto ou para um menu durante a espera, o
// foco, o texto e o cursor ficam onde ela deixou.

/** O mínimo de um elemento que a regra lê (o DOM no app; objetos simples nos testes). */
export interface FocusTarget {
  readonly isConnected: boolean
  closest(selector: string): unknown
  getClientRects(): { length: number }
}

/** O foco se perdeu: está no corpo da página, num elemento removido, escondido, inerte ou sem caixa. */
export function focusLost(active: FocusTarget | null, body: unknown, root: unknown): boolean {
  if (!active || active === body || active === root) return true
  if (!active.isConnected) return true
  if (active.closest('[hidden], [inert]')) return true
  return active.getClientRects().length === 0
}

/** Foca `target` só se o foco atual se perdeu. Retorna se focou. */
export function restoreFocusIfLost(target: HTMLElement | null): boolean {
  if (!target || !focusLost(document.activeElement as HTMLElement | null, document.body, document.documentElement)) return false
  target.focus({ preventScroll: true })
  return true
}

/** Espera a saída real da camada, inclusive a animação, sem tirar o foco escolhido pela pessoa. */
export function restoreFocusAfterLayerExit(home: () => HTMLElement | null, layer: Element | null): () => void {
  let active = true
  let settled = false
  let frame = 0
  let observer: MutationObserver | null = null
  const finish = () => {
    if (!active || settled || layer?.isConnected) return
    settled = true
    observer?.disconnect()
    frame = requestAnimationFrame(() => { if (active) restoreFocusIfLost(home()) })
  }
  if (layer?.isConnected) {
    observer = new MutationObserver(finish)
    observer.observe(document.body, { childList: true, subtree: true })
  } else frame = requestAnimationFrame(finish)
  return () => { active = false; observer?.disconnect(); cancelAnimationFrame(frame) }
}

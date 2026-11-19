// D169 R6 (ChatHeader): where focus goes when Details closes. The header target is picked at close; on
// the next frame it gets focus only if focus is still lost (page body, a removed node or the closed
// panel). The returned cleanup belongs to this close: once it runs (Details opened again, or the
// conversation left), even a frame already queued does nothing.

type Focusable = { readonly isConnected: boolean; focus(options?: FocusOptions): void }
type Active = { readonly isConnected: boolean; closest(selector: string): unknown } | null
type Env = {
  active: () => Active
  body: () => unknown
  frame: (f: () => void) => number
  cancel: (id: number) => void
}

const browser: Env = {
  active: () => document.activeElement,
  body: () => document.body,
  frame: (f) => requestAnimationFrame(f),
  cancel: (id) => cancelAnimationFrame(id),
}

export function returnFocusAfterClose(target: Focusable, env: Env = browser): () => void {
  const lost = () => {
    const active = env.active()
    return !active || active === env.body() || !active.isConnected || !!active.closest('.details-panel')
  }
  if (!lost()) return () => {}
  let current = true
  const id = env.frame(() => {
    if (!current || !target.isConnected || !lost()) return
    target.focus({ preventScroll: true })
  })
  return () => {
    current = false
    env.cancel(id)
  }
}

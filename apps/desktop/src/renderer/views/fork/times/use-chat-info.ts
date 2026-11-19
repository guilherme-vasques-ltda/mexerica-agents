import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react'
import { useEscape } from '../../../components/Overlay'

// Mesmas camadas à frente reconhecidas pela janela; .anchored inclui painéis sem role=menu.
// Conta também a saída animada: a dica só volta à pilha depois da remoção real da camada.
const FRONT_LAYERS = '[aria-modal], [role="dialog"], [role="alertdialog"], [role="menu"], .context-layer, .anchored'

function hasFrontLayer() {
  return [...document.querySelectorAll<HTMLElement>(FRONT_LAYERS)].some((node) =>
    !node.closest('[hidden], [inert]') && node.getClientRects().length > 0,
  )
}

/** A dica cede a camadas superiores mesmo quando o hover começa depois delas. */
function useTipEscape(open: boolean, dismiss: () => void, layer: RefObject<HTMLElement | null>) {
  // Começa suspensa até conferir o DOM, antes da inscrição passiva de useEscape.
  const [covered, setCovered] = useState(true)
  useLayoutEffect(() => {
    if (!open) {
      setCovered(true)
      return
    }
    const refresh = () => setCovered(hasFrontLayer())
    refresh()
    const observer = new MutationObserver(refresh)
    observer.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['class', 'role', 'aria-modal', 'hidden', 'inert'],
    })
    return () => observer.disconnect()
  }, [open])
  // Nenhum listener de Escape próprio: a pilha existente consome só a camada ativa.
  useEscape(open && !covered, dismiss, layer)
  return open && !covered
}

/** Interação local do (i); não fecha Times nem muda o foco. */
export function useChatInfo() {
  const trigger = useRef<HTMLSpanElement>(null)
  const tip = useRef<HTMLSpanElement>(null)
  const [state, setState] = useState({ hover: false, focus: false, dismissed: false })
  const shown = useTipEscape((state.hover || state.focus) && !state.dismissed, () => {
    setState((current) => ({ ...current, dismissed: true }))
  }, trigger)

  const leaveHover = () => setState((current) => ({
    ...current, hover: false, dismissed: current.dismissed && current.focus,
  }))
  const inHoverRegion = (x: number, y: number) => {
    const button = trigger.current?.getBoundingClientRect()
    const text = tip.current?.getBoundingClientRect()
    const inside = (rect: DOMRect | undefined, bridge = 0) => !!rect
      && x >= rect.left && x <= rect.right && y >= rect.top - bridge && y <= rect.bottom
    // A caixa continua mensurável com visibility:hidden. Os 6 px são a ponte do CSS.
    return inside(button) || inside(text, 6)
  }

  useEffect(() => {
    if (!state.dismissed || !state.hover) return
    // Esconder o texto sob o ponteiro gera mouseleave sem a pessoa ter saído da região.
    // Acompanhar a saída real evita reabrir ao voltar diretamente ao botão; não intercepta cliques.
    const onMove = (event: MouseEvent) => {
      if (!inHoverRegion(event.clientX, event.clientY)) leaveHover()
    }
    const onExitWindow = (event: MouseEvent) => {
      if (event.relatedTarget === null && !inHoverRegion(event.clientX, event.clientY)) leaveHover()
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseout', onExitWindow)
    return () => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseout', onExitWindow)
    }
  }, [state.dismissed, state.hover])

  return {
    trigger,
    tip,
    shown,
    enterHover: () => setState((current) => ({ ...current, hover: true })),
    leaveHover: (x: number, y: number) => {
      if (state.dismissed && inHoverRegion(x, y)) return
      leaveHover()
    },
    focus: () => setState((current) => ({ ...current, focus: true })),
    blur: () => setState((current) => ({
      ...current, focus: false, dismissed: current.dismissed && current.hover,
    })),
  }
}

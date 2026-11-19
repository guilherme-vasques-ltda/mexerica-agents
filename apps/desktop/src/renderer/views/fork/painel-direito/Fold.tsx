import { useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import { reduceMotion } from '../../../lib/theme'

// Recorte com clip-path (não overflow), para as linhas guia da árvore não serem cortadas nas laterais.
const CLIP = 'inset(0 -48px 0 -48px)'

/**
 * Barra05 ("leve animação, e não seco"): a dobra abre crescendo de 0 à altura com fade (240 ms) e fecha
 * encolhendo (180 ms) antes de sumir. Sem animação com movimento reduzido. Interromper volta do ponto atual.
 */
export function Fold({ open, className, children }: { open: boolean; className?: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null)
  const [shown, setShown] = useState(open)
  const opening = useRef(false)

  useLayoutEffect(() => {
    if (open) {
      if (!shown) {
        opening.current = true
        setShown(true)
      }
      return
    }
    const el = ref.current
    if (!shown) return
    if (!el?.animate || reduceMotion()) return setShown(false)
    el.style.clipPath = CLIP
    const a = el.animate([{ height: `${el.getBoundingClientRect().height}px`, opacity: 1 }, { height: '0px', opacity: 0 }], { duration: 180, easing: 'cubic-bezier(0.4, 0, 1, 1)' })
    a.onfinish = () => setShown(false)
    return () => {
      a.onfinish = null
      a.cancel()
      el.style.clipPath = ''
    }
  }, [open])

  useLayoutEffect(() => {
    const el = ref.current
    if (!shown || !opening.current || !el) return
    opening.current = false
    if (!el.animate || reduceMotion()) return
    el.style.clipPath = CLIP
    const a = el.animate([{ height: '0px', opacity: 0 }, { height: `${el.getBoundingClientRect().height}px`, opacity: 1 }], { duration: 240, easing: 'cubic-bezier(0.22, 1, 0.36, 1)' })
    a.onfinish = () => { el.style.clipPath = '' }
  }, [shown])

  return shown ? <div ref={ref} className={className}>{children}</div> : null
}

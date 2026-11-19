import { useLayoutEffect, useRef, useState, type RefObject } from 'react'
import { reduceMotion } from '../../lib/theme'

/**
 * A352-1 (diagnóstico do Fable): a barra fixa abre e fecha animando a largura (duração e curva de `--layout`, o
 * movimento de layout do redesenho; nada com movimento reduzido), em vez de montar e desmontar de uma vez: conversa,
 * cartão de abas e botão da barra acompanham cada quadro, como no arrasto da borda. Fechando, a <aside> fica montada
 * com largura 0 até o fim. O estado é derivado na própria renderização, para ela continuar montada no mesmo commit em
 * que `collapsed` muda. Virar fixa a partir da flutuante e a primeira renderização não animam.
 */
export function useSidebarSlide(aside: RefObject<HTMLElement | null>, { collapsed, peek, width }: { collapsed: boolean; peek: boolean; width: number }) {
  const [closing, setClosing] = useState(false)
  const [prevCollapsed, setPrevCollapsed] = useState(collapsed)
  if (collapsed !== prevCollapsed) {
    setPrevCollapsed(collapsed)
    setClosing(collapsed && !peek && !reduceMotion())
  }
  if (closing && peek) setClosing(false)
  const mounted = !collapsed || peek || closing
  const docked = mounted && !collapsed
  const slide = useRef<Animation | null>(null)
  const was = useRef({ mounted: true, docked: true })
  // A última largura desenhada (inclui o arrasto, que escreve o estilo direto, e uma animação pela metade). O computed
  // depois do commit não serve de partida: com `.resizing` (sem transição; preso depois de recolher pelo arrasto),
  // ele já diz 0 e a barra pularia.
  const drawn = useRef(0)
  useLayoutEffect(() => {
    const el = aside.current
    if (!el) return
    drawn.current = el.getBoundingClientRect().width
    const observer = new ResizeObserver(([entry]) => { if (entry) drawn.current = entry.borderBoxSize[0]?.inlineSize ?? entry.contentRect.width })
    observer.observe(el)
    return () => observer.disconnect()
  }, [mounted])
  useLayoutEffect(() => {
    const el = aside.current
    if (el && docked !== was.current.docked && !reduceMotion()) {
      // Parte do valor visível agora (inclui uma animação pela metade) e vai a um destino explícito: a transição CSS
      // de largura (passos do teclado) dispara junto e o computed ainda diria 280.
      const now = getComputedStyle(el)
      const from = was.current.mounted ? { width: `${drawn.current}px`, marginLeft: now.marginLeft } : { width: '0px', marginLeft: '0px' }
      slide.current?.cancel()
      const to = docked ? { width: `${width}px`, marginLeft: getComputedStyle(el).marginLeft } : { width: '0px', marginLeft: '0px' }
      const [duration, ...easing] = getComputedStyle(document.documentElement).getPropertyValue('--layout').trim().split(' ')
      const a = el.animate([from, to], { duration: parseFloat(duration ?? '0') * 1000 || 0, easing: easing.join(' ') || 'linear' })
      a.onfinish = () => { if (slide.current === a) { slide.current = null; if (!docked) setClosing(false) } }
      slide.current = a
    }
    was.current = { mounted, docked }
  }, [mounted, docked])
  return { mounted, closing }
}

// Auditoria D249 (E2): a camada de Times fica com a janela; a tela carrega só ao abrir (fork/sob-demanda).
// Saiu de TimesView.tsx sem mudar o comportamento.
import { useContext, useEffect, useRef, type ReactNode } from 'react'
import { SubscriptionPaused, useModel } from '../../../lib/observable'
import { TimesScreenView } from '../sob-demanda/telas'
import { timesScreen } from './navigation'

/**
 * A janela mostra Times no lugar das conversas enquanto a tela está aberta, como Rotinas: a faixa e o fundo
 * continuam; o resto fica inerte e volta igual (rascunhos, seleção e rolagem) ao fechar.
 */
export function WithTimesScreen({ children }: { children: ReactNode }) {
  const screen = useModel(timesScreen)
  // MX-4 (C11): a camada de fora já pausada também pausa esta árvore (revisão P2-1).
  const inherited = useContext(SubscriptionPaused)
  const host = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const root = host.current
    if (!screen.open || !root) return
    const marked = new Set<Element>()
    const mark = () => {
      for (const el of root.querySelectorAll('.split > :not(.space-rail, .janela-header, .app-wallpaper, .app-veil)')) {
        if (el.hasAttribute('inert')) continue
        el.setAttribute('inert', '')
        marked.add(el)
      }
    }
    mark()
    const observer = new MutationObserver(mark)
    const split = root.querySelector('.split')
    if (split) observer.observe(split, { childList: true })
    return () => {
      observer.disconnect()
      for (const el of marked) el.removeAttribute('inert')
    }
  }, [screen.open])
  return (
    <div ref={host} className="times-host" data-coberto={screen.open ? '' : undefined}>
      {/* MX-4 (C11): coberta, a janela congela as assinaturas e ressincroniza ao voltar. */}
      <SubscriptionPaused.Provider value={inherited || screen.open}>{children}</SubscriptionPaused.Provider>
      {screen.open ? <div className="times-layer"><TimesScreenView key={screen.seq} /></div> : null}
    </div>
  )
}

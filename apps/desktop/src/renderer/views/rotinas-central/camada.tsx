// Auditoria D249 (E2): a camada de Rotinas, Ajustes e Extensões fica com a janela; cada tela carrega só ao abrir
// (fork/sob-demanda). Saiu de RoutinesCentralView.tsx sem mudar o comportamento.
import { useContext, useEffect, useRef, type ReactNode } from 'react'
import { SubscriptionPaused, useModel } from '../../lib/observable'
import { settingsScreen } from '../fork/ajustes/lugar-atual'
import { extensoesScreen } from '../fork/extensoes/lugar-atual'
import { AjustesTela, ExtensoesTela, RoutinesCentralView } from '../fork/sob-demanda/telas'
import { routinesScreen } from './navigation'

/** A janela mostra a tela de Rotinas no lugar das conversas enquanto ela está aberta. */
/**
 * D165 fase G (components/RoutinesCalendar): Rotinas takes the place of the sidebar and the conversation,
 * over the same wallpaper, with the rail still there. The window stays mounted underneath (drafts,
 * selection and scroll survive); while Rotinas is open, everything in it but the rail and the wallpaper
 * is inert, so neither the pointer nor Tab reaches what's behind.
 */
export function WithRoutinesScreen({ children }: { children: ReactNode }) {
  const screen = useModel(routinesScreen)
  // ajustes-tela-01: Ajustes is a screen like this one, over the same window (fork/ajustes).
  const settings = useModel(settingsScreen)
  // D249 E3: Extensões is a screen too.
  const extensions = useModel(extensoesScreen)
  const covered = screen.open || settings.open || extensions.open
  // MX-4 (C11): as camadas se aninham — a de fora já pausada também pausa esta árvore (revisão P2-1).
  const inherited = useContext(SubscriptionPaused)
  const host = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const root = host.current
    if (!covered || !root) return
    const marked = new Set<Element>()
    const mark = () => {
      for (const el of root.querySelectorAll('.split > :not(.space-rail, .janela-header, .app-wallpaper, .app-veil)')) {
        if (el.hasAttribute('inert')) continue
        el.setAttribute('inert', '')
        marked.add(el)
      }
    }
    mark()
    // The sidebar can mount again behind (the rail's toggle): it goes inert too.
    const observer = new MutationObserver(mark)
    const split = root.querySelector('.split')
    if (split) observer.observe(split, { childList: true })
    return () => {
      observer.disconnect()
      for (const el of marked) el.removeAttribute('inert')
    }
  }, [covered])
  // data-coberto no lugar de :has(> .rc-layer): o :has() reavaliava a subárvore inteira da
  // conversa a cada tecla digitada (medido: estilo de 37,9 → 4,5 ms por tecla sem ele).
  return (
    <div ref={host} className="rc-host" data-coberto={covered ? '' : undefined}>
      {/* MX-4 (C11): coberta, a janela congela as assinaturas — a loja segue viva e, ao voltar,
          um re-render ressincroniza sem perder estado, rascunho nem rolagem. */}
      <SubscriptionPaused.Provider value={inherited || covered}>{children}</SubscriptionPaused.Provider>
      {screen.open ? <div className="rc-layer"><RoutinesCentralView /></div> : null}
      {settings.open ? <div className="rc-layer"><AjustesTela /></div> : null}
      {extensions.open ? <div className="rc-layer"><ExtensoesTela /></div> : null}
    </div>
  )
}

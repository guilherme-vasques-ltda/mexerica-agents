import { startTransition, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useEscape } from '../../../components/Overlay'
import { usePref } from '../../../lib/prefs'
import { reduceMotion } from '../../../lib/theme'
import { PANEL_EASE } from './curva'
import { PanelPeek } from './peek'
import { PANEL_WIDTH, panelPrefs } from './prefs'
import './painel-direito.css'

export type RightPanel = ReturnType<typeof useRightPanel>

/**
 * Barra05 (passo 15): o painel direito solto (abre no hover da borda, por cima do chat, com vidro) ou fixo
 * (alfinete, ⌘I, dois cliques numa aba de sessão ou clique na alça; empurra o chat). `roomy`: a janela tem
 * lugar para o chat e o painel lado a lado; senão o fixo também abre por cima.
 */
export function useRightPanel(roomy: boolean) {
  const [pinnedPref, setPinned] = usePref(panelPrefs.pinned)
  // WhatsApp R5c: com a tela grande no centro, o painel sai de vista (`away`, só a alça) sem mudar a preferência;
  // `veil` o esconde enquanto a moldura do Recolher voa até ele.
  const [away, setAway] = useState(false)
  const [veil, setVeil] = useState(false)
  const pinned = pinnedPref && roomy && !away
  const [peekState, setPeekOpen] = useState(false)
  const peekOpen = peekState && !away
  const [entering, setEntering] = useState(false)
  const ref = useRef<HTMLElement>(null)
  const pinFx = useRef<{ from: 'peek'; transform: string; opacity: string } | { from: 'closed' } | null>(null)
  const enterTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  // O conteúdo chega em sequência (cabeçalho, Ligar, Agora, abas, lista) a cada abertura.
  const enter = () => {
    if (reduceMotion()) return
    clearTimeout(enterTimer.current)
    setEntering(true)
    enterTimer.current = setTimeout(() => setEntering(false), 560)
  }
  const peek = useMemo(() => new PanelPeek((open) => {
    setPeekOpen(open)
    if (open) enter()
  }), [])
  useEffect(() => () => {
    peek.dispose()
    clearTimeout(enterTimer.current)
  }, [peek])

  // Fixar e soltar animam a largura (0 ↔ 332 px, 320/220 ms); o conteúdo tem largura fixa e só é revelado.
  const togglePin = () => {
    const el = ref.current
    const still = reduceMotion() || !el?.animate
    if (pinned) {
      if (still) {
        peek.dismiss()
        return setPinned(false)
      }
      const a = el.animate([{ width: `${el.getBoundingClientRect().width}px`, marginRight: '8px', opacity: 1 }, { width: '0px', marginRight: '0px', opacity: 0 }], { duration: 220, easing: 'cubic-bezier(0.4, 0, 1, 1)' })
      a.onfinish = () => {
        peek.dismiss()
        setPinned(false)
      }
      return
    }
    // A352-4: vindo do hover, o painel já está à vista: ele fica parado (do ponto da entrada, se ainda entrava) e só
    // a reserva do chat cresce por baixo dele. Fechado, ele cresce de 0. Os dois rodam antes de pintar (layout effect).
    const cs = el ? getComputedStyle(el) : null
    pinFx.current = still || !roomy ? null : peek.open ? { from: 'peek', transform: cs?.transform ?? 'none', opacity: cs?.opacity ?? '1' } : { from: 'closed' }
    peek.dismiss()
    setPinned(true)
    if (pinFx.current?.from === 'closed') enter()
  }
  useLayoutEffect(() => {
    const fx = pinFx.current
    const el = ref.current
    pinFx.current = null
    if (!pinned || !fx || !el?.animate) return
    const ease = PANEL_EASE
    el.style.willChange = 'width, margin-left, transform'
    const done = () => { el.style.willChange = '' }
    if (fx.from === 'closed') {
      el.animate([{ width: '0px', marginRight: '0px', opacity: 0 }, { width: `${PANEL_WIDTH}px`, marginRight: '8px', opacity: 1 }], { duration: 320, easing: ease }).onfinish = done
      return
    }
    // Margem esquerda negativa do tamanho do painel: ele não ocupa lugar no fluxo e fica onde estava por cima.
    el.animate([{ marginLeft: `-${PANEL_WIDTH + 8}px` }, { marginLeft: '0px' }], { duration: 320, easing: ease }).onfinish = done
    if (fx.transform !== 'none' || fx.opacity !== '1') el.animate([{ transform: fx.transform, opacity: fx.opacity }, { transform: 'none', opacity: 1 }], { duration: 200, easing: ease })
  }, [pinned])
  /** Abre já ("···", atalhos da barra): solto, por cima; fixo, já está à mostra. */
  const show = () => {
    if (pinned) return
    peek.show()
  }
  // Funções estáveis: a conversa (memo) não renderiza de novo a cada mudança do painel.
  const latest = useRef({ togglePin, show })
  latest.current = { togglePin, show }
  const actions = useMemo(() => ({
    togglePin: () => latest.current.togglePin(),
    show: () => latest.current.show(),
    dismiss: () => peek.dismiss(),
    /** WhatsApp R5c: tira o painel de vista (tela grande) ou o devolve. */
    setAway: (v: boolean) => { if (v) peek.dismiss(); setAway(v) },
    setVeil,
    /** Fixa já, sem a animação de largura (o Recolher anima a própria moldura). */
    pinNow: () => { peek.dismiss(); setPinned(true) },
  }), [peek, setPinned])
  return { pinned, open: pinned || peekOpen, peekOpen: !pinned && peekOpen, entering, ref, peek, away, veil, roomy, ...actions }
}

export function PainelDireito({ panel, attention, label, children }: {
  panel: RightPanel
  /** Alguém pede você: a alça fica âmbar. */
  attention: boolean
  label: string
  children: ReactNode
}) {
  const { pinned, open, peekOpen, entering, peek, ref, togglePin, dismiss, away, veil } = panel
  // O conteúdo monta uma vez, logo que a conversa abre, em tempo ocioso e em transição (o React fatia o trabalho):
  // abrir, fixar e soltar só mudam o modo do mesmo nó, sem montar nada no meio da animação. Fechado, fica
  // invisível e inerte.
  const [mounted, setMounted] = useState(open)
  useEffect(() => {
    if (mounted) return
    if (open) return setMounted(true)
    const idle = window.requestIdleCallback ?? ((cb: () => void) => window.setTimeout(cb, 200))
    const cancel = window.cancelIdleCallback ?? window.clearTimeout
    const id = idle(() => startTransition(() => setMounted(true)), { timeout: 1500 })
    return () => cancel(id)
  }, [open, mounted])
  useEscape(peekOpen, dismiss, ref)
  // Clique fora fecha o painel solto (o "···" das abas abre e não conta como fora).
  useEffect(() => {
    if (!peekOpen) return
    const onDown = (e: PointerEvent) => {
      const target = e.target as Element | null
      if (!target?.closest('.rpanel, .rp-edge, .ctab-more')) dismiss()
    }
    document.addEventListener('pointerdown', onDown, true)
    return () => document.removeEventListener('pointerdown', onDown, true)
  }, [peekOpen, dismiss])

  const editing = () => {
    const a = document.activeElement
    return !!a && !!ref.current?.contains(a) && a.matches('input, textarea, select, [contenteditable="true"]')
  }
  return (
    <>
      {!pinned ? (
        <div className={`rp-edge${attention ? ' need' : ''}${peekOpen ? ' covered' : ''}${away ? ' away' : ''}`} aria-hidden
          // Arrastando (seleção de texto, janela), a faixa não abre; a roda sobre ela rola a conversa de baixo.
          onMouseEnter={(e) => { if (e.buttons === 0) peek.edgeEnter() }} onMouseLeave={() => { if (!ref.current?.matches(':hover')) peek.edgeLeave() }} onClick={togglePin}
          onWheel={(e) => e.currentTarget.parentElement?.querySelector('.transcript')?.scrollBy({ top: e.deltaY, left: 0 })}>
          <span className="handle" />
        </div>
      ) : null}
      <aside ref={ref} className={`rpanel${pinned ? ' pinned' : ''}${peekOpen ? ' peek' : ''}${entering ? ' enter' : ''}${away ? ' away' : ''}${veil ? ' veil' : ''}`}
        role="complementary" aria-label={label} inert={!open} style={{ ['--rp-width' as string]: `${PANEL_WIDTH}px` }}
        onMouseEnter={() => peek.panelEnter()} onMouseLeave={() => peek.panelLeave(editing())}>
        {mounted ? <div className="rp-inner">{children}</div> : null}
      </aside>
    </>
  )
}

import { sessionMenuKey } from '../sessoes/menu'
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import type { Bot } from '@shared/models'
import { BotAvatar } from '../../../components/Avatar'
import { Icon } from '../../../components/Icon'
import { usePref } from '../../../lib/prefs'
import { reduceMotion } from '../../../lib/theme'
import { LDot } from '../sessoes/LDot'
import { SESSION_LABEL, sessionTitle } from '../sessoes/estado'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { ChatSessions } from '../sessoes/useChatSessions'
import { panelPrefs } from '../painel-direito/prefs'
import { doubleClicks } from './memoria'
import { tabsStartExpanded } from './inicio'
import { fitTabs, type TabFit } from './encaixe'
import { clickIntent, redirectMs, staggerDelays, type ItemKind } from './movimento'
import './chat-tabs.css'

// O que cada aba de sessão soma além dela: o traço de 1 px e os dois espaços de 2 px; o "···" com a margem.
const TAB_GAP = 5
const MORE_WIDTH = 32
const SEP_WIDTH = 19.5
const ALL: TabFit = { shown: [], cut: null, hidden: 0 }
// A largura da aba cortada fica com o flex; o encaixe só muda o que entra ou sai.
const sameFit = (a: TabFit, b: TabFit) => a.hidden === b.hidden && a.cut?.index === b.cut?.index && a.shown.join() === b.shown.join()
// Curvas e durações do protótipo: largura 320 ms; abas entrando 280 ms, saindo 140/160 ms.
const EASE = 'cubic-bezier(0.22, 1, 0.36, 1)'
const WIDTH_MS = 320
// Os filhos que entram e saem: tudo menos a aba do bot, o "+" colado nela (D527), o ‹‹/›› e o rascunho de medida.
const moving = (bar: HTMLElement) => [...bar.children].filter((c) => !c.matches('.ctab.main, .ctab-add, .ctab-toggle, .ctabs-measure')) as HTMLElement[]
// D527: o que a aba do bot ocupa com o "+" colado nela (espaço e margem incluídos), medido. Do lado que for (em árabe,
// o "+" fica à esquerda do chip); o "+" escondido (barra flutuando por cima, chat-tabs.css) não conta.
export const mainWidth = (bar: Pick<HTMLElement, 'querySelector'>) => {
  const main = bar.querySelector<HTMLElement>('.ctab.main')
  const add = bar.querySelector<HTMLElement>('.ctab-add')
  if (!main) return 0
  if (!add?.getClientRects().length) return main.offsetWidth
  const m = main.getBoundingClientRect()
  const a = add.getBoundingClientRect()
  return Math.max(m.right, a.right) - Math.min(m.left, a.left)
}
const kindOf = (c: Element): ItemKind => (c.matches('.ctab') ? 'tab' : c.matches('.tsep, .csep') ? 'sep' : 'tail')
const stopAll = (els: HTMLElement[]) => els.forEach((c) => c.getAnimations().forEach((a) => a.cancel()))

/**
 * Barra05 (passo 14): o topo do chat é um cartão de abas. A 1ª é o bot (ou o canal), a conversa principal;
 * depois as sessões abertas, compactas, com o ponto de estado. As que não cabem vão para "···" (abre o painel
 * em Sessões). ‹‹/›› ou dois cliques na aba do bot recolhem as sessões até a pílula do bot.
 */
export function ChatTabs({ chat, channel, sessions, openId, closed, onOpen, onCloseTab, onTogglePin, onMore, onNew, onTabMenu = null, menuSessionId = null, hidden = false }: {
  chat: Bot
  channel: boolean
  sessions: ChatSessions
  openId: string | null
  closed: ReadonlySet<string>
  onOpen: (sessionId: string | null) => void
  onCloseTab: (sessionId: string) => void
  onTogglePin: () => void
  onMore: () => void
  /** "+": só quando o Hub anuncia `create` e a conversa tem extras (views/fork/sessoes/ganchos.tsx `useNewSession`). */
  onNew: { run: (onFail?: () => void) => void; busy: boolean } | null
  /** D545: menu de cópia/arquivamento da sessão do catálogo. */
  onTabMenu?: ((sessionId: string, x: number, y: number, title?: string) => void) | null
  menuSessionId?: string | null
  hidden?: boolean
}) {
  const [saved, setExpanded] = usePref(panelPrefs.tabsOpen)
  // As sessões com aba: as em andamento sem "×", e a aberta agora mesmo se a aba dela foi fechada.
  const tabs = useMemo(() => {
    const list = sessions.open.filter((s) => !closed.has(s.id) || s.id === openId)
    const opened = openId ? sessions.archived.find((s) => s.id === openId) : undefined
    return opened ? [...list, opened] : list
  }, [sessions.open, sessions.archived, closed, openId])
  const expanded = tabsStartExpanded(tabs.length, saved)
  const bar = useRef<HTMLDivElement>(null)
  const measure = useRef<HTMLDivElement>(null)
  const twice = useMemo(() => doubleClicks(), [])
  // A intenção (mostrar ou ocultar) muda já no clique; a pref segue quando a animação acaba (A352-3).
  const intent = useRef(expanded)
  const phase = useRef<'in' | 'out' | null>(null)
  const widthAnim = useRef<Animation | null>(null)
  const pendingIn = useRef<number | null>(null)
  if (phase.current === null) intent.current = expanded
  const name = channel ? chat.name.toLocaleLowerCase('pt-BR') : chat.name

  const active = tabs.findIndex((s) => s.id === openId)
  const visible = expanded ? tabs : tabs.filter((s) => s.id === openId)
  const [fit, setFit] = useState<TabFit>(ALL)
  const t = useT()
  const idioma = useIdioma()
  // D527: criar falhou: o "+" (desabilitado enquanto criava, perdeu o foco) o recebe de volta quando volta a valer,
  // se o foco ficou perdido (no body). No sucesso, o foco vai para a caixa da sessão (sessoes/ganchos `useOpenFocused`).
  const addRef = useRef<HTMLButtonElement>(null)
  const [refocus, setRefocus] = useState(false)
  useEffect(() => {
    if (!refocus || onNew?.busy) return
    setRefocus(false)
    const active = document.activeElement
    if (!active || active === document.body) addRef.current?.focus()
  }, [refocus, onNew?.busy])
  const titles = visible.map((s) => sessionTitle(s, t)).join('\n')

  // Encaixe por medição. O espaço vem do contêiner do cartão (não da largura do cartão, que anima ao recolher),
  // então a barra esquerda abrindo ou fechando muda o encaixe só quando uma aba entra ou sai: a aba cortada
  // encolhe sozinha pelo flex. Durante a animação do próprio cartão o encaixe fica parado e refaz no fim.
  const refit = useRef<() => void>(() => {})
  useLayoutEffect(() => {
    const el = bar.current
    const draft = measure.current
    const host = el?.parentElement
    if (!el || !draft || !host) return
    const run = () => {
      if (phase.current) return
      if (!expanded) return setFit((f) => (f === ALL ? f : ALL))
      const hs = getComputedStyle(host)
      const bs = getComputedStyle(el)
      const width = host.clientWidth - parseFloat(hs.paddingLeft) - parseFloat(hs.paddingRight)
      const room = width - parseFloat(bs.paddingLeft) - parseFloat(bs.paddingRight) - mainWidth(el) - SEP_WIDTH
      const widths = [...draft.children].map((c) => (c as HTMLElement).getBoundingClientRect().width)
      const next = fitTabs(widths, room, active, { gap: TAB_GAP, more: MORE_WIDTH })
      setFit((f) => (sameFit(f, next) ? f : next))
    }
    refit.current = run
    run()
    const observer = new ResizeObserver(run)
    observer.observe(host)
    // D527: o "+" sumindo ou voltando (peek) muda o espaço das abas sem mudar o contêiner.
    const add = el.querySelector('.ctab-add')
    if (add) observer.observe(add)
    return () => observer.disconnect()
  }, [expanded, titles, active, !!onNew])

  // Um encaixe de antes da mudança das abas vale só até o efeito acima medir de novo (antes de pintar).
  const shown = (expanded && fit !== ALL ? fit.shown : visible.map((_, i) => i)).filter((i) => i < visible.length)
  const overflow = expanded && fit.hidden > 0

  const innerOf = (host: HTMLElement) => {
    const hs = getComputedStyle(host)
    const r = host.getBoundingClientRect()
    return { left: r.left + parseFloat(hs.paddingLeft), inner: r.width - parseFloat(hs.paddingLeft) - parseFloat(hs.paddingRight) }
  }
  const pillWidth = (el: HTMLElement) => {
    const style = getComputedStyle(el)
    const on = el.querySelector<HTMLElement>('.ctab.on:not(.main)')
    return parseFloat(style.paddingLeft) + parseFloat(style.paddingRight) + mainWidth(el) + (on ? SEP_WIDTH + on.offsetWidth : 0)
  }
  // Uma animação de largura por vez; a nova sempre parte da largura visível agora (que inclui a anterior pela metade).
  const animateWidth = (el: HTMLElement, from: number, to: number, duration: number) => {
    widthAnim.current?.cancel()
    el.style.willChange = 'width'
    const a = el.animate([{ width: `${from}px` }, { width: `${to}px` }], { duration, easing: EASE, fill: 'forwards' })
    widthAnim.current = a
    return a
  }
  const settle = (el: HTMLElement) => {
    widthAnim.current?.cancel()
    widthAnim.current = null
    el.style.willChange = ''
    phase.current = null
  }

  // Mostrar: o cartão cresce da largura atual até a barra cheia; as abas entram da esquerda, 26 ms entre abas.
  // Redirecionado (era ocultar), as abas voltam do ponto em que estão, sem recomeçar a sequência.
  const startIn = (el: HTMLElement, from: number, fresh: boolean) => {
    phase.current = 'in'
    widthAnim.current?.cancel()
    const to = el.getBoundingClientRect().width
    const a = animateWidth(el, from, to, redirectMs(from, to, to - pillWidth(el), WIDTH_MS))
    a.onfinish = () => {
      if (widthAnim.current !== a) return
      // Se o contêiner cresceu aos poucos no meio, a barra cheia agora é maior que o destino: completa sem salto.
      const reached = el.getBoundingClientRect().width
      settle(el)
      const full = el.getBoundingClientRect().width
      if (full - reached > 1) el.animate([{ width: `${reached}px` }, { width: `${full}px` }], { duration: 160, easing: EASE })
      refit.current()
    }
    const items = moving(el)
    if (fresh) {
      const delays = staggerDelays(items.map(kindOf))
      items.forEach((c, i) => c.animate([{ opacity: 0, transform: 'translateX(-16px)' }, { opacity: 1, transform: 'none' }], { duration: 280, delay: delays[i], easing: EASE, fill: 'backwards' }))
    } else {
      const now = items.map((c) => { const cs = getComputedStyle(c); return { opacity: cs.opacity, transform: cs.transform } })
      stopAll(items)
      items.forEach((c, i) => c.animate([now[i]!, { opacity: 1, transform: 'none' }], { duration: 200, easing: EASE }))
    }
  }

  useLayoutEffect(() => {
    const el = bar.current
    if (!el) return
    if (expanded && pendingIn.current !== null) {
      const from = pendingIn.current
      pendingIn.current = null
      startIn(el, from, true)
    } else if (!expanded) {
      // Já na pílula: solta o que ficou preso no fim da saída (a aba aberta continua no mesmo nó).
      stopAll(moving(el))
      settle(el)
    }
  }, [expanded])

  const toggle = () => {
    const el = bar.current
    const next = clickIntent(intent.current)
    intent.current = next
    if (!el || reduceMotion()) {
      if (el) { stopAll(moving(el)); settle(el) }
      return setExpanded(next)
    }
    const from = el.getBoundingClientRect().width
    if (next) {
      // Da pílula, primeiro desenha as abas; no meio de ocultar, elas ainda estão lá: redireciona já.
      if (!expanded) {
        phase.current = 'in'
        pendingIn.current = from
        return setExpanded(true)
      }
      return startIn(el, from, false)
    }
    // Ocultar: as abas saem 16 px para a esquerda (140/160 ms) e o cartão encolhe até a pílula (320 ms).
    phase.current = 'out'
    const to = pillWidth(el)
    const host = el.parentElement
    const span = (host ? innerOf(host).inner : from) - to
    const a = animateWidth(el, from, to, redirectMs(from, to, span, WIDTH_MS))
    const items = moving(el)
    const now = items.map((c) => { const cs = getComputedStyle(c); return { opacity: cs.opacity, transform: cs.transform } })
    stopAll(items)
    items.forEach((c, i) => c.animate([now[i]!, { opacity: 0, transform: 'translateX(-16px)' }], { duration: 160, easing: 'ease-in', fill: 'forwards' }))
    a.onfinish = () => {
      if (widthAnim.current !== a || intent.current) return
      setExpanded(false)
    }
  }

  // ⌘1 = a conversa principal; ⌘2…⌘9 = as abas na ordem da barra.
  const order = shown.map((i) => visible[i]!.id)
  const orderRef = useRef(order)
  orderRef.current = order
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey || e.shiftKey || !/^[1-9]$/.test(e.key) || e.defaultPrevented) return
      // Com outra tela por cima (Rotinas, Ajustes), a conversa fica inerte e não troca de aba.
      if (bar.current?.closest('[inert]')) return
      const n = Number(e.key)
      const target = n === 1 ? null : orderRef.current[n - 2]
      if (n > 1 && target === undefined) return
      e.preventDefault()
      onOpen(target ?? null)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onOpen])

  const tabKey = (id: string) => (e: ReactKeyboardEvent) => {
    if (onTabMenu && sessionMenuKey(e, () => { const r = e.currentTarget.getBoundingClientRect(); onTabMenu(id, r.left, r.bottom, sessionTitle(visible.find((s) => s.id === id)!, t)) })) return
    if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault()
      onCloseTab(id)
    }
  }

  return (
    // DON-903: textos traduzidos (dicas e nomes acessíveis) com `lang` próprio; a direção do cromo da janela continua
    // LTR (semáforos do macOS à esquerda) até o lote que espelhar o topo inteiro.
    <div ref={bar} className={`ctabs${expanded ? '' : ' collapsed'}${hidden ? ' hidden-by-call' : ''}`} role="tablist" aria-label={t('sessions.tabs.label', { nome: name })} inert={hidden} lang={idioma}>
      <button className={`ctab main${openId === null ? ' on' : ''}`} role="tab" aria-selected={openId === null}
        title={channel ? t('sessions.tabs.mainChannel', { nome: name, atalho: '⌘1' }) : t('sessions.tabs.main', { nome: name, estado: t(SESSION_LABEL[sessions.combined]), atalho: '⌘1' })}
        aria-haspopup={onTabMenu ? 'menu' : undefined}
        aria-expanded={onTabMenu ? menuSessionId === (sessions.principal?.id ?? chat.id) : undefined}
        onContextMenu={onTabMenu ? (e) => { e.preventDefault(); e.currentTarget.focus(); onTabMenu(sessions.principal?.id ?? chat.id, e.clientX, e.clientY, chat.name) } : undefined}
        onKeyDown={(e) => { if (onTabMenu) sessionMenuKey(e, () => { const r = e.currentTarget.getBoundingClientRect(); onTabMenu(sessions.principal?.id ?? chat.id, r.left, r.bottom, chat.name) }) }}
        onClick={() => {
          onOpen(null)
          if (twice('main') && tabs.length) toggle()
        }}>
        {channel ? <span className="ch-hash" aria-hidden><Icon name="number" size={12} weight="semibold" /></span> : <BotAvatar bot={chat} size={24} />}
        <span className="tn">{name}</span>
      </button>
      {/* D527: o "+" fica colado à aba do bot, com as sessões mostradas ou recolhidas; é o mesmo `onNew` de antes. */}
      {onNew ? (
        <button ref={addRef} className="ctab-add" title={t('sessions.new.title', { nome: name })} aria-label={t(onNew.busy ? 'sessions.new.busy' : 'sessions.new.label')} disabled={onNew.busy} aria-busy={onNew.busy || undefined} onClick={() => onNew.run(() => setRefocus(true))}>
          <Icon name="plus" size={16} weight="medium" />
        </button>
      ) : null}
      {shown.length ? <span className="csep" aria-hidden /> : null}
      {shown.map((i, k) => {
        const s = visible[i]!
        const on = s.id === openId
        const state = sessions.state(s.id)
        return [
          k > 0 ? <span key={`sep-${s.id}`} className="tsep" aria-hidden /> : null,
          <button key={s.id} className={`ctab${on ? ' on' : ''}`} role="tab" aria-selected={on} data-session={s.id}
            data-cut={fit.cut?.index === i ? '' : undefined}
            title={k < 8
              ? t('sessions.tabs.tabShortcut', { titulo: sessionTitle(s, t), estado: t(SESSION_LABEL[state]), atalho: `⌘${k + 2}` })
              : t('sessions.tabs.tab', { titulo: sessionTitle(s, t), estado: t(SESSION_LABEL[state]) })}
            aria-label={t('sessions.tabs.tabLabel', { titulo: sessionTitle(s, t), estado: t(SESSION_LABEL[state]) })}
            aria-haspopup={onTabMenu ? 'menu' : undefined}
            aria-expanded={onTabMenu ? menuSessionId === s.id : undefined}
            onKeyDown={tabKey(s.id)}
            onContextMenu={onTabMenu ? (e) => { e.preventDefault(); e.currentTarget.focus(); onTabMenu(s.id, e.clientX, e.clientY, sessionTitle(s, t)) } : undefined}
            onClick={() => {
              onOpen(s.id)
              if (twice(s.id)) onTogglePin()
            }}>
            <LDot state={state} />
            <span className="tn">{sessionTitle(s, t)}</span>
            <span className="tx" title={t('sessions.tabs.closeTab')} aria-hidden
              onClick={(e) => {
                e.stopPropagation()
                onCloseTab(s.id)
              }}>
              <Icon name="xmark" size={9} weight="semibold" scaled={false} />
            </span>
          </button>,
        ]
      })}
      {overflow ? (
        <button className="ctab-more" title={t('sessions.tabs.allTitle', { nome: name, n: sessions.open.length })} aria-label={t('sessions.tabs.all')} onClick={onMore}>
          <Icon name="ellipsis" size={13} weight="semibold" scaled={false} />
        </button>
      ) : null}
      <button className="ctab-toggle" aria-expanded={expanded} onClick={toggle}
        title={t(expanded ? 'sessions.tabs.hideTitle' : 'sessions.tabs.showTitle', { nome: name })} aria-label={t(expanded ? 'sessions.tabs.hide' : 'sessions.tabs.show')}>
        <Icon name={expanded ? 'chevron.left.2' : 'chevron.right.2'} size={13} weight="semibold" scaled={false} />
      </button>
      {/* Rascunho escondido com as larguras naturais das abas (a ativa reserva o lugar do "×"). */}
      <div className="ctabs-measure" ref={measure} aria-hidden inert>
        {expanded ? visible.map((s) => (
          <span key={s.id} className={`ctab${s.id === openId ? ' on' : ''}`}><LDot state="idle" /><span className="tn">{sessionTitle(s, t)}</span></span>
        )) : null}
      </div>
    </div>
  )
}

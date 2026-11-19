import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent, type RefObject } from 'react'
import { reduceMotion } from '../../lib/theme'
import { messagePreview } from '../../lib/message-preview'
import { useT } from '../../lib/fork-i18n/estado'
import { hoverScale, VIEW_SCALE, timelineFocusIndex, timelineMarks, timelineTicks, type TimelineMessage } from './chat-timeline'
import './chat-timeline.css'

// D165 fase E (components/ChatTimeline) e D432 (07/11, variante B provisória; ponto único em chat-timeline.ts):
// marcas na margem esquerda, uma por mensagem (a sua mais longa), as da tela realçadas, escala no hover, clique e
// arraste para pular. Não lidas em `accent`, do mesmo conjunto da linha "Novas" (leitura.ts). Fora da ordem do Tab
// (auditoria D249, F3): um clique foca o grupo; ↑/↓, Home/End andam; Enter/Espaço pulam.

export function ChatTimeline({ messages, visible, scroller, onJump }: {
  messages: TimelineMessage[]
  /** As linhas à vista (visible-rows.ts). */
  visible: ReadonlySet<string>
  scroller: RefObject<HTMLDivElement | null>
  /** Leaving the newest: the transcript stops following before it scrolls. */
  onJump: () => void
}) {
  const t = useT()
  const nav = useRef<HTMLElement>(null)
  const track = useRef<HTMLDivElement>(null)
  const [height, setHeight] = useState(0)
  const [focus, setFocus] = useState<string | null>(null)
  const [tip, setTip] = useState<{ index: number; top: number } | null>(null)
  const focusedButton = useRef<HTMLButtonElement | null>(null)
  const drag = useRef<{ y: number; moved: boolean } | null>(null)
  const dragged = useRef(false)

  useEffect(() => {
    const el = track.current?.parentElement
    if (!el) return
    const observer = new ResizeObserver(([e]) => setHeight(e!.contentRect.height))
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  const { marks, owner } = useMemo(() => timelineMarks(messages), [messages])
  const ticks = timelineTicks(marks, height)
  const inView = useMemo(() => new Set([...visible].map((id) => owner.get(id)).filter((id): id is string => !!id)), [visible, owner])
  const firstInView = ticks.items.find((tick) => tick.ids.some((id) => inView.has(id)))?.ids[0] ?? null
  const roving = timelineFocusIndex(ticks.items, focus, firstInView)
  // A posição atual (como no T3): uma marca só, a da primeira mensagem à vista.
  const current = timelineFocusIndex(ticks.items, null, firstInView)
  useLayoutEffect(() => {
    const previous = focusedButton.current
    if (!previous) return
    // Removing a focused button leaves BODY active. An explicit blur relinquishes ownership;
    // a different active element must never lose focus to regrouping.
    const active = document.activeElement
    const owned = previous.isConnected ? active === previous : active === document.body
    if (!owned || ticks.items.length < 2) {
      focusedButton.current = null
      setFocus(null)
      return
    }
    const tick = ticks.items[roving]!
    if (!tick.ids.includes(focus ?? '')) setFocus(tick.ids[0]!)
    const next = track.current?.querySelectorAll<HTMLButtonElement>('.chat-tick')[roving]
    if (next && next !== active) next.focus({ preventScroll: true })
  }, [ticks.items, roving, focus])

  // The track stays mounted (empty) so its height is measured before the second message arrives.
  if (ticks.items.length < 2) return <nav ref={nav} className="chat-timeline" aria-hidden><div ref={track} className="chat-timeline-track" /></nav>

  const buttons = () => [...(track.current?.querySelectorAll<HTMLButtonElement>('.chat-tick') ?? [])]
  const jump = (id: string, smooth: boolean) => {
    const container = scroller.current
    const row = container?.querySelector<HTMLElement>(`[data-chat-item="${CSS.escape(id)}"]`)
    if (!container || !row) return
    onJump()
    container.scrollTo({
      top: container.scrollTop + row.getBoundingClientRect().top - container.getBoundingClientRect().top - (container.clientHeight - row.offsetHeight) / 2,
      behavior: smooth && !reduceMotion() ? 'smooth' : 'auto',
    })
  }
  const show = (index: number) => {
    const el = buttons()[index]
    if (el) setTip({ index, top: el.offsetTop + el.offsetHeight / 2 })
  }
  // Arrastar pelo trilho: a marca mais perto do ponteiro, sem animação a cada passo.
  const nearest = (y: number) => {
    let best = 0
    let distance = Infinity
    buttons().forEach((b, i) => {
      const r = b.getBoundingClientRect()
      const d = Math.abs(r.top + r.height / 2 - y)
      if (d < distance) { distance = d; best = i }
    })
    return best
  }
  const onPointerDown = (e: PointerEvent) => { if (e.button === 0) drag.current = { y: e.clientY, moved: false } }
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current
    if (!d) return
    if (!d.moved && Math.abs(e.clientY - d.y) < 4) return
    if (!d.moved) { d.moved = true; e.currentTarget.setPointerCapture(e.pointerId) }
    const i = nearest(e.clientY)
    show(i)
    jump(ticks.items[i]!.ids[0]!, false)
  }
  const onPointerUp = () => {
    dragged.current = !!drag.current?.moved
    drag.current = null
  }
  const onKey = (e: KeyboardEvent) => {
    const last = ticks.items.length - 1
    const next = e.key === 'ArrowDown' ? Math.min(roving + 1, last) : e.key === 'ArrowUp' ? Math.max(roving - 1, 0) : e.key === 'Home' ? 0 : e.key === 'End' ? last : null
    if (next === null) return
    e.preventDefault()
    buttons()[next]?.focus({ preventScroll: true })
  }
  const active = tip ? ticks.items[tip.index]?.messages[0] : undefined

  return (
    <nav ref={nav} className="chat-timeline" aria-label={t('conversa.navegacao.rotulo')}>
      <div ref={track} className="chat-timeline-track" style={{ gap: ticks.gap, '--chat-tick-gap': `${ticks.gap}px` } as CSSProperties}
        onKeyDown={onKey} onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} onPointerCancel={onPointerUp}
        onPointerLeave={() => { if (!drag.current) setTip((s) => (s && focus ? s : null)) }}>
        {ticks.items.map((tick, i) => {
          const first = tick.messages[0]!
          const texto = messagePreview(first.text) || t('conversa.navegacao.semTexto')
          const label = tick.messages.length > 1 ? t('conversa.navegacao.irGrupo', { hora: first.time, n: tick.messages.length, texto }) : t('conversa.navegacao.ir', { hora: first.time, texto })
          const view = tick.ids.some((id) => inView.has(id))
          return (
            <button
              key={first.id}
              className={`chat-tick${view ? ' is-view' : ''}${i === current && view ? ' is-current' : ''}${tip?.index === i ? ' is-hover' : ''}${tick.unread ? ' is-unread' : ''}${first.mine ? ' is-mine' : ' is-bot'}`}
              style={{ '--k': Math.max(tip ? hoverScale(Math.abs(i - tip.index)) : 1, i === current && view ? VIEW_SCALE : 1) } as CSSProperties}
              tabIndex={-1}
              aria-label={label}
              aria-current={i === roving && view ? true : undefined}
              onPointerEnter={() => { if (!drag.current) show(i) }}
              onFocus={(e) => {
                focusedButton.current = e.currentTarget
                setFocus((anchor) => tick.ids.includes(anchor ?? '') ? anchor : first.id)
                show(i)
              }}
              onBlur={(e) => {
                if (!e.currentTarget.parentElement?.contains(e.relatedTarget as Node | null)) {
                  focusedButton.current = null
                  setFocus(null)
                  setTip(null)
                }
              }}
              onClick={() => {
                if (dragged.current) { dragged.current = false; return }
                jump(first.id, true)
              }}
            />
          )
        })}
      </div>
      {active && tip ? (
        <div className="chat-tick-tip" style={{ top: tip.top }} aria-hidden>
          <span className="q"><time>{active.time}</time>{messagePreview(active.text)}</span>
          {active.mine && active.answer ? <span className="ans">{active.answer.split('\n').map(messagePreview).filter(Boolean).join(' ')}</span> : null}
        </div>
      ) : null}
    </nav>
  )
}

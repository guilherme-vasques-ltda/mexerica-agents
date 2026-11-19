import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent, type WheelEvent as ReactWheelEvent } from 'react'
import type { TimesBot, TimesGraph } from '@shared/fork-times'
import { Icon } from '../../../components/Icon'
import { clampZoom, fitView, port, revealPan, ZOOM, type BoardLayout, type CardBox, type FrameBox, type Point } from './board-layout'
import type { BoardPrefs } from './board-prefs'
import { ancestorsOf, type treeIndex } from './structure'
import { areaPath, OwnerFace, TimesFace, useTimesTextos } from './parts'
import { frase, plural } from '../../../lib/fork-i18n/frase'
import { ltr } from '@shared/bidi'

// O Quadro (components/TimesView): cartões com o rosto oficial, áreas em contorno, cotovelos de estrutura, curvas
// de Reporta a e rotas propostas tracejadas, em SVG e DOM próprios. Três gestos, três efeitos: arrastar o cartão só
// muda a disposição (neste aparelho); arrastar a porta pede um Reporta a novo ao Hub; Mover fica no menu. Travar
// impede arrastar; o teclado e a Lista continuam editando. D454 (fatia 9c): textos no idioma da tela; nomes de bot e
// de área na direção deles; o quadro em si (coordenadas, curvas) não se espelha.

export type BoardAction = 'reports' | 'move' | 'menu'

export interface BoardProps {
  layout: BoardLayout
  index: ReturnType<typeof treeIndex>
  graph: TimesGraph
  bots: ReadonlyMap<string, TimesBot>
  primaryBotId: string | null
  prefs: BoardPrefs
  setPrefs: (next: BoardPrefs) => void
  selected: string | null
  onSelect: (key: string) => void
  /** Pode pedir mudanças ao Hub agora (link vivo, nada em curso). */
  editable: boolean
  /** Reportes que mudaram no rascunho (ainda não salvos). */
  drafted: ReadonlySet<string>
  /** A ligação pedida pela porta, enquanto o Hub confere. */
  ghost: { from: string; to: string } | null
  onReport: (botId: string, to: string) => void
  onAction: (action: BoardAction, key: string, invoker: HTMLElement) => void
  onToggle: (key: string) => void
}

type Drag =
  | { kind: 'pan'; start: Point; pan: Point; moved: boolean }
  | { kind: 'card'; key: string; start: Point; origin: Point; moved: boolean; element: HTMLElement }
  | { kind: 'port'; botId: string; start: Point }

const MOVE_SLOP = 4

export function TimesBoard(p: BoardProps) {
  const viewport = useRef<HTMLDivElement>(null)
  const [size, setSize] = useState({ w: 0, h: 0 })
  const [view, setView] = useState<{ zoom: number; pan: Point } | null>(() => (p.prefs.zoom !== null && p.prefs.pan ? { zoom: clampZoom(p.prefs.zoom), pan: p.prefs.pan } : null))
  const drag = useRef<Drag | null>(null)
  const [pointer, setPointer] = useState<Point | null>(null)
  const [target, setTarget] = useState<string | null>(null)

  useLayoutEffect(() => {
    const el = viewport.current
    if (!el) return
    const observer = new ResizeObserver(([entry]) => entry && setSize({ w: entry.contentRect.width, h: entry.contentRect.height }))
    observer.observe(el)
    return () => observer.disconnect()
  }, [])
  // Na primeira vez (sem preferência guardada), cabe tudo na tela.
  useEffect(() => {
    if (!view && size.w > 0 && size.h > 0) setView(fitView(p.layout, size))
  }, [view, size, p.layout])

  const current = view ?? { zoom: 1, pan: { x: 0, y: 0 } }
  const keep = (next: { zoom: number; pan: Point }) => {
    setView(next)
    p.setPrefs({ ...p.prefs, zoom: next.zoom, pan: next.pan })
  }
  const zoomAt = (zoom: number, at: Point = { x: size.w / 2, y: size.h / 2 }) => {
    const z = clampZoom(zoom)
    const world = { x: (at.x - current.pan.x) / current.zoom, y: (at.y - current.pan.y) / current.zoom }
    keep({ zoom: z, pan: { x: at.x - world.x * z, y: at.y - world.y * z } })
  }
  const fit = () => keep(fitView(p.layout, size))
  const locked = p.prefs.locked

  // Mantém à vista o item escolhido (depois de Mover para dentro de algo recolhido, por exemplo).
  useEffect(() => {
    if (!p.selected || !view || !size.w) return
    const box = p.layout.cards.find((c) => c.key === p.selected) ?? p.layout.frames.find((f) => f.key === p.selected)
    if (!box) return
    const pan = revealPan(box, view.zoom, view.pan, size)
    if (pan) keep({ zoom: view.zoom, pan })
  }, [p.selected, p.layout])

  const local = (e: { clientX: number; clientY: number }): Point => {
    const rect = viewport.current!.getBoundingClientRect()
    return { x: e.clientX - rect.left, y: e.clientY - rect.top }
  }
  const world = (pt: Point): Point => ({ x: (pt.x - current.pan.x) / current.zoom, y: (pt.y - current.pan.y) / current.zoom })

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return
    const el = e.target as HTMLElement
    if (el.closest('button, .times-tools, .times-frame-head')) return
    const at = local(e)
    const portEl = el.closest<HTMLElement>('[data-times-port]')
    const cardEl = el.closest<HTMLElement>('[data-times-card]')
    if (portEl && !locked && p.editable) {
      drag.current = { kind: 'port', botId: portEl.dataset.timesPort!, start: at }
      setPointer(world(at))
    } else if (cardEl && !portEl) {
      const key = cardEl.dataset.timesCard!
      const origin = p.prefs.offsets[key] ?? { x: 0, y: 0 }
      const body = cardEl.querySelector<HTMLElement>('[data-times-nav]') ?? cardEl
      drag.current = { kind: 'card', key, start: at, origin, moved: false, element: body }
    } else if (!cardEl && !locked) {
      drag.current = { kind: 'pan', start: at, pan: current.pan, moved: false }
    } else return
    viewport.current?.setPointerCapture(e.pointerId)
  }
  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current
    if (!d) return
    const at = local(e)
    const dx = at.x - d.start.x
    const dy = at.y - d.start.y
    if (d.kind === 'pan') {
      if (!d.moved && Math.hypot(dx, dy) < MOVE_SLOP) return
      d.moved = true
      setView({ zoom: current.zoom, pan: { x: d.pan.x + dx, y: d.pan.y + dy } })
    } else if (d.kind === 'card') {
      if (locked || (!d.moved && Math.hypot(dx, dy) < MOVE_SLOP)) return
      d.moved = true
      p.setPrefs({ ...p.prefs, offsets: { ...p.prefs.offsets, [d.key]: { x: d.origin.x + dx / current.zoom, y: d.origin.y + dy / current.zoom } } })
    } else {
      setPointer(world(at))
      const hit = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>('[data-times-card]')?.dataset.timesCard ?? null
      setTarget(hit && hit !== d.botId ? hit : null)
    }
  }
  const onPointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    const d = drag.current
    drag.current = null
    if (viewport.current?.hasPointerCapture(e.pointerId)) viewport.current.releasePointerCapture(e.pointerId)
    if (!d) return
    if (d.kind === 'pan') {
      if (d.moved) keep({ zoom: current.zoom, pan: current.pan })
    } else if (d.kind === 'card') {
      if (!d.moved) {
        p.onSelect(d.key)
        d.element.focus({ preventScroll: true })
      }
    } else {
      const to = target
      setPointer(null)
      setTarget(null)
      if (to) p.onReport(d.botId, to)
    }
  }
  const onWheel = (e: ReactWheelEvent<HTMLDivElement>) => {
    if (e.ctrlKey || e.metaKey) {
      zoomAt(current.zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1), local(e))
    } else if (!locked) {
      setView({ zoom: current.zoom, pan: { x: current.pan.x - e.deltaX, y: current.pan.y - e.deltaY } })
    }
  }
  // A roda grava o deslocamento um pouco depois de parar, não a cada evento.
  useEffect(() => {
    if (!view) return
    const timer = setTimeout(() => {
      if (p.prefs.zoom !== view.zoom || p.prefs.pan?.x !== view.pan.x || p.prefs.pan?.y !== view.pan.y) p.setPrefs({ ...p.prefs, zoom: view.zoom, pan: view.pan })
    }, 400)
    return () => clearTimeout(timer)
  }, [view])

  // MARK: teclado (equivalente a cada gesto)

  const navKeys = p.index.order.map((i) => (i.kind === 'area' ? `area:${i.nodeId}` : i.id)).filter((k) => p.layout.cards.some((c) => c.key === k) || p.layout.frames.some((f) => f.key === k))
  const tabStop = p.selected && navKeys.includes(p.selected) ? p.selected : navKeys[0] ?? null
  const focusNav = (key: string | null | undefined) => {
    if (!key) return
    p.onSelect(key)
    viewport.current?.querySelector<HTMLElement>(`[data-times-nav="${CSS.escape(key)}"]`)?.focus({ preventScroll: true })
  }
  const onNavKey = (e: ReactKeyboardEvent<HTMLElement>, key: string) => {
    if (e.altKey || e.metaKey || e.ctrlKey) return
    const at = navKeys.indexOf(key)
    const isBot = p.graph.members.some((m) => m.botId === key)
    const handled = (() => {
      switch (e.key) {
        case 'ArrowLeft': focusNav(navKeys[at - 1]); return true
        case 'ArrowRight': focusNav(navKeys[at + 1]); return true
        case 'ArrowUp': focusNav(ancestorsOf(p.index, key).find((k) => navKeys.includes(k))); return true
        case 'ArrowDown': focusNav(navKeys.slice(at + 1).find((k) => ancestorsOf(p.index, k).includes(key))); return true
        case 'Home': focusNav(navKeys[0]); return true
        case 'End': focusNav(navKeys.at(-1)); return true
        case 'Enter': p.onSelect(key); return true
        case ' ': if (key.startsWith('area:') || key === 'loose') { p.onToggle(key); return true } p.onSelect(key); return true
        case 'r': case 'R': if (isBot && !e.shiftKey) { p.onSelect(key); p.onAction('reports', key, e.currentTarget); return true } return false
        case 'm': case 'M': if (isBot && !e.shiftKey) { p.onSelect(key); p.onAction('move', key, e.currentTarget); return true } return false
        case 'F10': if (e.shiftKey) { p.onSelect(key); p.onAction('menu', key, e.currentTarget); return true } return false
        case 'ContextMenu': p.onSelect(key); p.onAction('menu', key, e.currentTarget); return true
        default: return false
      }
    })()
    if (handled) { e.preventDefault(); e.stopPropagation() }
  }
  const onViewportKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.altKey || e.metaKey || e.ctrlKey || (e.target as HTMLElement).closest('input, textarea, select')) return
    if (e.key === '+' || e.key === '=') zoomAt(current.zoom + ZOOM.step)
    else if (e.key === '-') zoomAt(current.zoom - ZOOM.step)
    else if (e.key === '0') fit()
    else return
    e.preventDefault()
  }

  const cardOf = new Map(p.layout.cards.map((c) => [c.key, c]))
  const drawing = drag.current?.kind === 'port' && pointer ? { from: port(cardOf.get(drag.current.botId) ?? { x: 0, y: 0, w: 0 }), to: pointer } : null
  const ghost = p.ghost ? ghostLine(cardOf.get(p.ghost.from), cardOf.get(p.ghost.to)) : null
  const offsets = Object.keys(p.prefs.offsets).length > 0

  const { tx } = useTimesTextos()
  return (
    <div className="times-board" role="region" aria-label={tx(frase('times.quadro.board.rotulo'))}>
      <div className="times-tools" role="toolbar" aria-label={tx(frase('times.quadro.board.controles'))}>
        <div className="times-zoom">
          <button type="button" aria-label={tx(frase('times.quadro.board.diminuir'))} title={tx(frase('times.quadro.board.diminuirDica'))} onClick={() => zoomAt(current.zoom - ZOOM.step)} disabled={current.zoom <= ZOOM.min}>−</button>
          <span aria-live="polite" dir="ltr">{Math.round(current.zoom * 100)}%</span>
          <button type="button" aria-label={tx(frase('times.quadro.board.aumentar'))} title={tx(frase('times.quadro.board.aumentarDica'))} onClick={() => zoomAt(current.zoom + ZOOM.step)} disabled={current.zoom >= ZOOM.max}>+</button>
        </div>
        <button type="button" className="times-tool" title={tx(frase('times.quadro.board.ajustarDica'))} onClick={fit}><Icon name="arrow.up.left.and.arrow.down.right" size={12} />{tx(frase('times.quadro.board.ajustar'))}</button>
        <button type="button" className="times-tool" aria-pressed={locked} title={tx(frase('times.quadro.board.travarDica'))} onClick={() => p.setPrefs({ ...p.prefs, locked: !locked })}>{tx(frase(locked ? 'times.quadro.board.travado' : 'times.quadro.board.travar'))}</button>
        <button type="button" className="times-tool" aria-pressed={p.prefs.showRoutes} title={tx(frase('times.quadro.board.rotasDica'))} onClick={() => p.setPrefs({ ...p.prefs, showRoutes: !p.prefs.showRoutes })}>{tx(frase('times.quadro.board.rotas'))}</button>
        <button type="button" className="times-tool" disabled={!offsets} title={tx(frase('times.quadro.board.originalDica'))} onClick={() => p.setPrefs({ ...p.prefs, offsets: {} })}>{tx(frase('times.quadro.board.original'))}</button>
      </div>
      <div
        ref={viewport}
        className={`times-viewport${locked ? ' is-locked' : ''}${drawing ? ' is-linking' : ''}`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onWheel={onWheel}
        onKeyDown={onViewportKey}
      >
        <div className="times-world" style={{ transform: `translate(${current.pan.x}px, ${current.pan.y}px) scale(${current.zoom})`, width: p.layout.width, height: p.layout.height }}>
          {p.layout.frames.map((f) => <Frame key={f.key} frame={f} graph={p.graph} bots={p.bots} selected={p.selected === f.key} tabStop={tabStop === f.key} onKey={onNavKey} onSelect={p.onSelect} onToggle={p.onToggle} onMenu={(el) => p.onAction('menu', f.key, el)} />)}
          <svg className="times-lines" width={Math.max(1, p.layout.width)} height={Math.max(1, p.layout.height)} aria-hidden>
            <defs>
              <marker id="times-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0L8 4L0 8z" className="times-arrow" /></marker>
              <marker id="times-arrow-on" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0L8 4L0 8z" className="times-arrow is-on" /></marker>
              <marker id="times-arrow-draft" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0L8 4L0 8z" className="times-arrow is-draft" /></marker>
            </defs>
            {p.layout.structure.map((l) => <path key={l.key} d={l.d} className="times-line-structure" />)}
            {p.layout.routes.map((l) => (
              <g key={l.key} className={p.selected === l.from || p.selected === l.to ? 'is-on' : undefined}>
                <path d={l.d} className="times-line-route" />
                <text x={l.label.x} y={l.label.y} className="times-line-label" textAnchor="middle">{tx(frase('times.quadro.board.proposta'))}</text>
              </g>
            ))}
            {p.layout.reports.map((l) => {
              const on = p.selected === l.from
              const draft = p.drafted.has(l.from)
              return <path key={l.key} d={l.d} className={`times-line-report${on ? ' is-on' : ''}${draft ? ' is-draft' : ''}`} markerEnd={`url(#times-arrow${on ? '-on' : draft ? '-draft' : ''})`} />
            })}
            {ghost ? (
              <g className="times-line-ghost">
                <path d={ghost.d} />
                <text x={ghost.label.x} y={ghost.label.y} className="times-line-label" textAnchor="middle">{tx(frase('times.quadro.board.conferindo'))}</text>
              </g>
            ) : null}
            {drawing ? <path d={`M${drawing.from.x} ${drawing.from.y}L${drawing.to.x} ${drawing.to.y}`} className="times-line-drawing" /> : null}
          </svg>
          {p.layout.cards.map((c) => (
            <Card
              key={c.key}
              card={c}
              graph={p.graph}
              bots={p.bots}
              primaryBotId={p.primaryBotId}
              selected={p.selected === c.key}
              tabStop={tabStop === c.key}
              target={target === c.key}
              linkable={p.editable && !locked}
              onKey={onNavKey}
              onMenu={(el) => p.onAction('menu', c.key, el)}
              onToggle={() => p.onToggle(c.key)}
            />
          ))}
        </div>
      </div>
    </div>
  )
}

function ghostLine(from: CardBox | undefined, to: CardBox | undefined) {
  if (!from || !to) return null
  const a = port(from)
  const b = to.y + to.h <= a.y ? { x: to.x + to.w / 2, y: to.y + to.h } : { x: to.x + to.w / 2, y: to.y }
  return { d: `M${a.x} ${a.y}C${a.x} ${a.y - 40} ${b.x} ${b.y + (b.y <= a.y ? 40 : -40)} ${b.x} ${b.y}`, label: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 - 6 } }
}

function Card({ card, graph, bots, primaryBotId, selected, tabStop, target, linkable, onKey, onMenu, onToggle }: {
  card: CardBox
  graph: TimesGraph
  bots: ReadonlyMap<string, TimesBot>
  primaryBotId: string | null
  selected: boolean
  tabStop: boolean
  target: boolean
  linkable: boolean
  onKey: (e: ReactKeyboardEvent<HTMLElement>, key: string) => void
  onMenu: (invoker: HTMLElement) => void
  onToggle: () => void
}) {
  const menu = useRef<HTMLButtonElement>(null)
  const { T, tx, ROLE_LABEL, nameOf, reportsLabel } = useTimesTextos()
  const style = { left: card.x, top: card.y, width: card.w, height: card.h }
  const hasChildren = card.hiddenBelow > 0 || graph.members.some((m) => m.parentBotId === card.botId)
  if (card.kind === 'owner') {
    return (
      <div className="times-card times-card-owner" style={style} data-times-card="owner">
        <div className={`times-card-body${selected ? ' is-selected' : ''}${target ? ' is-target' : ''}`} role="button" tabIndex={tabStop ? 0 : -1} data-times-nav="owner" aria-label={tx(frase('times.quadro.board.voceDestino'))} onKeyDown={(e) => onKey(e, 'owner')}>
          <OwnerFace size={28} />
          <span className="times-card-name">{T.owner}</span>
        </div>
      </div>
    )
  }
  const botId = card.botId!
  const bot = bots.get(botId) ?? null
  const member = graph.members.find((m) => m.botId === botId)
  const primary = botId === primaryBotId
  const role = primary ? tx(frase('times.quadro.board.principal')) : member?.role === 'manager' ? ROLE_LABEL.manager : null
  const reports = member ? reportsLabel(bots, member) : '—'
  const area = member?.teamId ? areaPath(graph, member.teamId) : ''
  return (
    <div className={`times-card${bot?.hidden ? ' is-hidden-bot' : ''}`} style={style} data-times-card={botId}>
      {linkable ? <span className="times-port" data-times-port={botId} title={tx(frase('times.quadro.board.arraste', { nome: nameOf(bots, botId) }))} aria-hidden /> : <span className="times-port is-off" aria-hidden />}
      <div
        className={`times-card-body${selected ? ' is-selected' : ''}${target ? ' is-target' : ''}`}
        role="button"
        tabIndex={tabStop ? 0 : -1}
        data-times-nav={botId}
        aria-pressed={selected}
        aria-label={tx(frase(role && area ? 'times.quadro.board.cartaoPapelArea' : role ? 'times.quadro.board.cartaoPapel' : area ? 'times.quadro.board.cartaoArea' : 'times.quadro.board.cartao', { nome: nameOf(bots, botId), papel: role ?? '', area, alvo: reports }))}
        aria-description={tx(frase('times.quadro.board.teclas'))}
        onKeyDown={(e) => onKey(e, botId)}
      >
        <TimesFace bot={bot} size={32} />
        {/* D249: o nome tem a linha 1 inteira; selos e função vão na linha 2, e só a função encolhe. */}
        <span className="times-card-text">
          <span className="times-card-name" dir="auto" title={nameOf(bots, botId)}>{nameOf(bots, botId)}</span>
          <span className="times-card-meta">
            {role ? <span className={`times-tag${primary ? ' is-primary' : ''}`}>{role}</span> : null}
            {bot?.hidden ? <span className="times-tag">{tx(frase('times.quadro.board.oculto'))}</span> : null}
            <span className="times-card-fn" dir="auto">{bot?.title || area || ' '}</span>
          </span>
        </span>
      </div>
      <button ref={menu} type="button" className="times-card-menu" tabIndex={-1} aria-label={tx(frase('times.quadro.board.acoesDe', { nome: nameOf(bots, botId) }))} aria-haspopup="menu" onClick={() => menu.current && onMenu(menu.current)}>
        <Icon name="ellipsis" size={12} />
      </button>
      {hasChildren ? (
        <button type="button" className="times-fold" tabIndex={-1} aria-label={tx(card.hiddenBelow ? frase('times.quadro.board.mostrarAbaixo', { n: card.hiddenBelow, nome: nameOf(bots, botId) }) : frase('times.quadro.board.recolherAbaixo', { nome: nameOf(bots, botId) }))} onClick={onToggle}>
          {card.hiddenBelow ? ltr(`+${card.hiddenBelow}`) : '−'}
        </button>
      ) : null}
    </div>
  )
}

function Frame({ frame, graph, bots, selected, tabStop, onKey, onSelect, onToggle, onMenu }: {
  frame: FrameBox
  graph: TimesGraph
  bots: ReadonlyMap<string, TimesBot>
  selected: boolean
  tabStop: boolean
  onKey: (e: ReactKeyboardEvent<HTMLElement>, key: string) => void
  onSelect: (key: string) => void
  onToggle: (key: string) => void
  onMenu: (invoker: HTMLElement) => void
}) {
  const { T, tx, nameOf } = useTimesTextos()
  const node = frame.nodeId ? graph.nodes.find((n) => n.id === frame.nodeId) ?? null : null
  const name = node ? node.name : T.loose
  const manager = node?.managerBotId ? nameOf(bots, node.managerBotId) : null
  const menu = useRef<HTMLButtonElement>(null)
  return (
    <div className={`times-frame${frame.kind === 'loose' ? ' is-loose' : ''}${selected ? ' is-selected' : ''}`} style={{ left: frame.x, top: frame.y, width: frame.w, height: frame.h }}>
      <div className="times-frame-head">
        <span
          role="button"
          tabIndex={tabStop ? 0 : -1}
          data-times-nav={frame.key}
          aria-expanded={!frame.collapsed}
          aria-label={frame.kind === 'loose' ? T.loose : tx(manager ? frase('times.quadro.board.areaGerente', { nome: name, gerente: manager }) : frase('times.quadro.board.area', { nome: name }))}
          aria-description={tx(frase('times.quadro.board.espaco'))}
          className="times-frame-name"
          dir="auto"
          onClick={() => onSelect(frame.key)}
          onKeyDown={(e) => onKey(e, frame.key)}
        >
          {name}
        </span>
        <button type="button" className="times-frame-toggle" tabIndex={-1} aria-label={tx(frase(frame.collapsed ? 'times.quadro.board.mostrar' : 'times.quadro.board.recolher', { nome: name }))} onClick={() => onToggle(frame.key)}>
          <Icon name={frame.collapsed ? 'chevron.right' : 'chevron.down'} size={10} weight="semibold" />
        </button>
        {frame.kind === 'area' ? (
          <button ref={menu} type="button" className="times-frame-toggle" tabIndex={-1} aria-label={tx(frase('times.quadro.board.acoesArea', { nome: name }))} aria-haspopup="menu" onClick={() => menu.current && onMenu(menu.current)}>
            <Icon name="ellipsis" size={10} />
          </button>
        ) : null}
      </div>
      {frame.collapsed ? <button type="button" className="times-frame-more" onClick={() => onToggle(frame.key)}>{tx(plural('times.quadro.board.maisBots', frame.hiddenBelow))}</button> : null}
    </div>
  )
}

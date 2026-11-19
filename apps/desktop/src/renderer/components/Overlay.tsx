// Modificado pela DONNA Bot em 2026-11-06: coordenação modal seletiva de Codync
// 219935410dffccb950f90d0565bd1d9f1661bd5b, Apache-2.0; leepokai e Claude Opus 5.5. Ver THIRD_PARTY.md.
import { createContext, useContext, useEffect, useId, useLayoutEffect, useRef, useState, type ReactElement, type ReactNode, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { interactive, useInertExit, useLayerEscape as useEscape, useModalFocus } from '../lib/fork-modal-stack'
import { reduceMotion } from '../lib/theme'
import { Icon } from './Icon'
import { HStack, VStack } from './Stack'
import { Text } from './Text'
import './overlay.css'

export { useEscape }

/** Keeps a layer mounted while it animates away; `shown` drives the CSS transition. */
export function usePresence(open: boolean, exitMs = 300, animateOnMount = true) {
  const [mounted, setMounted] = useState(open)
  const [shown, setShown] = useState(open && !animateOnMount)
  useLayoutEffect(() => {
    if (reduceMotion()) {
      setMounted(open)
      setShown(open)
      return
    }
    if (open) {
      setMounted(true)
      let reveal = 0
      const frame = requestAnimationFrame(() => { reveal = requestAnimationFrame(() => setShown(true)) })
      return () => {
        cancelAnimationFrame(frame)
        cancelAnimationFrame(reveal)
      }
    }
    setShown(false)
    const timer = setTimeout(() => setMounted(false), reduceMotion() ? 0 : exitMs)
    return () => clearTimeout(timer)
  }, [open, exitMs])
  return { mounted, shown }
}

/** Closes the modal the view is in (kit's `\.dismissModal`). */
export const DismissContext = createContext<() => void>(() => {})
export const useDismiss = () => useContext(DismissContext)

/** The open modal's title id, so its ModalHeader names the dialog. */
const ModalTitleContext = createContext<string | undefined>(undefined)

/** The Mac modal (`.codyncSheet`): a centered card over a dim; the content sizes it. */
export function Sheet({ open, onClose, width, height, maxHeight, dismissOnDim = false, children, label, language }: {
  open: boolean
  onClose: () => void
  /** A click outside the card closes it (transient pickers, not forms). */
  dismissOnDim?: boolean
  width?: number | string
  height?: number | string
  /** Without `height` the card fits its content, up to this (and the window). */
  maxHeight?: number | string
  children: ReactNode
  label?: string
  /** D454: `lang`/`dir` do idioma escolhido na raiz do portal, quando o conteúdo já é traduzido; sem ele, o limite legado do body. */
  language?: PortalLanguage
}) {
  const { mounted, shown } = usePresence(open)
  const last = useRef(children)
  const layer = useRef<HTMLDivElement>(null)
  const titleId = useId()
  if (open) last.current = children
  useEscape(open, onClose, layer)
  useModalFocus(open, mounted, layer)
  if (!mounted) return null
  return createPortal(
    <DismissContext.Provider value={onClose}>
      <ModalTitleContext.Provider value={titleId}>
        <div ref={layer} inert={!open} className={`modal-card ${shown ? 'shown' : ''}${language ? ' idioma-superficie' : ''}`} {...language} role="dialog" aria-modal aria-label={label} aria-labelledby={label ? undefined : titleId}>
          <div className="modal-dim" onClick={dismissOnDim ? onClose : undefined} />
          <div className="modal-content" data-modal-card tabIndex={-1} style={{ width, height, maxHeight }}>
            {open ? children : last.current}
          </div>
        </div>
      </ModalTitleContext.Provider>
    </DismissContext.Provider>,
    document.body,
  )
}

export interface DialogAction {
  title: string
  destructive?: boolean
  action: () => void
}

/** A centered card over a dimmed screen with the actions and Cancel (`.codyncDialog`). */
export function Dialog({ open, title, message, actions, cancel = 'Cancelar', onClose, language }: {
  open: boolean
  title: string
  /** Texto, ou um bloco próprio (a lista dos bots que perdem uma skill, na confirmação de remover). */
  message?: ReactNode
  actions: DialogAction[]
  cancel?: string | null
  onClose: () => void
  /** D454: `lang`/`dir` do idioma escolhido na raiz do portal, quando o conteúdo já é traduzido; sem ele, o limite legado do body. */
  language?: PortalLanguage
}) {
  const { mounted, shown } = usePresence(open, 120)
  const last = useRef({ title, message, actions })
  const layer = useRef<HTMLDivElement>(null)
  const id = useId()
  if (open) last.current = { title, message, actions }
  useEscape(open && cancel !== null, onClose, layer)
  useModalFocus(open, mounted, layer)
  if (!mounted) return null
  const shownContent = last.current
  return createPortal(
    <div ref={layer} inert={!open} className={`dialog ${shown ? 'shown' : ''}${language ? ' idioma-superficie' : ''}`} {...language} role="alertdialog" aria-modal aria-labelledby={`${id}-title`} aria-describedby={shownContent.message ? `${id}-message` : undefined}>
      <div className="dialog-dim" onClick={() => cancel !== null && onClose()} />
      <div className="dialog-card" data-modal-card tabIndex={-1}>
        <div className="dialog-text">
          <Text as="h2" id={`${id}-title`} size="title3" weight="semibold">{shownContent.title}</Text>
          {shownContent.message ? <Text as="div" id={`${id}-message`} size="body" tone="secondary">{shownContent.message}</Text> : null}
        </div>
        <div className="dialog-buttons">
          {cancel !== null ? (
            <button className="dialog-button" data-autofocus onClick={onClose}>
              {cancel}
            </button>
          ) : null}
          {shownContent.actions.map((a) => (
            <button
              key={a.title}
              className={`dialog-button ${a.destructive ? 'destructive' : 'prominent'}`}
              onClick={() => {
                onClose()
                a.action()
              }}
            >
              {a.title}
            </button>
          ))}
        </div>
      </div>
    </div>,
    document.body,
  )
}

/** One row in a Codync menu: an action, or a choice when `selected` is set. */
/** `lang`/`dir` de uma superfície traduzida (o `superficieIdioma` do idioma escolhido). */
export type PortalLanguage = { lang: string; dir: 'ltr' | 'rtl' }

export interface MenuItem {
  title: string
  icon?: string
  /** A picture in the icon's place (an agent's logo). */
  leading?: ReactNode
  selected?: boolean
  /** Shown but not choosable (an account that isn't connected). */
  disabled?: boolean
  destructive?: boolean
  divider?: boolean
  /** A short reading on the trailing edge (a count). */
  detail?: string | null
  action: () => void
}

/** A row of emoji above a message's menu (Slack's quick reactions). */
export interface ReactionPick {
  emoji: string[]
  chosen: string[]
  toggle: (emoji: string) => void
}

export function ReactionStrip({ pick, dismiss, size = 26 }: { pick: ReactionPick; dismiss?: () => void; size?: number }) {
  return (
    <HStack align="stretch">
      {pick.emoji.map((emoji) => {
        const chosen = pick.chosen.includes(emoji)
        return (
          <button
            key={emoji}
            className={`reaction-button ${chosen ? 'chosen' : ''}`}
            style={{ width: size, height: size, borderRadius: size * 0.28 }}
            title={chosen ? `Remove ${emoji}` : `React ${emoji}`}
            aria-label={chosen ? `Remove reaction ${emoji}` : `React ${emoji}`}
            onClick={() => {
              dismiss?.()
              pick.toggle(emoji)
            }}
          >
            <Text size={14}>{emoji}</Text>
          </button>
        )
      })}
    </HStack>
  )
}

/** The floating panel of menu rows. */
export function MenuPanel({ items, reactions, dismiss, maxHeight, focusMenu = false, menuLabel }: { items: MenuItem[]; reactions?: ReactionPick; dismiss: () => void; maxHeight?: number; focusMenu?: boolean; menuLabel?: string }) {
  const [highlight, setHighlight] = useState(-1)
  const panel = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (focusMenu) panel.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus({ preventScroll: true })
  }, [focusMenu])
  useEffect(() => {
    if (focusMenu) return
    const onKey = (e: KeyboardEvent) => {
      if (!interactive(panel.current)) return
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault()
        setHighlight((h) => (h + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length)
      } else if (e.key === 'Enter' && highlight >= 0) {
        e.preventDefault()
        if (items[highlight]?.disabled) return
        dismiss()
        items[highlight]?.action()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [items, highlight, dismiss, focusMenu])
  return (
    <div ref={panel} className="menu-panel" role="menu" aria-label={menuLabel} style={{ maxHeight }} onKeyDown={focusMenu ? (e) => {
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End', 'Tab'].includes(e.key)) return
      e.preventDefault()
      e.stopPropagation()
      if (e.key === 'Tab') { dismiss(); return }
      const rows = [...panel.current!.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')]
      const at = rows.indexOf(document.activeElement as HTMLButtonElement)
      const next = e.key === 'Home' ? 0 : e.key === 'End' ? rows.length - 1 : (at + (e.key === 'ArrowUp' ? -1 : 1) + rows.length) % rows.length
      rows[next]?.focus()
    } : undefined}>
      {reactions ? (
        <div className={items.length ? 'menu-reactions' : undefined}>
          <ReactionStrip pick={reactions} dismiss={dismiss} />
        </div>
      ) : null}
      {items.map((item, i) => (
        <div key={`${item.title}-${i}`} role="none">
          {item.divider ? <div className="menu-divider" role="separator" /> : null}
          <button
            className={`menu-row ${item.destructive ? 'destructive' : ''} ${highlight === i ? 'highlight' : ''}`}
            role={item.selected === undefined ? 'menuitem' : 'menuitemradio'}
            aria-checked={item.selected}
            disabled={!focusMenu && item.disabled}
            aria-disabled={focusMenu && item.disabled || undefined}
            title={item.title}
            onMouseEnter={() => setHighlight(i)}
            onClick={() => {
              if (item.disabled) return
              dismiss()
              item.action()
            }}
          >
            {item.leading || item.icon ? (
              <span className="menu-icon">
                {item.leading ?? (item.icon ? <Icon name={item.icon} size={15} /> : null)}
              </span>
            ) : null}
            <span className="menu-title">{item.title}</span>
            {item.detail ? <span className="menu-detail">{item.detail}</span> : null}
            {item.selected !== undefined ? (
              <span className={`check-slot${item.selected ? ' on' : ''}`}>
                <Icon name="checkmark" size={10} weight="semibold" />
              </span>
            ) : null}
          </button>
        </div>
      ))}
    </div>
  )
}

/**
 * Where a panel opens beside `rect`: below it or above (whichever has more room), aligned to the
 * anchor's leading or trailing edge and kept on screen. `room` is the height available.
 */
export function anchorPlacement(rect: DOMRect) {
  const W = window.innerWidth
  const H = window.innerHeight
  const roomBelow = Math.max(0, H - rect.bottom - 14)
  const roomAbove = Math.max(0, rect.top - 14)
  const below = roomBelow >= roomAbove
  const leading = rect.left + rect.width / 2 < W * 0.6
  const width = Math.min(320, W - 16)
  const inset = Math.max(8, W - width - 8)
  const style: React.CSSProperties = {
    [below ? 'top' : 'bottom']: below ? rect.bottom + 6 : H - rect.top + 6,
    ...(leading ? { left: Math.min(Math.max(8, rect.left), inset) } : { right: Math.min(Math.max(8, W - rect.right), inset) }),
    transformOrigin: `${below ? 'top' : 'bottom'} ${leading ? 'left' : 'right'}`,
  }
  return { style, width, room: below ? roomBelow : roomAbove }
}

/**
 * Presents `MenuPanel` next to `anchor` (or at `point`, for right-clicks), flipping to stay on
 * screen; a click anywhere else closes it (`.codyncMenu` / `.contextActions`).
 */
export function AnchoredMenu({ open, onClose, anchor, point, items, reactions, language, focusMenu = false, menuLabel }: {
  open: boolean
  onClose: () => void
  anchor?: RefObject<HTMLElement | null>
  point?: { x: number; y: number } | null
  items: () => MenuItem[]
  reactions?: ReactionPick
  /** Opt-in D545: foco real e navegação por teclado. */
  focusMenu?: boolean
  menuLabel?: string
  /** D454: como no `Dialog`, `lang`/`dir` na raiz do portal quando os itens já são traduzidos. */
  language?: PortalLanguage
}) {
  const { mounted, shown } = usePresence(open, 120)
  const [rect, setRect] = useState<DOMRect | null>(null)
  const frozen = useRef<MenuItem[]>([])
  const layer = useRef<HTMLDivElement>(null)
  if (open) frozen.current = items()
  useLayoutEffect(() => {
    if (!open) return
    if (point) setRect(new DOMRect(point.x, point.y, 0, 0))
    else if (anchor?.current) setRect(anchor.current.getBoundingClientRect())
  }, [open, point, anchor])
  useEscape(open, onClose, layer)
  useInertExit(open, mounted, layer)
  if (!mounted || !rect) return null
  const { style, width, room } = anchorPlacement(rect)
  return createPortal(
    <div ref={layer} inert={!open} className={`anchored ${shown ? 'shown' : ''}${language ? ' idioma-superficie' : ''}`} {...language} onMouseDown={(e) => e.target === e.currentTarget && onClose()} onContextMenu={(e) => { e.preventDefault(); onClose() }}>
      <div className="anchored-panel" style={{ ...style, maxWidth: width }}>
        <MenuPanel items={frozen.current} reactions={reactions} dismiss={onClose} maxHeight={Math.min(420, room)} focusMenu={focusMenu && open} menuLabel={menuLabel} />
      </div>
    </div>,
    document.body,
  )
}

/** Right-click opens a Codync menu at the pointer (kit's `.contextActions`). */
export function useContextMenu() {
  const [point, setPoint] = useState<{ x: number; y: number } | null>(null)
  return {
    point,
    open: point !== null,
    onContextMenu: (e: React.MouseEvent) => {
      e.preventDefault()
      e.stopPropagation()
      setPoint({ x: e.clientX, y: e.clientY })
    },
    close: () => setPoint(null),
  }
}

/** A sheet's content: a full-height column on the page color, usually a ModalHeader and a body. */
export function SheetPage({ className, children }: { className?: string; children: ReactNode }) {
  return <div className={`sheet-page${className ? ` ${className}` : ''}`}>{children}</div>
}

/**
 * A modal's title row: an optional icon, the title (and a line under it), optional trailing
 * actions, and a close button that dismisses the modal, or calls `onClose` for a panel that
 * isn't one.
 */
export function ModalHeader({ title, subtitle, icon, trailing, onClose, closeTitle = 'Fechar' }: {
  title: ReactNode
  subtitle?: ReactNode
  /** An SF Symbol name (shown on a tile), or a picture of its own (an agent's logo). */
  icon?: string | ReactElement
  trailing?: ReactNode
  onClose?: () => void
  closeTitle?: string
}) {
  const dismiss = useDismiss()
  const titleId = useContext(ModalTitleContext)
  return (
    <div className="modal-header">
      {typeof icon === 'string' ? (
        <span className="modal-header-icon" aria-hidden>
          <Icon name={icon} size={15} />
        </span>
      ) : icon}
      <VStack gap={1} className="modal-title">
        <Text as="h2" id={titleId} size="title3" weight="semibold" tone="text" lines={2}>{title}</Text>
        {subtitle !== undefined ? <Text size="callout" tone="secondary" lines={2} title={typeof subtitle === 'string' ? subtitle : undefined}>{subtitle}</Text> : null}
      </VStack>
      {trailing}
      <IconButtonBase title={closeTitle} icon="xmark" onClick={onClose ?? dismiss} />
    </div>
  )
}

/** Plain icon button that lights up on hover and press. Icon-only, labelled for VoiceOver. */
export function IconButtonBase({ title, icon, onClick, selected = false, size = 28, disabled, className, buttonRef, menuOpen }: {
  title: string
  icon: string
  onClick?: (e: React.MouseEvent) => void
  selected?: boolean
  size?: number
  disabled?: boolean
  className?: string
  buttonRef?: RefObject<HTMLButtonElement | null>
  /** Set when the button opens a menu: whether that menu is open. */
  menuOpen?: boolean
}) {
  return (
    <button
      ref={buttonRef}
      className={`icon-button ${selected ? 'selected' : ''} ${className ?? ''}`}
      style={{ width: size, height: size, borderRadius: size * 0.28 }}
      title={title}
      aria-label={title}
      aria-haspopup={menuOpen === undefined ? undefined : 'menu'}
      aria-expanded={menuOpen}
      disabled={disabled}
      onClick={onClick}
    >
      <Icon name={icon} size={size / 2} weight="medium" />
    </button>
  )
}

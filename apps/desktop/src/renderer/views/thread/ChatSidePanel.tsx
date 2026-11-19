// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useRef, type ReactNode } from 'react'
import { ResizeHandle } from '../../components/Controls'
import { useT } from '../../lib/fork-i18n/estado'
import { usePresence } from '../../components/Overlay'

/**
 * The panel beside the conversation. The conversation takes its new width at once (one layout,
 * however long the chat), then the panel slides in or out over it; its edge drags the width. The
 * slide moves `right`, not a transform: window drag regions (the panel's top band) follow layout,
 * and a transform would leave them where the slide started, over the corner buttons.
 */
export function ChatSidePanel({ open, width, min, max, onResize, children, floating = false }: {
  open: boolean
  width: number
  min: number
  max: number
  onResize: (width: number) => void
  children: ReactNode
  floating?: boolean
}) {
  // Animate explicit open/close actions without animating the initial mount.
  const { mounted, shown } = usePresence(open, 300, false)
  const t = useT()
  const last = useRef({ width, children })
  if (open) last.current = { width, children }
  const content = open ? { width, children } : last.current
  // The content plus the edge's hairline.
  const panelWidth = content.width + (floating ? 24 : 1)
  return (
    <>
      <div className="side-panel-slot" style={{ width: open ? panelWidth : 0 }} />
      {mounted ? (
        <div className={`side-panel${floating ? ' bot-details-side-panel' : ''}`} inert={!open} style={{ width: panelWidth, right: shown ? 0 : -panelWidth }}>
          <ResizeHandle label={t('conversa.mensagens.redimensionar')} width={content.width} min={min} max={max} edge="leading" onResize={onResize} />
          <div className="side-panel-content" style={{ width: content.width }}>{content.children}</div>
        </div>
      ) : null}
    </>
  )
}

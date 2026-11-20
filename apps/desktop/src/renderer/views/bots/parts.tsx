// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
// Modificado pela DONNA Bot em 2026-11-06: ref/saída de painel, adaptado de Codync
// 219935410dffccb950f90d0565bd1d9f1661bd5b, Apache-2.0; leepokai e Claude Opus 5.5. Ver THIRD_PARTY.md.
import { useId, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { RowLabel, Switch } from '../../components/Controls'
import { anchorPlacement, useEscape, usePresence, type PortalLanguage } from '../../components/Overlay'
import { useInertExit } from '../../lib/fork-modal-stack'
import { VStack } from '../../components/Stack'
import './bots.css'

/** A labeled input inside a section row: label (and an optional note) over the field, which they name. */
export function Field({ label, detail, children }: { label: string; detail?: string | null; children: ReactNode }) {
  const id = useId()
  return (
    <VStack gap={8} role="group" aria-labelledby={`${id}-label`} aria-describedby={detail ? `${id}-detail` : undefined}>
      <RowLabel label={label} detail={detail} labelId={`${id}-label`} detailId={`${id}-detail`} />
      {children}
    </VStack>
  )
}

/** An on/off row: title and a short note, the switch on the right. */
export function SwitchRow({ title, detail, icon, on, onChange, disabled }: { title: string; detail?: string | null; /** Antes do título (o logo de um conector). */ icon?: ReactNode; on: boolean; onChange: (on: boolean) => void; disabled?: boolean }) {
  return (
    <Switch on={on} onChange={onChange} disabled={disabled}>
      {icon ? <span className="switch-row-com-icone">{icon}<RowLabel label={title} detail={detail} /></span> : <RowLabel label={title} detail={detail} />}
    </Switch>
  )
}

/** A multi-line field that grows with its text between `minRows` and `maxRows` (`axis: .vertical`). */
export function AutoTextArea({ value, onChange, placeholder, minRows, maxRows, className, onKeyDown }: {
  value: string
  onChange: (v: string) => void
  placeholder?: string
  minRows: number
  maxRows: number
  className?: string
  onKeyDown?: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void
}) {
  const ref = useRef<HTMLTextAreaElement>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const style = getComputedStyle(el)
    const line = parseFloat(style.lineHeight) || 15
    const pad = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom)
    el.style.height = '0px'
    el.style.height = `${Math.min(Math.max(el.scrollHeight, line * minRows + pad), line * maxRows + pad)}px`
  }, [value, minRows, maxRows])
  return <textarea ref={ref} rows={minRows} className={className} value={value} placeholder={placeholder} onChange={(e) => onChange(e.target.value)} onKeyDown={onKeyDown} />
}

const panelStyle = ({ style, room }: ReturnType<typeof anchorPlacement>) => ({ ...style, '--anchored-room': `${room}px` }) as CSSProperties

/** A floating panel next to `anchor` that a click anywhere else closes (kit `AnchoredPanel`). */
/** `language`: D454, `lang`/`dir` do idioma escolhido na raiz do portal quando o conteúdo já é traduzido (como no `Dialog`). */
export function AnchoredPanel({ open, onClose, anchor, children, language }: { open: boolean; onClose: () => void; anchor: RefObject<HTMLElement | null>; children: ReactNode; language?: PortalLanguage }) {
  const { mounted, shown } = usePresence(open, 120)
  const [rect, setRect] = useState<DOMRect | null>(null)
  const layer = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    if (open && anchor.current) setRect(anchor.current.getBoundingClientRect())
  }, [open, anchor])
  useEscape(open, onClose, layer)
  useInertExit(open, mounted, layer)
  if (!mounted || !rect) return null
  return createPortal(
    <div ref={layer} inert={!open} className={`anchored ${shown ? 'shown' : ''}${language ? ' idioma-superficie' : ''}`} {...language} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      {/* `--anchored-room`: a altura livre do lado em que abriu, para o conteúdo não passar da janela. */}
      <div className="anchored-panel" style={panelStyle(anchorPlacement(rect))}>
        {children}
      </div>
    </div>,
    document.body,
  )
}

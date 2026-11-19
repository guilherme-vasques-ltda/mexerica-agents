import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from 'react'
import { reduceMotion, useReduceMotion } from '../../../lib/theme'
import { useT } from '../../../lib/fork-i18n/estado'

// D165 fase F (components/ApprovalCard): "segure para aprovar". The pointer, Space or Enter held for
// 0.9 s confirms; letting go earlier cancels. Alternatives to the gesture: with reduced motion one press
// confirms, and an activation that carries no pointer or key (VoiceOver, Switch Control) confirms at once.
// It only calls `onConfirm` with the option it was given: nothing is approved by itself.

export const HOLD_MS = 900

export function HoldButton({ onConfirm, disabled, primary = false, label, children }: {
  onConfirm: () => void
  disabled?: boolean
  primary?: boolean
  /** The option's name, for the accessible label. */
  label: string
  children: ReactNode
}) {
  const reducedMotion = useReduceMotion()
  const t = useT()
  const [holding, setHolding] = useState(false)
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null)
  const pressed = useRef(false)
  const stop = () => {
    if (timer.current) clearTimeout(timer.current)
    timer.current = null
    pressed.current = false
    setHolding(false)
  }
  // Changing the gesture or disabling the request cancels a hold; it never completes one.
  useEffect(() => { stop(); return stop }, [reducedMotion, disabled])
  const start = () => {
    if (disabled || pressed.current) return
    pressed.current = true
    // Read at the event boundary too: a media change can precede React's next render.
    if (reduceMotion()) {
      onConfirm()
      return
    }
    setHolding(true)
    timer.current = setTimeout(() => {
      timer.current = null
      setHolding(false)
      onConfirm()
    }, HOLD_MS)
  }
  const keyDown = (e: KeyboardEvent) => {
    if (e.key !== ' ' && e.key !== 'Enter') return
    // The browser would click on Enter (keydown) and Space (keyup): the hold replaces that click.
    e.preventDefault()
    if (!e.repeat) start()
  }
  const keyUp = (e: KeyboardEvent) => {
    if (e.key !== ' ' && e.key !== 'Enter') return
    e.preventDefault()
    stop()
  }
  return (
    <button
      className={`hold-button${primary ? ' primary' : ''}${holding ? ' holding' : ''}`}
      disabled={disabled}
      aria-label={t(reducedMotion ? 'conversa.aprovacao.segurar.ative' : 'conversa.aprovacao.segurar.segure', { opcao: label })}
      style={{ '--hold-ms': `${HOLD_MS}ms` } as React.CSSProperties}
      onPointerDown={(e) => { if (e.button === 0) start() }}
      onPointerUp={stop}
      onPointerLeave={stop}
      onPointerCancel={stop}
      onKeyDown={keyDown}
      onKeyUp={keyUp}
      onBlur={stop}
      onClick={(e) => {
        // A click with no pointer or key behind it comes from assistive tech: confirm at once.
        if (e.detail === 0 && !pressed.current && !disabled) onConfirm()
      }}
    >
      <span className="hold-button-fill" aria-hidden />
      <span className="hold-button-label">{children}</span>
    </button>
  )
}

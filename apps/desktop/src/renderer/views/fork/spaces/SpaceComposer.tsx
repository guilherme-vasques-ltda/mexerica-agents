import { useEffect, useRef, useState } from 'react'
import { Icon } from '../../../components/Icon'
import { useT } from '../../../lib/fork-i18n/estado'
import type { SpaceContext, SpaceMessage } from '../../../store/fork-space-context'

/** Montado por Hub/Space/conversa: texto, nonce e erros nunca passam à próxima seleção. */
export function SpaceComposer({ context }: { context: SpaceContext }) {
  const t = useT()
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(false)
  const field = useRef<HTMLTextAreaElement>(null)
  const button = useRef<HTMLButtonElement>(null)
  const pending = useRef<SpaceMessage | null>(null)
  const locked = useRef(false)
  const alive = useRef(true)
  useEffect(() => { alive.current = true; return () => { alive.current = false } }, [])
  // O nó desmontado nunca limpa o rascunho de outra conversa.
  const send = async () => {
    if (locked.current || !context.canSend || !text.trim()) return
    const active = document.activeElement
    const fromButton = active === button.current
    const retainFocus = active === field.current || fromButton
    locked.current = true
    setBusy(true)
    setError(false)
    try {
      pending.current ??= context.prepareMessage(text)
      await pending.current.send()
      if (alive.current) { setText(''); pending.current = null }
    } catch { if (alive.current) setError(true) }
    finally {
      locked.current = false
      if (alive.current) {
        setBusy(false)
        const current = document.activeElement
        if (retainFocus && field.current?.isConnected && !field.current.disabled
          && (current === field.current || current === button.current || (fromButton && current === document.body))) field.current.focus({ preventScroll: true })
      }
    }
  }
  return <div className="space-composer-wrap">
    {error ? <p className="space-composer-error" role="alert">{t('spaces.compose.falhou')}</p> : null}
    <form className="space-composer" aria-busy={busy} onSubmit={(e) => { e.preventDefault(); void send() }}>
      <textarea ref={field} dir="auto" aria-label={t('spaces.compose.mensagem')} placeholder={t('spaces.compose.mensagem')}
        value={text} rows={2} maxLength={250000} readOnly={busy} disabled={!context.canSend}
        onChange={(e) => { if (locked.current) return; setText(e.target.value); pending.current = null; setError(false) }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) { e.preventDefault(); void send() }
        }} />
      <button ref={button} type="submit" className="space-composer-send" title={t('spaces.compose.enviar')} aria-label={t('spaces.compose.enviar')}
        disabled={busy || !context.canSend || !text.trim()}><Icon name="arrow.up" size={18} /></button>
    </form>
  </div>
}

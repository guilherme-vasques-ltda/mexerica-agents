import { useCallback, useRef, useState } from 'react'
import { ComposerInput } from '@shared/composer-segments'

/** Eventos nativos iguais na caixa principal e na Ilha, sem alterar o undo do campo. */
export function useComposerInput<T extends HTMLInputElement | HTMLTextAreaElement>() {
  const [input] = useState(() => new ComposerInput())
  const field = useRef<T | null>(null)
  const bind = useCallback((el: T | null) => {
    field.current = el
    if (!el) return
    const selection = () => ({ start: el.selectionStart ?? 0, end: el.selectionEnd ?? 0 })
    const before = (event: Event) => {
      if (event instanceof InputEvent) input.before(event.inputType, event.isTrusted, selection(), event.isComposing)
    }
    const start = () => input.startComposition(selection())
    const end = () => input.endComposition()
    el.addEventListener('beforeinput', before)
    el.addEventListener('compositionstart', start)
    el.addEventListener('compositionend', end)
    return () => {
      el.removeEventListener('beforeinput', before)
      el.removeEventListener('compositionstart', start)
      el.removeEventListener('compositionend', end)
      field.current = null
    }
  }, [input])
  return { input, field, bind }
}

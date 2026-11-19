import { useEffect } from 'react'
import { acceptsBergaShortcut, bergaTarget, type BergaTargetSource } from '../../../lib/fork-atalho-berga'
import { prefs } from '../../../lib/prefs'

/** O listener pertence só ao renderer focado; recebe o contexto atual a cada render. */
export function useAtalhoBerga(source: () => BergaTargetSource, available: () => boolean, open: (target: NonNullable<ReturnType<typeof bergaTarget>>) => void, unavailable: () => void) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const editing = e.composedPath().some((el) => el instanceof HTMLElement && (
        el.isContentEditable || !!el.closest('input, textarea, select, [role="textbox"], .monaco-editor, .xterm')
      ))
      if (!acceptsBergaShortcut(e, window.codync.platform, prefs.commandSpaceBerga.get(), document.hasFocus(), editing) || !available()) return
      e.preventDefault()
      const target = bergaTarget(source())
      if (target) open(target)
      else unavailable()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })
}

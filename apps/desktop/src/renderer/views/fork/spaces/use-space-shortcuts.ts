import { useEffect, useRef } from 'react'
import { spaceShortcut } from '../../../lib/fork-space-shortcuts'

export function useSpaceShortcuts(source: { ids: string[]; activeId: string | null; available: () => boolean; select: (id: string) => void; open: () => void }) {
  const current = useRef(source)
  current.current = source
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      const source = current.current
      if (!source.available()) return
      const action = spaceShortcut(event, window.codync.platform === 'darwin', source.ids, source.activeId)
      if (!action) return
      event.preventDefault()
      if ('open' in action) source.open()
      else source.select(action.id)
    }
    // E2 reserva ⌘1…9 antes do listener legado das abas de sessão.
    window.addEventListener('keydown', keydown, true)
    return () => window.removeEventListener('keydown', keydown, true)
  }, [])
}

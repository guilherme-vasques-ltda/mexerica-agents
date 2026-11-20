// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useContext, useId, useLayoutEffect } from 'react'
import { PersonalReadingVisible } from '../fork/spaces/reading-visibility'
import type { BotStore } from '../../store/bot-store'

/** A scoped read receipt that follows the view's visibility (and the window's), not the selection. */
export function useReading(store: BotStore, botId: string) {
  const inView = useContext(PersonalReadingVisible)
  const token = useId()
  const connected = store.connection.kind
  useLayoutEffect(() => {
    const update = () => store.setReading(token, botId, inView && document.visibilityState === 'visible' && document.hasFocus())
    update()
    window.addEventListener('focus', update)
    window.addEventListener('blur', update)
    document.addEventListener('visibilitychange', update)
    return () => {
      window.removeEventListener('focus', update)
      window.removeEventListener('blur', update)
      document.removeEventListener('visibilitychange', update)
      store.setReading(token, botId, false)
    }
  }, [store, token, botId, connected, inView])
}

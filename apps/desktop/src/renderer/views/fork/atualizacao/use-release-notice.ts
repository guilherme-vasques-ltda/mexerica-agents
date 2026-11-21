import { useEffect, useState } from 'react'
import type { ReleaseNoticeState } from '@shared/fork-release-notice'

/**
 * O estado do aviso de versão (Nível 1): lê uma vez e segue as mudanças da ponte. Sem ponte (testes
 * SSR, builds sem o handler), fica `null` e nada aparece — ausência de sinal nunca é "suportado".
 */
export function useReleaseNotice(): ReleaseNoticeState | null {
  const [state, setState] = useState<ReleaseNoticeState | null>(null)
  useEffect(() => {
    const bridge = typeof window === 'undefined' ? undefined : window.codync?.releaseNotice
    if (!bridge) return
    let live = true
    void bridge.state().then((s) => { if (live) setState(s) })
    const off = bridge.onChange((s) => setState(s))
    return () => {
      live = false
      off()
    }
  }, [])
  return state
}

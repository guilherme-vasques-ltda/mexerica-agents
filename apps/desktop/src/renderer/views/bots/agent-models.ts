import { useEffect, useRef, useState } from 'react'
import { useStore } from '../../store/context'
import { errorText } from './drafts'
import { useT } from '../../lib/fork-i18n/estado'

/** A model's own setting (effort, fast…) and the values the agent accepts. */
export interface ModelOption {
  id: string
  name: string
  values: { id: string; name: string }[]
  default?: string | null
}

/** Model identifiers and display names advertised by the selected computer's ACP agent. */
export interface AgentModels {
  models: { id: string; name: string; description?: string | null; options?: ModelOption[] }[]
  currentModelId?: string | null
  /** The agent offers Fast mode (absent on older hosts). */
  fastMode?: boolean
}

/** What the agent behind `backend` offers, loaded from the selected computer. */
export interface AgentModelsState {
  catalog: AgentModels | null
  loading: boolean
  failure: string | null
  reload: () => void
}

/** Models come from the selected computer's agent, never a bundled provider list. */
export function useAgentModels(backend: string): AgentModelsState {
  const store = useStore()
  const online = store.connection.kind === 'online'
  const [catalog, setCatalog] = useState<AgentModels | null>(null)
  const scope = useRef<string | null>(null)
  const [loading, setLoading] = useState(false)
  // R1 do #575 (checklist): a falha fica como chave ou erro cru e é traduzida ao desenhar, no idioma de agora.
  const [failure, setFailure] = useState<'semConexao' | { erro: string } | null>(null)
  const [retry, setRetry] = useState(0)
  const custom = backend === 'custom'
  const t = useT()

  useEffect(() => {
    const key = `${store.computer.id}/${backend}`
    if (scope.current !== key) {
      setCatalog(null)
      scope.current = key
    }
    setFailure(null)
    setLoading(false)
    if (custom) return
    const client = store.client
    if (!online || !client) {
      setFailure('semConexao')
      return
    }
    let cancelled = false
    setLoading(true)
    client
      .call<AgentModels>('agentModels', { backend }, 300_000)
      .then((res) => !cancelled && setCatalog(res))
      .catch((e: unknown) => !cancelled && setFailure({ erro: errorText(e) }))
      .finally(() => !cancelled && setLoading(false))
    return () => {
      cancelled = true
    }
  }, [store.computer.id, store.client, backend, online, retry, custom])

  const failureText = failure === null ? null : failure === 'semConexao' ? t('bots.editor.modelo.conecte') : t('bots.editor.modelo.falhou', { erro: failure.erro })
  return { catalog, loading, failure: failureText, reload: () => setRetry((r) => r + 1) }
}

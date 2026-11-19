import { useEffect, useMemo } from 'react'
import { useModel } from '../../lib/observable'
import { usePref } from '../../lib/prefs'
import type { BotStore } from '../../store/bot-store'
import { engine, isVoiceProvider } from '../settings/voice-settings'
import { estadoVozAparelho } from '../fork/voz/estado-aparelho'
import { CallAvailabilityState } from './call-availability-state'
import { onDeviceAvailable } from './voice'

/** Disponibilidade compartilhada pelas entradas da chamada, sempre no Hub da conversa. */
export function useCallAvailability(store: BotStore, detailsOpen: boolean) {
  const [chosen] = usePref(engine)
  const state = useModel(useMemo(() => new CallAvailabilityState(store), [store]))
  const aparelho = useModel(estadoVozAparelho())
  useEffect(() => aparelho.observar(), [aparelho])
  const local = onDeviceAvailable()

  useEffect(() => {
    const stop = state.start(local)
    window.addEventListener('focus', state.refresh)
    return () => {
      stop()
      window.removeEventListener('focus', state.refresh)
    }
  }, [state, local, chosen, detailsOpen])

  return state.available(local, isVoiceProvider(chosen) ? chosen : null, aparelho.estado?.chaves)
}

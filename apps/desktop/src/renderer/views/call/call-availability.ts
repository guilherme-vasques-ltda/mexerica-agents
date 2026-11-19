import type { EstadoChaves } from '../../../shared/fork-voz.ts'
import { fonteEfetiva } from '../fork/voz/chave-aparelho.ts'
import type { VoiceProvider, VoiceStatus } from '../settings/voice-settings.ts'

/**
 * Só oferece a chamada quando o motor escolhido ou a voz local podem iniciar. voz-03 (S8): sem voz local,
 * nunca escolhe sozinho um provedor pago que tenha chave; a pessoa escolhe em Ajustes → Voz.
 */
export function canStartCall(local: boolean, provider: VoiceProvider | null, status: VoiceStatus | null, aparelho?: EstadoChaves['chaves'] | null): boolean {
  if (local) return true
  if (provider) {
    const hub = status?.providers.some((p) => p.provider === provider && p.configured) ? 'hub' : 'nenhuma'
    return ['aparelho', 'hub'].includes(fonteEfetiva('tempo-real', aparelho?.[provider], hub))
  }
  return false
}

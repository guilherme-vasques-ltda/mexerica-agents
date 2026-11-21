// O aviso de motor mudo. O Hub grava o estado e o tempo (dados); a frase sai do catálogo no idioma do app.
import type { Entry } from '@shared/models'
import type { Tradutor } from '../../../lib/fork-i18n/estado'

export type AvisoMotorMudo = { estado: 'mudo' | 'voltou'; minutos: number }

/** O aviso de motor mudo desta linha, ou `null` se a linha é outra coisa (ou o Hub não mandou o campo direito). */
export function lerMotorMudo(entry: Entry): AvisoMotorMudo | null {
  const dado = entry.data.motorMudo
  if (!dado || (dado.estado !== 'mudo' && dado.estado !== 'voltou') || !Number.isFinite(dado.segundos)) return null
  return { estado: dado.estado, minutos: Math.max(1, Math.round(dado.segundos / 60)) }
}

export function textoMotorMudo(t: Tradutor, aviso: AvisoMotorMudo): string {
  if (aviso.estado === 'voltou') return t('conversa.avisos.motorMudo.voltou')
  return t('conversa.avisos.motorMudo.texto', { tempo: t.plural('conversa.avisos.motorMudo.minutos', aviso.minutos) })
}

import type { ChaveTexto } from '../../lib/fork-i18n/index.ts'

/**
 * Por que não há voz no próprio computador, como chave do catálogo (voz/chamada). Na Web o "computador" é o
 * navegador, e o stub de plataforma da Web diz `linux`: por isso a Web vem antes e nunca cita o Linux.
 */
export function chaveSemVozNoAparelho(plataforma: string, web: boolean): ChaveTexto {
  if (web) return 'voz.chamada.indisponivel.web'
  return plataforma === 'darwin' ? 'voz.chamada.indisponivel.mac'
    : plataforma === 'win32' ? 'voz.chamada.indisponivel.windows' : 'voz.chamada.indisponivel.linux'
}

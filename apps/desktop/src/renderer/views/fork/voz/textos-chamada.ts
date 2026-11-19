import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import type { Mensagem } from '../bot-settings/mensagem.ts'
import { textoTransicaoMac, textoTransicaoSemLocal, type ParteLocal } from './chave-aparelho.ts'

// voz-03 (plano §5, S9): textos da barra da chamada, sem jargão. Fatia 6b (D454): como dado (D507), traduzidos ao
// desenhar (catálogo voz/chamada e voz/ajustes, com `textoVoz`).

export const TEXTOS_CHAMADA = {
  /** Provedor falhou ou faltou a chave, no Mac com o Ditado: segue grátis no próprio Mac. */
  quedaLocal: { chave: 'voz.chamada.aviso.quedaLocal' } as Mensagem,
  /** Windows/Linux: não há voz local para seguir; a chamada para. */
  falhaSemQueda: { chave: 'voz.chamada.aviso.falhaSemQueda' } as Mensagem,
  /**
   * Sem voz utilizável e sem provedor escolhido (Windows, Linux ou Mac sem o ajudante de fala). Antes do
   * voz-03 a chamada escolhia sozinha o primeiro provedor com chave no Hub; agora a pessoa escolhe.
   * `motivo`: a chave do porquê (voz.chamada.indisponivel.*); são duas frases inteiras, uma depois da outra.
   */
  semVoz: (motivo: ChaveTexto): Mensagem[] => [{ chave: motivo }, { chave: 'voz.chamada.aviso.semVoz' }],
  /** Modo Fala com a entrada local indisponível e nenhuma Voz para texto paga escolhida. */
  semOuvir: { chave: 'voz.chamada.aviso.semOuvir' } as Mensagem,
  /** voz-04 até o 05b, no Mac: a parte com chave deste Mac segue no próprio Mac. */
  transicao: (parte: ParteLocal): Mensagem => ({ chave: textoTransicaoMac(parte) }),
  /** voz-04 até o 05b, sem Ditado: a chamada no modo Fala não liga. */
  soTempoReal: (hubComChave: string | null): Mensagem => textoTransicaoSemLocal(hubComChave),
  /** O main não contou as chaves deste aparelho: sem saber quem paga, a chamada não liga. */
  semEstadoDasChaves: { chave: 'voz.chamada.aviso.semEstadoDasChaves' } as Mensagem,
} as const

import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/troca.ts'

// DON-903: conversa/troca (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-907 C9a: a linha de troca de modelo e o balão técnico (views/fork/troca-modelo).
  'conversa.troca.linha': 'Modelo cambiado de {de} a {para}. La conversación pasó al nuevo modelo.',
  'conversa.troca.balao.sessao': 'El nuevo modelo retomó la conversación completa.',
  'conversa.troca.balao.passagem': 'El nuevo modelo recibió el resumen y las últimas solicitudes, no la conversación completa.',
  'conversa.troca.balao.enviando': 'Enviando…',
  'conversa.troca.balao.reenviados': '{n} tokens reenviados, sin aprovechar lo que ya estaba listo.',
  'conversa.troca.balao.enviados': '{n} tokens enviados, sin aprovechar lo que ya estaba listo.',
}

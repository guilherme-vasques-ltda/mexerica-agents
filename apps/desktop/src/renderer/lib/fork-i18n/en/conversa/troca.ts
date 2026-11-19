import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/troca.ts'

// DON-903: conversa/troca (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-907 C9a: a linha de troca de modelo e o balão técnico (views/fork/troca-modelo).
  'conversa.troca.linha': 'Model changed from {de} to {para}. The conversation was passed to the new model.',
  'conversa.troca.balao.sessao': 'The new model resumed the whole conversation.',
  'conversa.troca.balao.passagem': 'The new model got the summary and the latest requests, not the whole conversation.',
  'conversa.troca.balao.enviando': 'Sending…',
  'conversa.troca.balao.reenviados': '{n} tokens sent again, without reusing what was already prepared.',
  'conversa.troca.balao.enviados': '{n} tokens sent, without reusing what was already prepared.',
}

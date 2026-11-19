import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/troca.ts'

// DON-903: conversa/troca (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-907 C9a: a linha de troca de modelo e o balão técnico (views/fork/troca-modelo).
  'conversa.troca.linha': 'Modèle changé de {de} à {para}. La conversation a été transmise au nouveau modèle.',
  'conversa.troca.balao.sessao': 'Le nouveau modèle a repris toute la conversation.',
  'conversa.troca.balao.passagem': 'Le nouveau modèle a reçu le résumé et les dernières demandes, pas toute la conversation.',
  'conversa.troca.balao.enviando': 'Envoi…',
  'conversa.troca.balao.reenviados': '{n} tokens renvoyés, sans réutiliser ce qui était déjà prêt.',
  'conversa.troca.balao.enviados': '{n} tokens envoyés, sans réutiliser ce qui était déjà prêt.',
}

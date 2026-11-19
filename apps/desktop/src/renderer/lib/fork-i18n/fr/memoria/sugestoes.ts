import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/memoria/sugestoes.ts'

// DON-903: memoria/sugestoes (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // D389: Sugestões na Memória do bot (views/fork/memoria-auto) e o contador do cartão.
  'memoria.sugestoes.esperando': "Suggestions en attente : {n}",
  'memoria.sugestoes.aba': "Suggestions",
  'memoria.sugestoes.lead': "Ce que les bots veulent retenir. Rien de tout cela n'est en mémoire tant que vous ne l'approuvez pas.",
  'memoria.sugestoes.vazio': "Aucune suggestion en attente.",
  'memoria.sugestoes.aprovar': "Approuver",
  'memoria.sugestoes.descartar': "Écarter",
  'memoria.sugestoes.acao.remember': "Nouveau souvenir",
  'memoria.sugestoes.acao.revise': "Modification d'un souvenir",
  'memoria.sugestoes.acao.archive': "Archiver un souvenir",
  'memoria.sugestoes.acao.confirm': "Confirmer : vous l'avez dit",
  'memoria.sugestoes.acao.reclassify': "Confirmer d'après votre message",
  'memoria.sugestoes.acao.note': "Note {arquivo}",
  'memoria.sugestoes.removerNota': "Supprimer cette note",
  'memoria.sugestoes.compartilhada': "Pour tous vos bots",
  'memoria.sugestoes.mudou': "La mémoire a changé après cette suggestion. Rien n'a été écrasé.",
  'memoria.sugestoes.semMudanca': "C'était déjà en mémoire.",
  'memoria.sugestoes.erro': "Impossible de terminer. Réessayez.",
}

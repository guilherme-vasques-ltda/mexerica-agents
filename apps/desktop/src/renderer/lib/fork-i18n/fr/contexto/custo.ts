import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/contexto/custo.ts'

// DON-903: contexto/custo (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  // DON-908 C8
  'contexto.custo.titulo': 'Coût estimé de cette conversation',
  'contexto.custo.parcial': 'Partiel : {n} tours sur {total} sans prix',
  'contexto.custo.vazio': 'Aucun tour enregistré dans cette conversation depuis la mise à jour du Hub.',
  'contexto.custo.semTokens': 'Aucun token enregistré',
  'contexto.custo.falha': 'Coût indisponible pour le moment.',
  'contexto.custo.nota': "Estimation de l'équivalent API. Avec l'abonnement, vous ne payez pas au token ; ce qui compte, c'est la limite du compte.",
  'contexto.custo.parte.entrada': 'Entrée {n}',
  'contexto.custo.parte.cacheLido': 'Cache lu {n}',
  'contexto.custo.parte.cacheGravado': 'Cache écrit {n}',
  'contexto.custo.parte.saida': 'Sortie {n}',
}

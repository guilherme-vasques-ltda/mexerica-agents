import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/contrato.ts'

// DON-903: contrato (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'contrato.titulo': "Avec contrat",
  'contrato.resultado': "Résultat attendu",
  'contrato.aceite': "Comment vérifier",
  'contrato.escopo': "Où il peut intervenir",
  'contrato.limites': "Limites",
  'contrato.regra': "Règle de délégation suivie",
  'contrato.nota': "Proposé par le bot demandeur. Il oriente le travail ; il n’accorde aucune permission et ne change aucune limite.",
}

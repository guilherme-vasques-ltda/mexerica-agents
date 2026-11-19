import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/contrato.ts'

// DON-903: contrato (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'contrato.titulo': "With contract",
  'contrato.resultado': "Expected result",
  'contrato.aceite': "How to check",
  'contrato.escopo': "Where it may work",
  'contrato.limites': "Limits",
  'contrato.regra': "Delegation rule followed",
  'contrato.nota': "Proposed by the requesting bot. It guides the work; it grants no permission and changes no limits.",
}

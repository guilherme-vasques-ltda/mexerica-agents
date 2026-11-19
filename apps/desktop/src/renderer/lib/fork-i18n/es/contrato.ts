import type { Traducao } from '../index.ts'
import type { textos as fonte } from '../pt-BR/contrato.ts'

// DON-903: contrato (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  'contrato.titulo': "Con contrato",
  'contrato.resultado': "Resultado esperado",
  'contrato.aceite': "Cómo comprobarlo",
  'contrato.escopo': "Dónde puede trabajar",
  'contrato.limites': "Límites",
  'contrato.regra': "Regla de delegación seguida",
  'contrato.nota': "Propuesto por el bot que pidió. Orienta la entrega; no concede permisos ni cambia límites.",
}

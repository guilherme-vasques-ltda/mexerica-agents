import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/skills.ts'

// Fatia 2 (D454): conversa/skills (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "conversa.skills.motivo": "Motivo",
  "conversa.skills.agoraNao": "Ahora no",
  "conversa.skills.examinarRascunho": "Revisar borrador",
  "conversa.skills.soPessoal": "Las skills solo están disponibles en Personal. Este Space no puede abrir ni guardar en el catálogo personal.",
  "conversa.skills.indicacao.aria": "Sugerencia de skill",
  "conversa.skills.indicacao.de": "Sugerencia de {bot}",
  "conversa.skills.indicacao.referencia": "Referencia sugerida",
  "conversa.skills.indicacao.pista": "Es una pista de búsqueda por nombre o ID. No confirma existencia, versión, instalación ni uso por un bot. Revisar solo abre una consulta.",
  "conversa.skills.indicacao.soPessoal": "Las skills solo están disponibles en Personal. Este Space no puede consultar el catálogo personal.",
  "conversa.skills.indicacao.examinar": "Revisar sugerencia",
  "conversa.skills.recomendacao.aria": "Recomendación de skill",
  "conversa.skills.recomendacao.de": "Recomendación de {bot}",
  "conversa.skills.recomendacao.quando": "Cuándo usarla",
  "conversa.skills.recomendacao.instrucoes": "Instrucciones propuestas",
  "conversa.skills.recomendacao.nada": "Proponer y abrir este borrador no guarda nada, no activa skills ni aprueba acciones.",
  "conversa.skills.rascunho.revise": "Borrador de texto. Revísalo antes de guardar; no se activará ningún bot.",
}

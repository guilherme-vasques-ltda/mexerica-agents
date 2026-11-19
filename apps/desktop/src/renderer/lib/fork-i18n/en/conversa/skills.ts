import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/skills.ts'

// Fatia 2 (D454): conversa/skills (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "conversa.skills.motivo": "Reason",
  "conversa.skills.agoraNao": "Not now",
  "conversa.skills.examinarRascunho": "Review draft",
  "conversa.skills.soPessoal": "Skills are only available in Personal. This Space can't open or save to the personal catalog.",
  "conversa.skills.indicacao.aria": "Skill suggestion",
  "conversa.skills.indicacao.de": "Suggestion from {bot}",
  "conversa.skills.indicacao.referencia": "Suggested reference",
  "conversa.skills.indicacao.pista": "It's a search hint by name or ID. It doesn't confirm existence, version, installation or use by a bot. Reviewing only opens a search.",
  "conversa.skills.indicacao.soPessoal": "Skills are only available in Personal. This Space can't search the personal catalog.",
  "conversa.skills.indicacao.examinar": "Review suggestion",
  "conversa.skills.recomendacao.aria": "Skill recommendation",
  "conversa.skills.recomendacao.de": "Recommendation from {bot}",
  "conversa.skills.recomendacao.quando": "When to use",
  "conversa.skills.recomendacao.instrucoes": "Proposed instructions",
  "conversa.skills.recomendacao.nada": "Proposing and opening this draft doesn't save anything, turn on skills or approve actions.",
  "conversa.skills.rascunho.revise": "Text draft. Review it before saving; no bot will be turned on.",
}

import type { Traducao } from '../../index.ts'
import type { textos as fonte } from '../../pt-BR/conversa/skills.ts'

// Fatia 2 (D454): conversa/skills (registro.ts). Chaves completas; docs/guides/idiomas.md.
export const textos: Traducao<typeof fonte> = {
  "conversa.skills.motivo": "Raison",
  "conversa.skills.agoraNao": "Pas maintenant",
  "conversa.skills.examinarRascunho": "Examiner le brouillon",
  "conversa.skills.soPessoal": "Les skills ne sont disponibles que dans Personnel. Cet espace ne peut ni ouvrir ni enregistrer dans le catalogue personnel.",
  "conversa.skills.indicacao.aria": "Suggestion de skill",
  "conversa.skills.indicacao.de": "Suggestion de {bot}",
  "conversa.skills.indicacao.referencia": "Référence suggérée",
  "conversa.skills.indicacao.pista": "C'est une piste de recherche par nom ou ID. Elle ne confirme ni l'existence, ni la version, ni l'installation, ni l'usage par un bot. Examiner ouvre seulement une recherche.",
  "conversa.skills.indicacao.soPessoal": "Les skills ne sont disponibles que dans Personnel. Cet espace ne peut pas consulter le catalogue personnel.",
  "conversa.skills.indicacao.examinar": "Examiner la suggestion",
  "conversa.skills.recomendacao.aria": "Recommandation de skill",
  "conversa.skills.recomendacao.de": "Recommandation de {bot}",
  "conversa.skills.recomendacao.quando": "Quand l’utiliser",
  "conversa.skills.recomendacao.instrucoes": "Instructions proposées",
  "conversa.skills.recomendacao.nada": "Proposer et ouvrir ce brouillon n'enregistre rien, n'active pas de skills et n'approuve aucune action.",
  "conversa.skills.rascunho.revise": "Brouillon de texte. Relisez-le avant d’enregistrer ; aucun bot ne sera activé.",
}

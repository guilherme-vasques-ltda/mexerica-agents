/** d2a/DON-855. Só skillDetails RPC; comparação sem salvar revisão ou aprovar. */
export interface SkillReviewRequest {
  protocol: 1
  expectedInstallationId: string
  /** Ausente para abrir base; presente para comparar com a base já examinada. */
  proposal?: { expectedBodyDigest: string; markdown: string }
}

export interface SkillReviewDocument {
  revision: number
  bodyDigest: string
  /** SKILL.md integral, incluindo frontmatter; sem truncamento. */
  markdown: string
}

export interface SkillReviewComparison {
  reviewProtocol: 1
  effect: 'none'
  id: string
  installationId: string
  base: SkillReviewDocument
  /** Próxima revisão CANDIDATA. Não persistida. */
  proposal: SkillReviewDocument | null
  affectedBots: { id: string; name: string; selectionVersion: string }[]
  /** Integridade da comparação; não credencial, aprovação ou reserva para commit. */
  comparisonDigest: string
}

export interface SkillReviewResponse {
  skillProtocol: 2
  review: SkillReviewComparison
}

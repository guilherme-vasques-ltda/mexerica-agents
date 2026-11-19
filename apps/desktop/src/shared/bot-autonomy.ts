import type { BotDraft } from './models.ts'

/** O editor salva o perfil; autonomia e seleção de skills (c1) de um bot existente têm intenções próprias. */
export function botEditorUpdate(draft: BotDraft) {
  const { permission: _permission, permissionChosenByUser: _chosen, avatarConfig: _avatar, mexeriquinha: _mexeriquinha, skills: _skills, ...profile } = draft
  return { ...profile, model: draft.model ?? null }
}

/** O padrão geral preenche o formulário sem registrar uma escolha. */
export function inheritNewBotPermission(draft: BotDraft, permission: string): BotDraft {
  return draft.permissionChosenByUser === true ? draft : { ...draft, permission }
}

/** Inclusive voltar ao valor herdado é uma escolha explícita. */
export function chooseNewBotPermission(draft: BotDraft, permission: string): BotDraft {
  return { ...draft, permission, permissionChosenByUser: true }
}

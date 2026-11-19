import type { Entry, PermissionOption } from '../../../shared/models.ts'

export const APPROVE_IN_APP = 'Aprove no app'

/** Fail closed for old hosts, unknown classifications and ambiguous option IDs. */
export function voiceApprovalOption(card: Entry, requested: string): PermissionOption | undefined {
  if (card.kind !== 'permission' || card.data.status !== 'pending' || card.data.voiceApproval !== 'allow_once') return
  const wanted = requested.trim().toLocaleLowerCase('pt-BR')
  if (!wanted) return
  const options = card.data.options ?? []
  const matches = options.filter((o) => o.optionId === requested || o.name.toLocaleLowerCase('pt-BR') === wanted)
  const pick = matches.length === 1 ? matches[0] : undefined
  if (!pick || pick.kind !== 'allow_once' || !pick.optionId) return
  if (options.filter((o) => o.optionId === pick.optionId).length !== 1) return
  return pick
}

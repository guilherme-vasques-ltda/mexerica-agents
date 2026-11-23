import type { ComposerDrafts } from '../../../../shared/composer-drafts.ts'
import type { OriginDraft } from '../../../../shared/guard-hub.ts'

export function applyStarterDraft(drafts: ComposerDrafts, botId: string, prompt: string, previous: OriginDraft | undefined): OriginDraft | undefined {
  const current = drafts.get(botId)
  const before = drafts.origin(botId)
  if (current && (!previous || before !== previous)) return undefined
  drafts.edit(botId, prompt, 'generated', { start: 0, end: current.length })
  const after = drafts.origin(botId)
  return after !== before ? after : undefined
}

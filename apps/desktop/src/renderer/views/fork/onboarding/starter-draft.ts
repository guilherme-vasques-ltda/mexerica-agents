import type { ComposerDrafts } from '../../../../shared/composer-drafts.ts'
import type { OriginDraft } from '../../../../shared/guard-hub.ts'

export function applyStarterDraft(drafts: ComposerDrafts, botId: string, prompt: string, previous: OriginDraft | undefined): OriginDraft | undefined {
  const current = drafts.get(botId)
  if (current && (!previous || drafts.origin(botId) !== previous)) return undefined
  drafts.edit(botId, prompt, 'generated', { start: 0, end: current.length })
  return drafts.origin(botId)
}

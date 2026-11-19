// D169 (ToolSteps, revisão R1 F4): the live bubbles of a turn, one per author of pending steps.
import { isGroup, type Bot } from '../../../shared/models.ts'
import type { TurnSteps } from './cards/tool-steps.ts'

/** One author's pending steps; `author` is null when the Hub didn't say which bot did them, or named the channel. */
export interface WorkingBlock { authorId: string; author: Bot | null; steps: TurnSteps }

/** The pending steps by author, in arrival order. Only real bots are authors: a channel or an unknown id stays neutral. */
export function workingBlocks(pending: Map<string, TurnSteps>, bots: { get(id: string): Bot | undefined }): WorkingBlock[] {
  return [...pending.entries()].map(([authorId, steps]) => {
    const author = bots.get(authorId)
    return { authorId, author: author && !isGroup(author) ? author : null, steps }
  })
}

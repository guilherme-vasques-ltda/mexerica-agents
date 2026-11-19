/** Retrato de grupos do Space, na ordem de criação. */
export interface BotGroup { id: string; name: string; bots: string[] }
export interface BotGroupsSnapshot { rev: number; groups: BotGroup[] }
export type BotGroupsEdit =
  | { op: 'add'; name: string; botId?: string }
  | { op: 'rename'; id: string; name: string }
  | { op: 'remove'; id: string }
  | { op: 'place'; botId: string; groupId: string | null }
export interface BotGroupsEditResult { snapshot: BotGroupsSnapshot; created?: string }

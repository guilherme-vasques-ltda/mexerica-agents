import type { BotGroupsEdit, BotGroupsSnapshot } from '../../shared/bot-groups.ts'

export const UNGROUPED = 'fora-de-grupo'
export const groupOf = (snapshot: BotGroupsSnapshot, botId: string): string | null =>
  snapshot.groups.find((group) => group.bots.includes(botId))?.id ?? null

/** Preserva a ordem do roster, inclusive quando os IDs do Hub vêm ordenados de outra forma. */
export function groupRoster<T extends { bot: { id: string } }>(items: T[], snapshot: BotGroupsSnapshot) {
  return [
    { id: UNGROUPED, name: '', items: items.filter((item) => groupOf(snapshot, item.bot.id) === null) },
    ...snapshot.groups.map((group) => ({ id: group.id, name: group.name, items: items.filter((item) => group.bots.includes(item.bot.id)) })),
  ]
}

/** O que um grupo recolhido continua mostrando: a conversa aberta e as que esperam o dono (`needsInput`). */
export const stayVisibleWhenClosed = (bot: { status: string }, selected: boolean): boolean => selected || bot.status === 'needsInput'

export function inGroup(snapshot: BotGroupsSnapshot, botId: string, groupId: string): boolean {
  return (groupOf(snapshot, botId) ?? UNGROUPED) === groupId
}

export const collapseKey = (hubId: string, spaceId: string, groupId: string) => JSON.stringify([hubId, spaceId, groupId])

export function botGroupTarget(element: Element | null, computerId: string): string | undefined {
  const target = element?.closest<HTMLElement>('[data-bot-group][data-computer-id]')
  return target?.dataset.computerId === computerId ? target.dataset.botGroup : undefined
}

/** Não envia escrita se o nome ainda é o mesmo ou se o grupo sumiu. */
export function renameIntent(snapshot: BotGroupsSnapshot, id: string, name: string): BotGroupsEdit | null {
  const group = snapshot.groups.find((item) => item.id === id)
  return group && group.name !== name ? { op: 'rename', id, name } : null
}

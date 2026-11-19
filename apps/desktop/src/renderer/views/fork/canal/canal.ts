// Painel do canal (Barra05): líder, nome e descrição pelo `updateBot` do grupo que o Hub já tem
// (Bot.leader e GroupDraft.leader, PR #350).
import type { Bot, GroupDraft } from '../../../../shared/models.ts'

/** O líder do canal, se ainda for membro. Canal sem líder = null; ninguém vira líder sozinho. */
export function leaderOf(channel: Bot): string | null {
  return channel.leader && channel.members.includes(channel.leader) ? channel.leader : null
}

/** O draft do canal para o `updateBot`; `leader` ausente mantém o atual no Hub, null tira o líder. */
export function channelDraft(channel: Bot, change: Partial<Pick<GroupDraft, 'name' | 'description' | 'leader'>> = {}): GroupDraft {
  return {
    id: channel.id, kind: 'group', name: change.name ?? channel.name, description: change.description ?? channel.description,
    members: channel.members, pinned: channel.pinned, ...('leader' in change ? { leader: change.leader ?? null } : {}),
  }
}

/** O Hub recusa tirar o líder; a view desliga o "×" dele com a dica "Troque o líder antes". */
export const canRemoveMember = (channel: Bot, botId: string) => leaderOf(channel) !== botId

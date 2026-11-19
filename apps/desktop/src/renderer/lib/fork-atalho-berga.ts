import type { SpaceConversation } from '../../shared/fork-spaces.ts'

/** Só a combinação exata, fora de composição, repetição ou edição. Nunca registra atalho do sistema. */
export function acceptsBergaShortcut(e: Pick<KeyboardEvent, 'key' | 'code' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey' | 'repeat' | 'isComposing' | 'keyCode' | 'defaultPrevented'>, platform: string, enabled: boolean, focused: boolean, editing: boolean) {
  return platform === 'darwin' && enabled && focused && !editing && !e.defaultPrevented && !e.isComposing && e.keyCode !== 229 && !e.repeat &&
    e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && e.code === 'Space' && e.key === ' '
}

export interface BergaTargetSource {
  space: { id: string; ready: boolean; live: boolean; principalId: string | null; conversations: readonly SpaceConversation[] } | null
  personal: { computerId: string; ready: boolean; principalId: string | null; hasBot: boolean } | null
}

/** Um Space explícito nunca pode resolver para o Pessoal, mesmo carregando, offline ou sem principal. */
export function bergaTarget({ space, personal }: BergaTargetSource): { kind: 'space'; id: string } | { kind: 'personal'; computerId: string; botId: string } | null {
  if (space) {
    if (!space.ready || !space.live || !space.principalId) return null
    const chat = space.conversations.find((c) => c.spaceId === space.id && c.botId === space.principalId && c.primary && !c.closed)
    return chat ? { kind: 'space', id: chat.id } : null
  }
  return personal?.ready && personal.principalId && personal.hasBot
    ? { kind: 'personal', computerId: personal.computerId, botId: personal.principalId } : null
}

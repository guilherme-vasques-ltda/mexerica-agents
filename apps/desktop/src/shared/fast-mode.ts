// Fast mode in the chat (the composer's `/fast`) and bot settings; the host reports what the agent
// offers (`fastModeSupport`, `fastModeIssue`): docs/features/marketplace.md, "Fast mode".
import type { Bot } from './models'
import type { Tradutor } from '../renderer/lib/fork-i18n/index.ts'

export type FastModeState = 'on' | 'off' | 'refused'

/**
 * An agent bot unless its agent said it has no Fast mode (an unknown one may: its first session
 * settles it), or one that has it on (so it can be turned off).
 */
export function fastModeShown(bot: Bot | null): boolean {
  if (!bot || bot.kind === 'group') return false
  return bot.fastMode || bot.fastModeSupport !== 'unsupported'
}

export function fastModeState(bot: Bot): FastModeState {
  if (!bot.fastMode) return 'off'
  return bot.fastModeSupport === 'refused' ? 'refused' : 'on'
}

/** Why it isn't in effect, when the agent turned it off itself. */
export function fastModeRefusal(t: Tradutor, bot: Bot): string | null {
  if (fastModeState(bot) !== 'refused') return null
  return t('sistema.conversa.rapido.recusado', { motivo: bot.fastModeIssue ?? t('sistema.conversa.rapido.indisponivel') })
}

/**
 * A group's `/fast`: its members that show Fast mode, on once all of them have it on. Picking it
 * sets each one's own Fast mode, so it follows them into their own chats.
 */
export function groupFastMode(t: Tradutor, members: Bot[]): { members: Bot[]; state: FastModeState; detail: string } | null {
  const shown = members.filter(fastModeShown)
  if (!shown.length) return null
  if (!shown.every((m) => m.fastMode)) return { members: shown, state: 'off', detail: t('sistema.conversa.rapido.ligarTodos') }
  const refused = shown.find((m) => fastModeState(m) === 'refused')
  if (refused) return { members: shown, state: 'refused', detail: t('sistema.conversa.rapido.recusadoBot', { bot: refused.name, motivo: refused.fastModeIssue ?? t('sistema.conversa.rapido.indisponivel') }) }
  return { members: shown, state: 'on', detail: t('sistema.conversa.rapido.desligarTodos') }
}

/** What picking `/fast` in the composer's command list does, or why it isn't in effect. */
export function fastModeDetail(t: Tradutor, bot: Bot): string {
  switch (fastModeState(bot)) {
    case 'off':
      return t('sistema.conversa.rapido.ligar')
    case 'on':
      return t('sistema.conversa.rapido.desligar')
    case 'refused':
      return fastModeRefusal(t, bot) ?? ''
  }
}

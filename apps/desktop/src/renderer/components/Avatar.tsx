// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import type { CSSProperties } from 'react'
import { isGroup, needsInput, type Bot } from '@shared/models'
import { BotAvatar } from '../fork/avatars/Avatar'
export { BotAvatar, CharacterAvatar, type Mood } from '../fork/avatars/Avatar'
import { Icon } from './Icon'
import './avatar.css'

/**
 * A group chat's face, clustered like iMessage: two bots tucked diagonally, three in a
 * triangle, four in a 2×2 grid; past four the last cell counts the rest.
 */
export function GroupAvatar({ members, size = 40, animated = true }: { members: Bot[]; size?: number; animated?: boolean }) {
  const place = (bot: Bot, side: number, corner: CSSProperties) => (
    <div key={bot.id} className="avatar-slot" style={corner}>
      <BotAvatar bot={bot} size={side} animated={animated} />
    </div>
  )
  let content
  switch (members.length) {
    case 0:
      content = (
        <div className="avatar-empty">
          <Icon name="person.2" size={size * 0.4} />
        </div>
      )
      break
    case 1:
      content = <BotAvatar bot={members[0]!} size={size} animated={animated} />
      break
    case 2:
      content = [place(members[1]!, size * 0.66, { top: 0, right: 0 }), place(members[0]!, size * 0.66, { bottom: 0, left: 0 })]
      break
    case 3:
      content = [
        place(members[0]!, size * 0.55, { top: 0, left: (size - size * 0.55) / 2 }),
        place(members[1]!, size * 0.55, { bottom: 0, left: 0 }),
        place(members[2]!, size * 0.55, { bottom: 0, right: 0 }),
      ]
      break
    default:
      content = [
        place(members[0]!, size * 0.5, { top: 0, left: 0 }),
        place(members[1]!, size * 0.5, { top: 0, right: 0 }),
        place(members[2]!, size * 0.5, { bottom: 0, left: 0 }),
        members.length === 4 ? (
          place(members[3]!, size * 0.5, { bottom: 0, right: 0 })
        ) : (
          <div key="more" className="avatar-more" style={{ width: size * 0.5, height: size * 0.5, fontSize: size * 0.24 }}>
            +{members.length - 3}
          </div>
        ),
      ]
  }
  return <div role="group" aria-label={`Canal: ${members.map((bot) => bot.name).join(', ') || 'sem bots'}`} className="avatar-frame" style={{ width: size, height: size }}>{content}</div>
}

/** Avatar with the roster status dot: filled = unread, amber = needs you. A group shows its members. */
export function AvatarWithStatus({ bot, members = [], size = 44 }: { bot: Bot; members?: Bot[]; size?: number }) {
  return (
    <div className="avatar-frame" style={{ width: size, height: size }}>
      {isGroup(bot) ? <GroupAvatar members={members} size={size} /> : <BotAvatar bot={bot} size={size} />}
      {needsInput(bot) ? (
        <div className="avatar-badge needs-you" style={{ width: size * 0.36, height: size * 0.36 }}>
          <Icon name="exclamationmark" size={size * 0.2} weight="black" scaled={false} />
        </div>
      ) : bot.unread > 0 ? (
        <div className="avatar-badge unread" style={{ width: size * 0.28, height: size * 0.28 }} />
      ) : null}
    </div>
  )
}

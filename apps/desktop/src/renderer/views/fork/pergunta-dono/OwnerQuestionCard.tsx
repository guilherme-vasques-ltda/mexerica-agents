import type { ReactNode } from 'react'
import { useT } from '../../../lib/fork-i18n/estado'
import type { Bot, Entry } from '@shared/models'
import { isGroup } from '@shared/models'
import { ownerQuestionOf, ownerQuestionView } from '@shared/fork-owner-question'
import { BotAvatar } from '../../../components/Avatar'
import { Icon } from '../../../components/Icon'
import { useStoreRef as useStore } from '../../../store/context'
import { relativeTime } from '../../thread/chat-items'
import './owner-question.css'

// MB-1b (D326): o cartão "perguntar ao dono" no balão do bot. Proposta: docs/reference/redesenho/README.md
// (Pergunta ao dono). Só mostra o estado que o Hub registrou; não tem campo próprio: responder é escrever
// na caixa da conversa (a próxima mensagem humana ali responde). O texto do bot entra como nó de texto.
export function OwnerQuestionCard({ entry, chat }: { entry: Entry; chat: Bot | null }) {
  const store = useStore()
  const card = ownerQuestionOf(entry.data)
  if (!card) return null
  const author = entry.data.author ? store.bots.get(entry.data.author) : chat && !isGroup(chat) ? chat : undefined
  const name = store.authorName(entry.data.author)
  return <OwnerQuestionView status={card.status} text={entry.data.text ?? ''} name={name} time={relativeTime.time(entry.createdAt)} face={author ? <BotAvatar bot={author} size={28} animated={false} /> : null} />
}

/** Sem store: também serve à prévia com dados sintéticos. */
export function OwnerQuestionView({ status, text, name, time, face }: { status: 'pending' | 'answered' | 'withdrawn'; text: string; name: string; time: string; face?: ReactNode }) {
  const view = ownerQuestionView(status, name)
  const t = useT()
  return (
    <div className="message agent with-face owner-question" data-status={status}>
      <span className="message-face" aria-hidden>{face}</span>
      <div className="message-body">
        <div className="bubble agent owner-question-bubble" role="group" aria-label={t('conversa.avisos.pergunta.aria', { bot: name, estado: view.label })}>
          <div className="owner-question-head">
            <span className="bubble-meta-name">{name}</span>
            <time className="bubble-meta-time">{time}</time>
            <span className={`owner-question-state is-${status}`}>
              <Icon name={view.icon} size={13} className="owner-question-state-mark" />
              {view.label}
            </span>
          </div>
          <p className="owner-question-text">{text}</p>
          <p className="owner-question-note">{view.note}</p>
        </div>
      </div>
    </div>
  )
}

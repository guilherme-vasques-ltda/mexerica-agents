import type { ReactNode } from 'react'
import { isGroup, type Bot, type Entry } from '@shared/models'
import { BotAvatar } from '../../../components/Avatar'
import { useModel } from '../../../lib/observable'
import { useStoreRef as useStore } from '../../../store/context'
import { choiceAnswersOf } from '../../../store/fork-genui-choice'
import { relativeTime } from '../../thread/chat-items'
import { ChoiceCard } from './ChoiceCard'
import { choiceErrorText, genuiChoiceOf, type ChoiceEntry, type ChoiceSendError } from './choice-entry'
import type { ChoiceIntent, ChoiceSubmission } from './escolha-model'
import { useT } from '../../../lib/fork-i18n/estado'

// UI generativa no chat, G1b (D273): o cartão de escolha na lista de mensagens. ChatRows (herdado) só
// chama `genuiChoiceRow`; tudo o mais é deste módulo. A resposta sai pelo store (fork-genui-choice), no
// Space e na conversa da própria entrada; a view não faz I/O. Proposta aprovada:
// docs/reference/redesenho/propostas/ui-generativa-chat-marco2-choice-20261104.

/** Gancho de ChatRows: o cartão quando a entrada é `genuiChoice` válida; senão null e a linha segue o texto literal. */
export function genuiChoiceRow(entry: Entry, chat: Bot | null, groupStart: boolean): ReactNode {
  const card = genuiChoiceOf(entry)
  if (!card) return null
  return <div className={groupStart ? 'chat-row group-start' : 'chat-row'}><GenuiChoiceRow entry={entry} card={card} chat={chat} /></div>
}

function GenuiChoiceRow({ entry, card, chat }: { entry: Entry; card: ChoiceEntry; chat: Bot | null }) {
  const store = useStore()
  const answers = useModel(choiceAnswersOf(store))
  const author = entry.data.author ? store.bots.get(entry.data.author) : chat && !isGroup(chat) ? chat : undefined
  const sent = answers.submission(store, card.questionId)
  return (
    <GenuiChoiceMessage
      card={card}
      name={store.authorName(entry.data.author ?? chat?.id)}
      time={relativeTime.time(entry.createdAt)}
      face={author ? <BotAvatar bot={author} size={28} animated={false} /> : null}
      submission={sent?.submission ?? null}
      error={sent?.error ?? null}
      onAnswer={(intent) => void answers.answer(store, card, intent)}
    />
  )
}

/** Sem store: o balão do bot com o cartão real. Também serve à prévia com dados fictícios. */
export function GenuiChoiceMessage({ card, name, time, face, submission, error, onAnswer }: {
  card: ChoiceEntry
  name: string
  time: string
  face?: ReactNode
  submission: ChoiceSubmission | null
  error: ChoiceSendError | null
  onAnswer: (intent: ChoiceIntent) => void
}) {
  const t = useT()
  return (
    <div className="message agent with-face genui-choice-message">
      <span className="message-face" aria-hidden>{face}</span>
      <div className="message-body">
        <div className="bubble agent">
          <ChoiceCard
            questionId={card.questionId}
            payload={card.snapshot}
            hub={card.hub}
            submission={submission}
            onAnswer={onAnswer}
            botName={name}
            author={{ name, time }}
            errorText={error ? choiceErrorText(t, error) : null}
          />
        </div>
      </div>
    </div>
  )
}

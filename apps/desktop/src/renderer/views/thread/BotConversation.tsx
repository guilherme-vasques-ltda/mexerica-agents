// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useEffect, useRef, useState } from 'react'
import { BotAvatar } from '../../components/Avatar'
import { Icon } from '../../components/Icon'
import { ModalHeader } from '../../components/Overlay'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { chatEntriesOf, useStoreRef as useStore } from '../../store/context'
import { listEqual, useSelector } from '../../lib/observable'
import type { Bot, Entry } from '@shared/models'
import { botConversation, buildBotConversation, groupSummary, verbOf, type BotConversationRow, type BotExchange } from './bot-exchange'
import { AuthorLabel, Bubble, TimeSeparator } from './ChatRows'
import { MarkdownText } from './Markdown'
import { ContratoTarefa } from '../fork/contrato/ContratoTarefa'
import { useT } from '../../lib/fork-i18n/estado'
import './bot-conversation.css'

/** The compact chat row for bot-to-bot messages with one peer: who, never what. Opens the conversation. */
export function BotMessageRow({ exchanges, open }: { exchanges: BotExchange[]; open: (peerId: string) => void }) {
  const store = useStore()
  const { peerId, count, failed } = groupSummary(exchanges)
  const [peer, name] = useSelector(store, (s) => [s.bots.get(peerId), s.authorName(peerId)], listEqual)
  const t = useT()
  // O rótulo acessível é a frase inteira; o visível põe o rosto entre a abertura e o nome.
  const lead = count > 1 ? t.plural('conversa.mensagens.entreBots.varias', count) : t(`conversa.mensagens.entreBots.${verbOf(exchanges[0]!)}`)
  const frase = count > 1 ? t.plural('conversa.mensagens.entreBots.variasAria', count, { bot: name }) : t(`conversa.mensagens.entreBots.${verbOf(exchanges[0]!)}Aria`, { bot: name })
  return (
    <button
      className={failed ? 'press bot-message-row failed' : 'press bot-message-row'}
      onClick={() => open(peerId)}
      title={t('conversa.mensagens.entreBots.abrir')}
      aria-label={failed ? t('conversa.mensagens.entreBots.falhou', { frase }) : frase}
    >
      {failed ? <Icon name="exclamationmark.triangle.fill" size={10} color="var(--danger)" /> : null}
      <Text size="footnote">{lead}</Text>
      {peer ? <BotAvatar bot={peer} size={24} animated={false} /> : null}
      {/* D249: one line; a long name ends with an ellipsis (whole in `title` and in the row's label). */}
      <Text size="footnote" weight="semibold" tone={failed ? 'danger' : 'text'} truncate title={name}>{name}</Text>
      <Icon name="chevron.right" size={10} color="var(--text-tertiary, var(--ink-3))" />
    </button>
  )
}

/** The pair a conversation is between, for its header: the chat's own bot, then its peer. */
function BotPair({ botId, peerId }: { botId: string; peerId: string }) {
  const store = useStore()
  const [botA, botB] = useSelector(store, (s) => [s.bots.get(botId), s.bots.get(peerId)], listEqual)
  const t = useT()
  const label = (id: string, bot: Bot | undefined) => (
    <HStack as="span" gap={6} shrink>
      {bot ? <BotAvatar bot={bot} size={24} animated={false} /> : null}
      <Text truncate>{store.authorName(id)}</Text>
    </HStack>
  )
  return (
    <HStack as="span" gap={8} aria-label={t('conversa.mensagens.entreBots.par', { a: store.authorName(botId), b: store.authorName(peerId) })}>
      {label(botId, botA)}
      <Icon name="arrow.left.arrow.right" size={11} color="var(--text-tertiary, var(--ink-3))" />
      {label(peerId, botB)}
    </HStack>
  )
}

/** Read-only history between a bot and one peer: the live chat merged with what the host returns. */
export function BotConversationView({ botId, peerId }: { botId: string; peerId: string }) {
  const store = useStore()
  const t = useT()
  // null while the host's history loads.
  const [fetched, setFetched] = useState<Entry[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const scroller = useRef<HTMLDivElement>(null)
  const entries = useSelector(store, (s) => chatEntriesOf(s, botId))
  const rows = buildBotConversation(botConversation(fetched ?? [], entries, peerId))

  useEffect(() => {
    let current = true
    store.botConversation(botId, peerId).then(
      (entries) => {
        if (current) setFetched(entries)
      },
      (reason: unknown) => {
        console.warn('botConversation failed', reason)
        if (!current) return
        setFetched([])
        setError(reason instanceof Error ? reason.message : String(reason))
      },
    )
    return () => {
      current = false
    }
  }, [store, botId, peerId])

  useEffect(() => {
    const el = scroller.current
    if (el) el.scrollTop = el.scrollHeight
  }, [rows.length, rows[rows.length - 1]?.id])

  return (
    <div className="bot-conversation">
      <ModalHeader closeTitle={t('conversa.fechar')} title={<BotPair botId={botId} peerId={peerId} />} />
      <div className="bot-conversation-scroll" ref={scroller}>
        <div className="bot-conversation-rows">
          {error ? <Text as="div" size="footnote" tone="danger" className="bot-conversation-note">{error}</Text> : null}
          {!error && fetched && rows.length === 0 ? <Text as="div" size="footnote" tone="tertiary" className="bot-conversation-note">{t('conversa.mensagens.entreBots.vazio')}</Text> : null}
          {rows.map((row) => <ConversationRow key={row.id} row={row} />)}
        </div>
      </div>
    </div>
  )
}

function ConversationRow({ row }: { row: BotConversationRow }) {
  const store = useStore()
  const awaitingName = useSelector(store, (s) => s.authorName(row.kind === 'status' ? row.awaiting : null))
  const t = useT()
  switch (row.kind) {
    case 'separator':
      return <TimeSeparator date={row.at} />
    case 'message':
      return (
        <VStack align="flex-start" gap={4} className={row.showsAuthor ? 'chat-row group-start' : 'chat-row'}>
          {row.showsAuthor ? <AuthorLabel botId={row.author} /> : null}
          <div className="message agent">
            <div className="message-body">
              <Bubble className="bubble agent selectable" items={() => [{ title: t('conversa.mensagens.copiar'), icon: 'square.on.square', action: () => window.codync.app.copy(row.text) }]}>
                <MarkdownText text={row.text} />
              </Bubble>
              {row.contract ? <ContratoTarefa contrato={row.contract} /> : null}
            </div>
          </div>
        </VStack>
      )
    case 'status': {
      const failed = row.outcome.kind === 'failed'
      const text = row.outcome.kind === 'failed' ? row.outcome.detail || t('conversa.mensagens.entreBots.naoConcluiu') : t('conversa.mensagens.entreBots.aguardando', { bot: awaitingName })
      return <Text as="div" size="footnote" tone={failed ? 'danger' : 'secondary'} className="selectable bot-conversation-status">{text}</Text>
    }
  }
}

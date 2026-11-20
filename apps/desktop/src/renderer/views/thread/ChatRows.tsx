// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { waitingLabel } from '@shared/subscription'
import { FileDownloadCard } from './FileDownloadCard'
import { memo, useEffect, useRef, useState, type ReactNode } from 'react'
import { groupBotReactions, isGroup, isReplyable, isWorking, migratedMemoryNote, needsInput, type Bot, type Entry, type ReplyQuote } from '@shared/models'
import { BotAvatar } from '../../components/Avatar'
import { Button, IconButton } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { AnchoredMenu, useContextMenu, type MenuItem, type ReactionPick } from '../../components/Overlay'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { pedidoAberto } from '../../lib/principal'
import { workingActivity } from '../fork/donna/DonnabotThinkingMark'
import { onlyQuestionsPending } from '@shared/fork-owner-question'
import { statusDoBalao, type ApprovalDecision } from './decisao'
import { WorkingDots } from '../fork/donna/WorkingDots'
import { messagePreview } from '../../lib/message-preview'
import { botAccent } from '@shared/fork-avatar-config'
import { useStore as useLiveStore, useStoreRef as useStore } from '../../store/context'
import { listEqual, useSelector } from '../../lib/observable'
import { QUICK_REACTIONS, type BotStore } from '../../store/bot-store'
import { AttachmentList } from './Attachments'
import { dataCompleta } from '../../lib/data-completa'
import { relativeTime } from './chat-items'
import { MarkdownText } from './Markdown'
import { PermissionCard } from './PermissionCard'
import { ConectorCard } from '../fork/conector-card/ConectorCard'
import { WhatsappDraftCard } from '../fork/conector-card/WhatsappDraftCard'
import { ConnectionRequestCard } from '../marketplace/ConnectionRequestCard'
import { OwnerQuestionCard } from '../fork/pergunta-dono/OwnerQuestionCard'
import { genuiChoiceRow } from '../fork/genui/GenuiChoiceRow'
import { HandoffNotice } from './HandoffNotice'
import { LinhaTroca } from '../fork/troca-modelo/LinhaTroca'
import { TeamChangeNotice } from './TeamChangeNotice'
import { MemoryNotice } from './MemoryNotice'
import { RascunhoDaConversa } from '../fork/skill-drafts/RascunhoDaConversa'
import { ToolSteps } from './cards/ToolSteps'
import type { TurnSteps } from './cards/tool-steps'
import type { WorkingBlock } from './working-blocks'
import { bubbleRect, ReceiptPopover, ReceiptTrigger } from '../fork/recibo/ReciboDoTurno'
import type { ReceiptAnchor } from '../fork/recibo/presentation'
import { AvisoDeEspera } from '../fork/conversa/LimitesDeUso'
import { alvoDaEspera, paradaDaEspera } from '../fork/conversa/limites'
import { superficieIdioma, tradutor, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { AcaoDaMensagemFalha, AvisoDeFalhaDeConta, BotaoAbrirContas } from '../fork/contas-ia/FalhaDeConta'
import { ehAvisoDePreparo, lerReason } from '../fork/contas-ia/falha-de-conta'
import { recusaOpen } from '../fork/spaces/erro-envio'

export function reactionPick(store: BotStore, entry: Entry): ReactionPick {
  return { emoji: QUICK_REACTIONS, chosen: entry.data.reactions ?? [], toggle: (e) => store.react(entry, e) }
}

/** One chat-visible entry, with what can be done to it. */
export const ChatRow = memo(function ChatRow({ entry, groupStart, chat, openTrace, onReply, steps, receipt }: {
  entry: Entry
  /** Rota-03b: a resposta que abre o turno, com onde ler o recibo dele. */
  receipt?: ReceiptAnchor
  groupStart: boolean
  /** D165 fase F: the steps the Hub recorded for this reply's turn (tool entries). */
  steps?: TurnSteps
  chat: Bot | null
  openTrace: () => void
  /** Quotes this message in the composer; absent where the computer can't take replies. */
  onReply?: (e: Entry) => void
}) {
  const reply = onReply && isReplyable(entry) ? () => onReply(entry) : undefined
  switch (entry.kind) {
    case 'user':
      return (
        <VStack align="flex-end" gap={4} className={groupStart ? 'chat-row group-start' : 'chat-row'}>
          <UserBubble entry={entry} botWorking={!!chat && isWorking(chat)} reply={reply} />
          <ReactionsRow entry={entry} />
        </VStack>
      )
    case 'agent':
      return (
        <VStack align="flex-start" gap={4} className={groupStart ? 'chat-row group-start' : 'chat-row'}>
          {/* D165 fase E + D169 (MessageBubble): name and time live inside the bubble; the bot's 28px face
              sits beside it at the start of its run, in every conversation. */}
          <AgentBubble entry={entry} openTrace={openTrace} reply={reply} meta={groupStart} face fallbackAuthor={chat} steps={steps} receipt={receipt} />
          <VStack gap={4} className="chat-row-indent">
            <ReactionsRow entry={entry} />
          </VStack>
        </VStack>
      )
    case 'permission':
      return (
        <VStack gap={4} className="chat-row group-start">
          {chat && isGroup(chat) ? <AuthorLabel botId={entry.data.author} /> : null}
          {entry.data.conectorOperation ? <ConectorCard entry={entry} /> : <LivePermission entry={entry} />}
        </VStack>
      )
    default:
      if (entry.data.conectorDraft) return <div className="chat-row group-start"><WhatsappDraftCard entry={entry} /></div>
      { const card = genuiChoiceRow(entry, chat, groupStart); if (card) return card } // G1b: pela projeção válida, sem data.card
      if (entry.data.card === 'ownerQuestion') return <div className={groupStart ? 'chat-row group-start' : 'chat-row'}><OwnerQuestionCard entry={entry} chat={chat} /></div>
      if (entry.data.connectionRequest) return <div className="chat-row group-start"><ConnectionRequestCard entry={entry} /></div>
      if (entry.data.teamChange) return <div className="chat-row notice"><TeamChangeNotice entry={entry} /></div>
      if (entry.data.modelSwitch) return <div className="chat-row notice"><LinhaTroca entry={entry} change={entry.data.modelSwitch} /></div>
      if (entry.data.handoff) return <div className="chat-row notice"><HandoffNotice entry={entry} card={entry.data.handoff} /></div>
      if (entry.data.memory || migratedMemoryNote(entry.data)) return <div className="chat-row notice"><MemoryNotice entry={entry} /></div>
      return <div className="chat-row notice"><NoticeRow entry={entry} /></div>
  }
})

/** Follows the store: the card shows a spinner while its answer is on its way. */
function LivePermission({ entry }: { entry: Entry }) {
  const store = useLiveStore()
  return <PermissionCard entry={entry} hostName={store.hostName} answering={store.answering.get(entry.id) ?? null} respond={(o) => store.respond(entry, o)} />
}

/** Who wrote a message in a group: their avatar and name in their color. */
export function AuthorLabel({ botId }: { botId?: string | null }) {
  const store = useStore()
  const [bot, name] = useSelector(store, (s) => [botId ? s.bots.get(botId) : undefined, s.authorName(botId)], listEqual)
  return (
    <HStack gap={8} className="chat-author">
      {bot ? <BotAvatar bot={bot} size={26} animated={false} /> : null}
      <Text size="subheadline" weight="medium" tone="secondary" style={bot ? { color: botAccent(bot) } : undefined}>{name}</Text>
    </HStack>
  )
}

/** The message a reply quotes, as its first line without Markdown markers: at the top of its bubble, and in the composer while replying. */
export function QuoteRow({ quote, className }: { quote: ReplyQuote; className?: string }) {
  return (
    <HStack gap={6} className={className ?? 'bubble-quote'}>
      <Icon name="text.bubble" size={12} color="var(--context-ink-2, var(--ink-2))" />
      <Text size="footnote" tone="secondary" truncate grow dir="auto">{`“${messagePreview(quote.text)}”`}</Text>
    </HStack>
  )
}

/** A message's text with a right-click menu (with quick reactions on top). */
export function Bubble({ className, items, reactions, children }: { className: string; items: () => MenuItem[]; reactions?: ReactionPick; children: React.ReactNode }) {
  const menu = useContextMenu()
  const idioma = useIdioma()
  return (
    <>
      <div className={className} onContextMenu={menu.onContextMenu}>
        {children}
      </div>
      <AnchoredMenu language={superficieIdioma(idioma)} open={menu.open} onClose={menu.close} point={menu.point} items={items} reactions={reactions} />
    </>
  )
}

/** The centered time label between runs of messages. */
export function TimeSeparator({ date }: { date: number }) {
  // A glass pill: nothing reads straight off the wallpaper (D165 fase E).
  const idioma = useIdioma()
  return <div className="time-separator"><Text size="footnote" tone="secondary" className="chat-pill glass-raised">{relativeTime.separator(tradutor(idioma), idioma, date)}</Text></div>
}

const fullDate = { format: dataCompleta } // Auditoria D249, F8: pt-BR, não o idioma do sistema.

export function UserBubble({ entry, botWorking, reply }: { entry: Entry; botWorking: boolean; reply?: () => void }) {
  const store = useStore()
  const [hovering, setHovering] = useState(false)
  const t = useT()
  const status = entry.data.status
  const copy = () => entry.data.text && window.codync.app.copy(entry.data.text)
  return (
    <div className="message user" title={fullDate.format(entry.createdAt)} onMouseEnter={() => setHovering(true)} onMouseLeave={() => setHovering(false)}>
      <div className="message-body">
        {entry.data.attachments?.length ? <AttachmentList attachments={entry.data.attachments} botId={entry.botId} /> : null}
        {entry.data.text || entry.data.replyTo ? (
          <Bubble
            className="bubble user selectable"
            reactions={reactionPick(store, entry)}
            items={() => [
              { title: t('conversa.mensagens.copiar'), icon: 'square.on.square', action: copy },
              ...(reply ? [{ title: t('conversa.mensagens.responder'), icon: 'arrowshape.turn.up.left', action: reply }] : []),
            ]}
          >
            {entry.data.replyTo ? <QuoteRow quote={entry.data.replyTo} /> : null}
            {entry.data.text}
          </Bubble>
        ) : null}
        {status && status !== 'sent' ? <span className="chat-status"><UserStatus entry={entry} botWorking={botWorking} hovering={hovering} /></span> : null}
        {/* Beside the bubble, inside its hover region: showing them never changes the height. */}
        <MessageActions className="leading" visible={hovering} reactions={reactionPick(store, entry)} reply={reply} copy={copy} />
      </div>
    </div>
  )
}

function UserStatus({ entry, botWorking, hovering }: { entry: Entry; botWorking: boolean; hovering: boolean }) {
  const store = useStore()
  const t = useT()
  switch (entry.data.status) {
    case 'sending':
      return <Text size="caption2" tone="tertiary">{t('conversa.mensagens.enviando')}</Text>
    case 'queued':
      return <Text size="caption2" tone="tertiary">{t(botWorking ? 'conversa.mensagens.naFilaAte' : 'conversa.mensagens.naFila')}</Text>
    case 'failed': {
      const reason = lerReason(entry.data.reason)
      const recusa = recusaOpen(entry.data.recusa)
      return (
        <HStack as="span" gap={2}>
          <Text size="caption2" weight="semibold" tone="danger" className="user-status-error">{t('conversa.mensagens.naoEnviada')}</Text>
          {recusa ? <Text size="caption2" tone="danger" role="alert">{t(recusa)}</Text> : null}
          {reason ? <AcaoDaMensagemFalha reason={reason} botId={entry.data.author ?? entry.botId} botName={store.bots.get(entry.data.author ?? entry.botId)?.name ?? ''} /> : null}
          <IconButton title={t('conversa.mensagens.reenviar')} icon="arrow.clockwise" size={24} onClick={() => store.retry(entry)} />
          <IconButton title={t('conversa.mensagens.apagar')} icon="trash" size={24} onClick={() => store.discard(entry)} />
        </HStack>
      )
    }
    case 'cancelled':
      return <Text size="caption2" tone="tertiary">{t('conversa.mensagens.interrompida')}</Text>
    default:
      return <Text size="caption" tone="tertiary" className={hovering ? 'user-status-time shown' : 'user-status-time'}>{relativeTime.time(entry.createdAt)}</Text>
  }
}

export function AgentBubble({ entry, openTrace, reply, meta = false, face = false, fallbackAuthor = null, steps, receipt }: { entry: Entry; openTrace: () => void; reply?: () => void; meta?: boolean; face?: boolean; fallbackAuthor?: Bot | null; steps?: TurnSteps; receipt?: ReceiptAnchor }) {
  const store = useStore()
  const [hovering, setHovering] = useState(false)
  const t = useT()
  // Rota-03b: "Como foi respondida" (Mais ações ou menu de contexto) abre o recibo junto de quem pediu.
  const [explain, setExplain] = useState<{ rect: DOMRect; from: HTMLElement | null } | null>(null)
  const row = useRef<HTMLDivElement>(null)
  const receiptBot = entry.data.author ?? entry.botId
  const explainItem = receipt ? [{ title: t('conversa.mensagens.comoFoi'), icon: 'info.circle', action: () => setExplain({ rect: bubbleRect(row.current!.querySelector('.message-body') ?? row.current!), from: null }) }] : []
  const copy = () => entry.data.text && window.codync.app.copy(entry.data.text)
  // A direct chat's replies may come without an author: they are the chat's own bot.
  const author = entry.data.author ? store.bots.get(entry.data.author) : fallbackAuthor && !isGroup(fallbackAuthor) ? fallbackAuthor : undefined
  return (
    <div ref={row} className={`message agent${face ? ' with-face' : ''}`} title={fullDate.format(entry.createdAt)} onMouseEnter={() => setHovering(true)} onMouseLeave={() => setHovering(false)}>
      {face ? <span className="message-face" aria-hidden>{meta && author ? <BotAvatar bot={author} size={28} animated={false} /> : null}</span> : null}
      <div className="message-body">
        <Bubble
          className={entry.data.files?.length ? "file-message" : "bubble agent"}
          reactions={reactionPick(store, entry)}
          items={() => [
            { title: t('conversa.mensagens.copiar'), icon: 'square.on.square', action: copy },
            ...(reply ? [{ title: t('conversa.mensagens.responder'), icon: 'arrowshape.turn.up.left', action: reply }] : []),
            { title: t('conversa.mensagens.verTrabalho'), icon: 'list.bullet', action: openTrace },
            ...explainItem,
          ]}
        >
          {meta ? (
            <div className="bubble-meta">
              {/* In `ink`, like the design's meta: a bot's own color could fall under 4.5:1 on glass. */}
              <span className="bubble-meta-name">{store.authorName(entry.data.author)}</span>
              <time className="bubble-meta-time">{relativeTime.time(entry.createdAt)}</time>
              {receipt ? <ReceiptTrigger anchor={receipt} botId={receiptBot} botName={store.authorName(entry.data.author)} /> : null}
            </div>
          ) : null}
          {/* D169 (ToolSteps): the work comes first, under the name, then the answer. */}
          {steps ? <ToolSteps steps={steps} /> : null}
          {entry.data.files?.length ? <div className="file-message-cards">{entry.data.files.map((file) => <FileDownloadCard key={file.id} entry={entry} file={file} />)}</div> : <MarkdownText text={entry.data.text ?? ''} />}
          {entry.kind === 'agent' && entry.data.final === true && !entry.data.files?.length ? <RascunhoDaConversa entry={entry} nomeAutor={author?.name} /> : null}
        </Bubble>
        <MessageActions className="trailing" visible={hovering} reactions={reactionPick(store, entry)} reply={reply} trace={openTrace} copy={copy}
          explain={receipt ? (from) => setExplain({ rect: bubbleRect(from), from }) : undefined} />
      </div>
      {receipt && explain ? <ReceiptPopover anchor={receipt} rect={explain.rect} botId={receiptBot} botName={store.authorName(entry.data.author)} returnFocus={explain.from} onClose={() => setExplain(null)} /> : null}
    </div>
  )
}

export function NoticeRow({ entry }: { entry: Entry }) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  // MX-245: a espera do limite (Hub antigo, sem `waitId`) também diz "Parou pelo limite do plano; volta às HH:MM".
  const text = paradaDaEspera(t, idioma, entry) ?? entry.data.text ?? ''
  if (entry.data.callSeconds !== undefined) {
    const s = entry.data.callSeconds
    return (
      <HStack justify="center" gap={6} className="chat-pill glass-raised chat-pill-center">
        <Icon name="waveform" size={10} color="var(--context-ink-2, var(--ink-2))" />
        <Text size="footnote" tone="secondary" tabular>{`${text} · ${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`}</Text>
      </HStack>
    )
  }
  // DON-907 C4: a espera do limite com "Não retomar"/"Retomar agora", quando o Hub anuncia a espera (fork/conversa).
  if (alvoDaEspera(entry)) return <AvisoDeEspera entry={entry} />
  switch (entry.data.style) {
    case 'error': {
      // Hub novo: `reason` vira a frase com a conta e o botão. Hub antigo: o texto de sempre; no aviso de preparo, só o botão para as contas.
      const reason = lerReason(entry.data.reason)
      const botId = entry.data.author ?? entry.botId
      return (
        <HStack align="flex-start" gap={8} className="chat-notice-error">
          <Icon name="exclamationmark.triangle.fill" size={10} color="var(--danger)" />
          {reason ? <AvisoDeFalhaDeConta reason={reason} botId={botId} botName={store.bots.get(botId)?.name ?? ''} /> : <Text size="footnote" tone="text" className="selectable">{text}</Text>}
          {!reason && ehAvisoDePreparo(text) ? <BotaoAbrirContas /> : null}
          {entry.data.subscriptionWait?.retry ? <>
            <Button kind="secondary" onClick={() => store.retrySubscription(entry)}>{t('conversa.mensagens.tentarDeNovo')}</Button>
            <Button kind="secondary" onClick={() => store.stop(entry.data.author ?? entry.botId)}>{t('conversa.mensagens.parar')}</Button>
          </> : null}
        </HStack>
      )
    }
    default:
      return <div className="chat-notice"><Text size="footnote" tone="secondary" className="chat-pill glass-raised">{text}</Text></div>
  }
}

/**
 * D169 (ToolSteps): the bot at work, as bot bubbles — one per author of pending steps, with that author's face
 * and name, then "Trabalhando · N passos" with the list open and the running step turning. Steps of an unknown
 * author get a neutral bubble, without a face or a name (revisão R1, F4). The activity line (what it's doing, for
 * how long, and the thinking that unfolds on click) shows when no step runs, while it waits, or when there are
 * thoughts: in a direct chat it belongs to the chat's bot; in a channel the Hub doesn't say which member it is, so
 * it stays neutral too. D169 (28/10, pedido do dono): the bot at work is the bot, never the app's mark — soft dots
 * under its face (WorkingDots), or before the line in a channel; a clock while paused, a raised hand while it waits.
 */
export function WorkingIndicator({ bot, thinking, blocks = [], approvalShown = false, decision = null }: { bot: Bot; thinking: string | null; blocks?: WorkingBlock[]; approvalShown?: boolean; decision?: ApprovalDecision }) {
  const t = useT()
  const idioma = useIdioma()
  const [expanded, setExpanded] = useState(false)
  const paused = bot.status === 'waiting'
  const waiting = needsInput(bot)
  const running = blocks.some((b) => b.steps.steps.some((s) => s.state === 'running'))
  const channel = isGroup(bot)
  // Uma autorização só: with the card on screen, the card is the only place to decide. The bubble keeps a short
  // status (not a button, no thinking toggle); once decided, it says what the person chose. All of it comes from
  // state (status, cards); the Hub's activity text is shown as it came (MX-147).
  const said = statusDoBalao(waiting, approvalShown, decision)
  const deciding = said === 'esperando'
  const asked = waiting ? pedidoAberto(bot, false, onlyQuestionsPending(bot)) : null
  const showActivity = !deciding && (!running || paused || waiting || !!thinking)
  // In a direct chat the activity is the chat's bot; it joins that bot's block, or gets its own bubble.
  const ownBlock = channel ? -1 : blocks.findIndex((b) => b.author?.id === bot.id)
  // Dots while it really works (not paused, not waiting for the person): under the chat bot's face, or inline in a channel.
  const busy = !paused && !waiting
  const activity = showActivity ? (
    <button className="working" disabled={!thinking} onClick={() => setExpanded((e) => !e)} aria-expanded={thinking ? expanded : undefined}>
      <HStack as="span" gap={8}>
        {paused ? <Icon name="clock" size={16} color="var(--context-ink-2, var(--ink-2))" />
          : waiting ? <Icon name="hand.raised" size={16} color="var(--text-attention, var(--attention))" />
            : channel ? <WorkingDots placement="inline" /> : null}
        <Text size="subheadline" tone={waiting ? 'attention' : 'secondary'} truncate>{paused ? waitingLabel(t, idioma, bot) : asked ? t(asked === 'pergunta' ? 'navegacao.barra.pergunta' : 'navegacao.barra.aprovacao') : workingActivity(t, idioma, bot)}</Text>
        {!paused && bot.startedAt ? <Timer since={bot.startedAt} /> : null}
        {thinking ? (
          <HStack as="span" align="stretch" className={expanded ? 'working-chevron expanded' : 'working-chevron'}>
            <Icon name="chevron.down" size={10} weight="semibold" color="var(--text-tertiary, var(--ink-3))" />
          </HStack>
        ) : null}
      </HStack>
      {expanded && thinking ? (
        <div className="selectable thinking" ref={(el) => {
          if (el) el.scrollTop = el.scrollHeight
        }}>
          {thinking}
        </div>
      ) : null}
    </button>
  ) : null
  // One live region that only changes its words: a screen reader says each status once. It isn't a button.
  const status = (
    <p className="working-status" role="status" dir="auto">
      {said ? <>{!deciding ? <Icon name={said === 'permitiu' ? 'checkmark' : 'xmark'} size={12} weight="semibold" color="var(--text-secondary, var(--ink-2))" /> : null}
        <Text size="subheadline" tone="secondary">{t(`conversa.aprovacao.status.${said}`)}</Text></> : null}
    </p>
  )
  // The region stays in the bubble (empty and hidden) so a new status is a change, not a new element.
  const line = <>{status}{activity}</>
  if (!said && !activity && blocks.length === 0) return null
  return (
    <div className="working-group">
      {blocks.map((block, i) => (
        <WorkingBubble key={block.authorId} author={block.author} since={i === ownBlock ? bot.startedAt ?? null : null} busy={i === ownBlock && busy}>
          <ToolSteps steps={block.steps} live />
          {i === ownBlock ? line : null}
        </WorkingBubble>
      ))}
      {(said || activity) && ownBlock < 0 ? <WorkingBubble author={channel ? null : bot} since={channel ? null : bot.startedAt ?? null} busy={!channel && busy}>{line}</WorkingBubble> : null}
    </div>
  )
}

/** A live bot bubble: the author's face and name when known; neutral (no face, no name) when not. `busy`: the dots under the face. */
function WorkingBubble({ author, since, busy = false, children }: { author: Bot | null; since: number | null; busy?: boolean; children: ReactNode }) {
  const store = useStore()
  return (
    <div className="message agent with-face working-message">
      <span className={`message-face${author && busy ? ' working-face' : ''}`} aria-hidden>
        {author ? <BotAvatar bot={author} size={28} /> : null}
        {author && busy ? <WorkingDots placement="face" /> : null}
      </span>
      <div className="message-body">
        <div className="bubble agent working-bubble">
          {author ? (
            <div className="bubble-meta">
              <span className="bubble-meta-name">{store.authorName(author.id)}</span>
              {since ? <time className="bubble-meta-time">{relativeTime.time(since)}</time> : null}
            </div>
          ) : null}
          {children}
        </div>
      </div>
    </div>
  )
}

/** `Text(date, style: .timer)`: 0:42, 12:05, 1:02:33. */
function Timer({ since }: { since: number }) {
  const [now, setNow] = useState(Date.now())
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(timer)
  }, [])
  const s = Math.max(0, Math.floor((now - since) / 1000))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = String(s % 60).padStart(2, '0')
  return <Text size="footnote" tone="tertiary" tabular>{h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`}</Text>
}

/** A message's reactions: the user's (clicking one takes it back), then the bots', each emoji with the avatars of the
 * bots that chose it. */
export function ReactionsRow({ entry }: { entry: Entry }) {
  const store = useStore()
  const t = useT()
  const mine = entry.data.reactions ?? []
  const theirs = groupBotReactions(entry.data.botReactions ?? [])
    .map(({ emoji, bots }) => ({ emoji, bots: bots.map((id) => store.bots.get(id)).filter((b): b is Bot => !!b) }))
    .filter((g) => g.bots.length)
  if (!mine.length && !theirs.length) return null
  return (
    <HStack align="stretch" gap={4}>
      {mine.map((emoji) => (
        <button key={emoji} className="press reaction-chip" title={t('conversa.mensagens.tirar', { emoji })} aria-label={t('conversa.mensagens.tirarReacao', { emoji })} onClick={() => store.react(entry, emoji)}>
          <Text size="body">{emoji}</Text>
        </button>
      ))}
      {theirs.map(({ emoji, bots }) => {
        const label = t('conversa.mensagens.reagiu', { bots: bots.map((b) => b.name).join(', '), emoji })
        return (
          <HStack key={`bots-${emoji}`} gap={4} className="reaction-chip" title={label} aria-label={label} role="img">
            <Text size="body">{emoji}</Text>
            <HStack as="span" className="reaction-chip-bots">
              {bots.slice(0, 3).map((b) => (
                <span key={b.id}>
                  <BotAvatar bot={b} size={24} animated={false} />
                </span>
              ))}
            </HStack>
          </HStack>
        )
      })}
    </HStack>
  )
}

/** Compact actions beside the bubble: a reaction strip, reply, and more (copy, what it did). */
function MessageActions({ visible, className, reactions, reply, trace, copy, explain }: {
  /** Rota-03b: abre o recibo do turno junto do botão Mais ações. */
  explain?: (from: HTMLElement) => void
  visible: boolean
  className: string
  reactions: ReactionPick
  reply?: () => void
  trace?: () => void
  copy: () => void
}) {
  const [menu, setMenu] = useState<'reactions' | 'more' | null>(null)
  const t = useT()
  const idioma = useIdioma()
  const smile = useRef<HTMLButtonElement>(null)
  const more = useRef<HTMLButtonElement>(null)
  const shown = visible || menu !== null
  return (
    // Focusable, so never aria-hidden: keyboard focus shows the strip (thread.css, `:has(:focus-visible)`).
    <div className={`message-actions ${className} ${shown ? 'shown' : ''}`}>
      <button ref={smile} className="message-action" title={t('conversa.mensagens.reagir')} aria-label={t('conversa.mensagens.reagir')} onClick={() => setMenu('reactions')}>
        <Icon name="face.smiling" size={14} />
      </button>
      {reply ? <IconButton title={t('conversa.mensagens.responder')} icon="arrowshape.turn.up.left" size={26} onClick={reply} /> : null}
      <button ref={more} className="message-action" title={t('conversa.mensagens.maisAcoes')} aria-label={t('conversa.mensagens.maisAcoes')} onClick={() => setMenu('more')}>
        <Icon name="ellipsis" size={14} />
      </button>
      <AnchoredMenu language={superficieIdioma(idioma)} open={menu === 'reactions'} onClose={() => setMenu(null)} anchor={smile} items={() => []} reactions={reactions} />
      <AnchoredMenu language={superficieIdioma(idioma)}
        open={menu === 'more'}
        onClose={() => setMenu(null)}
        anchor={more}
        items={() => [{ title: t('conversa.mensagens.copiar'), icon: 'square.on.square', action: copy }, ...(trace ? [{ title: t('conversa.mensagens.verTrabalho'), icon: 'list.bullet', action: trace }] : []), ...(explain ? [{ title: t('conversa.mensagens.comoFoi'), icon: 'info.circle', action: () => explain(more.current!) }] : [])]}
      />
    </div>
  )
}

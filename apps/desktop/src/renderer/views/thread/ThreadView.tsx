// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { focoBerga } from '../fork/atalho-berga/foco'
import { Fragment, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { isGroup, isWorkingIn, type Bot, type Entry } from '@shared/models'
import { BotAvatar, GroupAvatar } from '../../components/Avatar'
import { Spinner } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Sheet, usePresence } from '../../components/Overlay'
import { HStack, IconSlot } from '../../components/Stack'
import { Text } from '../../components/Text'
import { useScreenViewed } from '../../store/analytics'
import { chatEntriesOf, useStore, useStoreRef } from '../../store/context'
import { UpdateNeededCard } from '../UpdateNeededCard'
import { chatView, relativeTime, rowIn, useArrivals, type ChatItem } from './chat-items'
import { lastDecision } from './decisao'
import { ChatTimeline } from './ChatTimeline'
import type { TurnSteps } from './cards/tool-steps'
import { stableAnchors, type ReceiptAnchor } from '../fork/recibo/presentation'
import type { TimelineMessage } from './chat-timeline'
import { BotConversationView, BotMessageRow } from './BotConversation'
import { ChatRow, TimeSeparator, WorkingIndicator } from './ChatRows'
import { MarcaCompactando, useCompactacao } from '../fork/contexto/Compactando'
import { workingBlocks } from './working-blocks'
import { Composer } from './Composer'
import { DetailsPanel, type DetailsTab } from './DetailsPanel'
import { TraceView } from './TraceView'
import { useReading } from './reading'
import { GroupEditorView } from '../bots/GroupEditorView'
import { CallView } from '../call/CallView'
import { useCallAvailability } from '../call/use-call-availability'
import { TEXTO_RECUSA, useEntradaVoz } from '../fork/voz/entrada-voz'
import { afterChange, jumpPill, markKey } from '../fork/novas/leitura'
import { useVisibleRows } from '../fork/novas/visible-rows'
import { useAbrirNaLinha, useVisita } from '../fork/novas/useVisita'
import { reduceMotion } from '../../lib/theme'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import './thread.css'
import './composer.css'
import './chat-controls.css'
import { ptBackend } from '../../lib/textos-pt'
import { routinesScreen } from '../rotinas-central/navigation'
import { routineNoticeText } from './notice-label'
import { returnFocusAfterClose } from './details-focus'
import { FaixaReconectando } from '../fork/conexao/FaixaReconectando'
import { emQueda } from '../../store/fork-queda-curta'
import '../fork/donna/chat-identity.css'
import './janela-baixa.css'
// Fork (Barra05): abas no topo, painel direito e sessões, em módulos próprios (D71).
import { ChatTabs } from '../fork/abas/ChatTabs'
import { tabMemory } from '../fork/abas/memoria'
import { PainelDireito, useRightPanel } from '../fork/painel-direito/PainelDireito'
import { PANEL_WIDTH } from '../fork/painel-direito/prefs'
import { useChatSessions, type ChatSessions } from '../fork/sessoes/useChatSessions'
import { ConversaEncerrada, useEncerrar } from '../fork/conversa/arquivar/Encerrada'
import { extrasAllowed, SessionComposerNotice, SessionFailureBanner, useConsumeNewRequest, useConsumePanelRequest, useNewSession, useOpenFocused } from '../fork/sessoes/ganchos'
import { RotuloModelo } from '../fork/sessoes/RotuloModelo'
import { listEqual, useModel, useSelector } from '../../lib/observable'
import { bloqueioSenha, destinoSenha } from '../../store/fork-senha-bloqueio'
import { TelaGrande, useTelaGrande } from '../fork/whatsapp/TelaGrande'
import { SuperficieAVista } from '../fork/whatsapp/superficie'

export interface ThreadPanelScope {
  panelId: string
  active: boolean
  initialSessionId?: string | null
  onSessionChange?: (sessionId: string | null) => void
}

/**
 * One endless conversation with a bot or a group chat. Only deliberate messages show here; tool
 * calls and thinking live in the "Full conversation" sheet. Replying to a message quotes it in the composer.
 */
export const ThreadView = memo(function ThreadView({ botId, panelScope }: { botId: string; panelScope?: ThreadPanelScope }) {
  const store = useStoreRef()
  // MX-4 (C9): só a fatia que a conversa desenha acorda o componente — eventos de outras
  // conversas ou campos internos (rascunhos, filas) não renderizam mais a tela aberta.
  useSelector(store, (s) => {
    const b = s.bots.get(botId) ?? null
    return [
      b, chatEntriesOf(s, botId), s.connection, s.shownConnection.kind,
      s.mismatch?.kind ?? null, s.hostVersion, s.linkCaidoDesde, s.spaces, s.hello,
      ...(b && isGroup(b) ? s.members(b) : []),
    ]
  }, listEqual)
  const t = useT()
  const idioma = useIdioma()
  const bot = store.bots.get(botId) ?? null
  const disconnected = store.shownConnection.kind !== 'online' || !!store.mismatch
  const queda = emQueda(store)
  const [showTrace, setShowTrace] = useState(false)
  const [replyTo, setReplyTo] = useState<Entry | null>(null)
  const cancelReply = useCallback(() => setReplyTo(null), [])
  const [botChat, setBotChat] = useState<string | null>(null)
  const [editingGroup, setEditingGroup] = useState(false)
  const [detailsTab, setDetailsTab] = useState<DetailsTab>('sessions')
  // A sessão aberta no chat (null = a conversa principal) e as abas fechadas com "×", por conversa ou por painel.
  const inPanel = !!panelScope
  const panelActive = !panelScope || panelScope.active
  const panelActiveRef = useRef(true)
  panelActiveRef.current = panelActive
  const onSessionChangeRef = useRef(panelScope?.onSessionChange)
  onSessionChangeRef.current = panelScope?.onSessionChange
  const panelMemory = useRef<{ open: string | null; closed: Set<string> } | null>(null)
  const panelMemoryId = useRef<string | null>(panelScope?.panelId ?? null)
  if (panelScope && panelMemoryId.current !== panelScope.panelId) {
    panelMemoryId.current = panelScope.panelId
    panelMemory.current = { open: panelScope.initialSessionId ?? null, closed: new Set() }
  }
  const memory = inPanel
    ? (panelMemory.current ??= { open: panelScope.initialSessionId ?? null, closed: new Set() })
    : tabMemory(botId)
  const [openSession, setOpenSessionState] = useState<string | null>(memory.open)
  const [closedTabs, setClosedTabs] = useState<ReadonlySet<string>>(memory.closed)
  const setOpenSession = useCallback((id: string | null) => {
    memory.open = id
    setOpenSessionState(id)
    // A citação é da aba em que foi escolhida: trocar de aba não a leva junto.
    setReplyTo(null)
    onSessionChangeRef.current?.(id)
  }, [memory])
  const prevInitialSession = useRef(panelScope?.initialSessionId)
  useEffect(() => {
    if (!inPanel) return
    const next = panelScope?.initialSessionId ?? null
    if (prevInitialSession.current === next) return
    prevInitialSession.current = next
    if (next === openSession) return
    memory.open = next
    if (next && memory.closed.has(next)) {
      const nextClosed = new Set(memory.closed)
      nextClosed.delete(next)
      memory.closed = nextClosed
      setClosedTabs(nextClosed)
    }
    setOpenSessionState(next)
    setReplyTo(null)
  }, [inPanel, panelScope?.initialSessionId, openSession, memory])
  const sessions = useChatSessions(store, bot)
  const sessionsModel = useModel(store.sessions)
  // MX-214: com o aviso de senha colada aberto, as entradas de chamada (Ao vivo, painel) ficam desligadas.
  const vozBloqueada = useModel(bloqueioSenha).bloqueado(destinoSenha(store.computer.id, botId))
  useEffect(() => {
    if (openSession && sessionsModel.isDeleted(botId, openSession)) {
      setOpenSession(null)
      root.current?.querySelector<HTMLButtonElement>('.ctab.main')?.focus({ preventScroll: true })
    }
  }, [sessionsModel, sessionsModel.getVersion(), botId, openSession, setOpenSession])
  const [calling, setCalling] = useState(false)
  const [callSpeaking, setCallSpeaking] = useState(false)
  const interruptCall = useRef<(() => void) | null>(null)
  const [routineId, setRoutineId] = useState<string | null>(null)
  const [routineRequest, setRoutineRequest] = useState(0)
  const root = useRef<HTMLDivElement>(null)
  const dock = useRef<HTMLDivElement>(null)
  // The width is measured, not rendered: resizing re-renders only when `roomy` flips.
  const width = useRef(800)
  // Lugar para o chat (380 px) e o painel fixo lado a lado; senão o painel abre por cima.
  const [roomy, setRoomy] = useState(true)
  const panel = useRightPanel(roomy)
  useReading(store, botId)
  // The chat window remounts this per conversation (`key`), so each one opened counts.
  useScreenViewed('chat')

  useLayoutEffect(() => {
    const el = root.current
    if (!el) return
    const observer = new ResizeObserver(([e]) => {
      const w = e!.contentRect.width
      width.current = w
      setRoomy(w >= 380 + PANEL_WIDTH + 8)
    })
    observer.observe(el)
    // The composer floats over the transcript; its height becomes the transcript's bottom padding.
    const docked = new ResizeObserver(([e]) => el.style.setProperty('--composer-height', `${Math.ceil(e!.borderBoxSize[0]!.blockSize)}px`))
    if (dock.current) docked.observe(dock.current)
    return () => {
      observer.disconnect()
      docked.disconnect()
    }
  }, [])

  const voiceAvailable = useCallAvailability(store, panel.open)
  // o atalho global abre ou encerra a MESMA chamada do botão, só na conversa principal de um bot à vista.
  const recusaVoz = useEntradaVoz(
    () => ({ grupo: !!bot && isGroup(bot), coberta: telaOn || (panel.open && !panel.pinned) || !panelActiveRef.current, sessaoAberta: openSession !== null, disponivel: voiceAvailable && !!bot, senhaBloqueada: vozBloqueada, emChamada: calling }),
    () => setCalling(true),
    () => setCalling(false),
  )
  const { togglePin, show: showPanel } = panel
  const togglePinRef = useRef(togglePin)
  togglePinRef.current = togglePin

  // ⌘I (menu "Pin Conversation Panel") fixa ou solta o painel.
  useEffect(
    () =>
      window.codync.app.onCommand((c) => {
        if (c.kind === 'toggleDetails' && !routinesScreen.open && panelActiveRef.current) togglePinRef.current()
      }),
    [],
  )
  // Dois cliques na barra, ⌃↩ ou "Sessões de …" (store.openSessionsPanel): o painel abre em Sessões.
  useConsumePanelRequest(store, panelActive ? botId : '', (tab) => {
    setDetailsTab(tab)
    showPanel()
  })

  // Fechar o painel solto devolve o foco à aba do bot, se ele ficou perdido no painel (D169 R6).
  const peekWasOpen = useRef(false)
  useEffect(() => {
    // Fixar não é fechar: o painel continua à mostra.
    const closing = peekWasOpen.current && !panel.peekOpen && !panel.pinned
    peekWasOpen.current = panel.peekOpen
    if (!closing) return
    const target = root.current?.querySelector<HTMLElement>('.ctab.main')
    return target ? returnFocusAfterClose(target) : undefined
  }, [panel.peekOpen, panel.pinned])

  // Esc volta para a conversa principal (as camadas por cima, como o painel solto, fecham antes).
  useEffect(() => {
    if (openSession === null) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || e.defaultPrevented || root.current?.closest('[inert]')) return
      if (inPanel && !root.current?.contains(e.target as Node | null) && !root.current?.contains(document.activeElement)) return
      e.preventDefault()
      setOpenSession(null)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [openSession, setOpenSession, inPanel])

  const closeTab = useCallback((id: string) => {
    const next = new Set(memory.closed)
    next.add(id)
    memory.closed = next
    setClosedTabs(next)
    if (openSession === id) setOpenSession(null)
  }, [memory, openSession, setOpenSession])
  const openTab = useCallback((id: string | null) => {
    if (id && memory.closed.has(id)) {
      const next = new Set(memory.closed)
      next.delete(id)
      memory.closed = next
      setClosedTabs(next)
    }
    setOpenSession(id)
  }, [memory, setOpenSession])

  // D548: abre a aba principal inclusive se uma sessão extra estava selecionada; preserva os rascunhos.
  const bergaRequest = useModel(focoBerga).request
  useEffect(() => {
    if (!panelActive || !bergaRequest || bergaRequest.computerId !== store.computer.id || bergaRequest.botId !== botId) return
    focoBerga.clear()
    openTab(null)
    requestAnimationFrame(() => root.current?.querySelector<HTMLTextAreaElement>('.composer textarea')?.focus({ preventScroll: true }))
  }, [panelActive, bergaRequest, store, botId, openTab])

  // UI-01: "+", "Nova sessão" do painel e do menu do bot criam no Hub e abrem a aba devolvida, sem enviar nada.
  const newSession = useNewSession(store, bot, useOpenFocused(openTab, openSession, root))
  useConsumeNewRequest(store, panelActive ? botId : '', panelActive ? (newSession?.run ?? null) : null)
  const extras = extrasAllowed(store, bot)
  const openRow = openSession === null ? null : store.sessions.of(botId)?.find((s) => s.id === openSession) ?? null
  const canWriteExtra = extras && openRow !== null && openRow.state === 'open' && sessionsModel.can(botId, 'send')
  // DON-907 C5: encerrar pela aba e a conversa encerrada com "Continuar" (fork/conversa/arquivar).
  const encerrar = useEncerrar(store, bot)
  const failure = sessionsModel.failure?.chatId === botId && (sessionsModel.failure.sessionId === null || sessionsModel.failure.sessionId === openSession) ? sessionsModel.failure : null

  // WhatsApp R5c: a tela grande do mini WhatsApp no centro (Expandir/Recolher) e a conversa aberta nela e no painel.
  const tela = useTelaGrande(panel, root, () => setDetailsTab('whatsapp'), () => openTab(null))
  const telaOn = tela.controle.expandida && !!bot

  const openTrace = useCallback(() => setShowTrace(true), [])

  const presentRoutine = useCallback((id: string | null) => {
    setRoutineId(id)
    setRoutineRequest((r) => r + 1)
    setDetailsTab('routines')
    showPanel()
  }, [showPanel])

  const details = (
    <DetailsPanel
      key={routineRequest}
      botId={botId}
      routineId={routineId}
      tab={detailsTab}
      setTab={setDetailsTab}
      active={panel.open}
      sessions={sessions}
      openSession={openSession}
      onOpenSession={openTab}
      onNewSession={newSession}
      pinned={panel.pinned}
      onTogglePin={togglePin}
      onCall={voiceAvailable && !calling && !vozBloqueada && bot && !isGroup(bot) ? () => setCalling(true) : null}
      editGroup={() => setEditingGroup(true)}
      whatsapp={tela.controle}
    />
  )

  return (
    <div className="thread-view" ref={root}>
      {/* D387: com a tela grande do WhatsApp aberta, a conversa fica escondida por CSS; número revelado nela esconde. */}
      <SuperficieAVista.Provider value={!telaOn}>
      <div className={`conversation${telaOn && tela.fase !== 'abrindo' ? ' wa-x' : ''}`}>
        {/* Barra05 (passo 14): o cartão de abas no lugar do rosto e nome centralizados (components/ChatHeader). */}
        <div className="top-chrome">
          {bot ? (
            <ChatTabs chat={bot} channel={isGroup(bot)} sessions={sessions} openId={openSession} closed={closedTabs} hidden={calling}
              onOpen={openTab} onCloseTab={closeTab} onTogglePin={togglePin}
              onMore={() => {
                setDetailsTab('sessions')
                showPanel()
              }}
              onNew={newSession} onTabMenu={encerrar.abrir} menuSessionId={encerrar.menuId} />
          ) : null}
          {/* MX-217: numa queda curta, a faixa com Tentar agora no lugar do selo; a conversa fica. */}
          {queda ? <FaixaReconectando /> : disconnected ? <div className="ctabs-connection"><ConnectionSubtitle /></div> : null}
          {/* UI01b / D372: só o rótulo do modelo da conversa aberta; o seletor único é da C9, no compositor. */}
          {!calling ? <RotuloModelo store={store} chat={bot} sessionId={openSession} /> : null}
        </div>
        {/* An update card takes the composer's place, so there's nothing to reply with. */}
        <Transcript key={markKey(botId, openSession)} botId={botId} sessionId={openSession} sessions={sessions} openTrace={openTrace} onReply={store.mismatch ? undefined : setReplyTo} openRoutine={presentRoutine} openBotChat={setBotChat} />
        <div className="composer-dock" ref={dock}>
          <div className="composer-fade" />
          {encerrar.aviso}
          {store.mismatch ? (
            <div className="composer-update-needed">
              <UpdateNeededCard />
            </div>
          ) : openRow?.state === 'closed' && extras ? (
            <ConversaEncerrada store={store} chat={bot} session={openRow} botName={bot?.name ?? ''} onBack={() => setOpenSession(null)} onOpen={openTab} />
          ) : openSession !== null && !canWriteExtra ? (
            <SessionComposerNotice botName={bot && isGroup(bot) ? `#${bot.name.toLocaleLowerCase('pt-BR')}` : bot?.name ?? ''} closed={openRow?.state === 'closed'} onBack={() => setOpenSession(null)} />
          ) : (
            <>
              {failure ? <SessionFailureBanner notice={failure} onDismiss={() => store.sessions.dismiss(botId)} /> : null}
              <Composer
                botId={botId}
                sessionId={openSession}
                reply={replyTo}
                cancelReply={cancelReply}
                onCall={openSession === null && voiceAvailable && !calling && !vozBloqueada && bot ? () => setCalling(true) : null}
                onInterrupt={openSession === null && callSpeaking ? () => interruptCall.current?.() : null}
              />
            </>
          )}
        </div>
        {recusaVoz && recusaVoz !== 'oculta' && !calling ? (
          <div className="call-overlay">
            <Text as="div" tone="secondary" className="call-line" role="status" dir="auto">{t(TEXTO_RECUSA[recusaVoz])}</Text>
          </div>
        ) : null}
        {calling ? (
          <div className="call-overlay">
            <CallView botId={botId} onSpeaking={setCallSpeaking} interrupt={interruptCall} onEnd={() => setCalling(false)} />
          </div>
        ) : null}
        {telaOn ? <TelaGrande bot={bot} controle={tela.controle} fase={tela.fase} /> : null}
      </div>
      </SuperficieAVista.Provider>
      {encerrar.camada}
      <PainelDireito panel={panel} attention={sessions.combined === 'needsYou'} label={bot ? t('conversa.mensagens.painelDe', { bot: bot.name }) : t('conversa.mensagens.painel')}>
        {details}
      </PainelDireito>
      <Sheet language={superficieIdioma(idioma)} open={botChat !== null} onClose={() => setBotChat(null)} width={620} height={560}>
        {botChat ? <BotConversationView botId={botId} peerId={botChat} /> : null}
      </Sheet>
      <Sheet language={superficieIdioma(idioma)} open={showTrace} onClose={() => setShowTrace(false)} width={620} height={560}>
        <TraceView botId={botId} />
      </Sheet>
      <Sheet language={superficieIdioma(idioma)} open={editingGroup} onClose={() => setEditingGroup(false)} width={420} height={560}>
        {bot ? <GroupEditorView group={bot} /> : null}
      </Sheet>
    </div>
  )
})

function ConnectionSubtitle() {
  const store = useStoreRef()
  // MX-4 (C9): o subtítulo só segue conexão, rota, nome do computador e identidade do Hub.
  useSelector(store, (s) => [s.connection, s.shownConnection.kind, s.mismatch?.kind ?? null, s.hostRoute, s.hello, s.hostName, s.connectionLabel], listEqual)
  const t = useT()
  const connecting = store.shownConnection.kind === 'connecting'
  const symbol = store.connection.kind !== 'online' ? 'wifi.slash' : store.hostRoute === 'relay' ? 'cloud' : store.hostRoute === 'direct' ? 'wifi' : store.hostRoute === 'loopback' ? 'desktopcomputer' : 'network'
  const description = (() => {
    switch (store.shownConnection.kind) {
      case 'connecting':
        return t('conversa.mensagens.conexao.conectando')
      case 'computerOffline':
      case 'offline':
        return t('conversa.mensagens.conexao.desconectado')
      case 'unauthorized':
        return t('conversa.mensagens.conexao.semAcesso')
      case 'unpaired':
        return t('conversa.mensagens.conexao.naoPareado')
    }
    if (store.mismatch) return t('conversa.mensagens.conexao.atualizar')
    return t(store.hostRoute === 'relay' ? 'conversa.mensagens.conexao.cloudflare' : store.hostRoute === 'direct' ? 'conversa.mensagens.conexao.direta' : store.hostRoute === 'loopback' ? 'conversa.mensagens.conexao.local' : 'conversa.mensagens.conexao.conectado')
  })()
  return (
    <HStack as="span" gap={6} className="title-pill-subtitle" title={`${store.hostName} · ${description}`} aria-label={`${store.hostName}, ${description}`}>
      <IconSlot width={12}>{connecting ? <Spinner size={8} /> : <Icon name={symbol} size={8} weight="medium" />}</IconSlot>
      <Text size="caption" weight="medium" truncate>{store.connectionLabel}</Text>
    </HStack>
  )
}

/** The messages: every loaded message, brought to the newest as it changes while the reader is at the bottom. */
const PAGE = 40

const Transcript = memo(function Transcript({ botId, sessionId, sessions, openTrace, onReply, openRoutine, openBotChat }: { botId: string; sessionId: string | null; sessions: ChatSessions; openTrace: () => void; onReply?: (e: Entry) => void; openRoutine: (id: string | null) => void; openBotChat: (peerId: string) => void }) {
  const store = useStoreRef()
  // MX-4 (C9): a fatia do transcript — esta conversa, o bot, a conexão e os autores das linhas.
  useSelector(store, (s) => [
    s.bots.get(botId) ?? null, chatEntriesOf(s, botId), s.isOffline, s.hello,
    s.canLoadOlder(botId), s.streamRev, ...s.bots.values(), ...s.answering.keys(), ...s.answering.values(),
  ], listEqual)
  const t = useT()
  const bot = store.bots.get(botId) ?? null
  // Fork (Barra05): numa sessão, só as entradas dela, e o "trabalhando" só se ela é a que roda.
  const here = sessionId === null || sessions.state(sessionId) === 'running' || sessions.state(sessionId) === 'needsYou'
  const live = here && !!bot && (isWorkingIn(bot, botId) || (bot.status === 'waiting' && (bot.workingChat ?? bot.id) === botId)) && !store.isOffline
  // "Compactando…": a marca no fluxo no lugar do "trabalhando" genérico; concluída e erro calmo (só do pedido manual) aparecem no mesmo lugar, enquanto a tela está aberta.
  const compactacao = useCompactacao(bot, botId, sessionId)
  // Rebuilt only when this chat changes: the store replaces a chat's list on every change.
  const chatEntries = store.allEntries(botId)
  // MX-4 (C10): sessão, fila, itens, passos e recibos vêm do cache por lista (chatView) — a remontagem da
  // troca de conversa não refaz o pipeline. DON-907: as que esperam na fila ficam na bandeja, fora da conversa.
  const view = useMemo(() => chatView(chatEntries, botId, sessionId, sessions.known), [chatEntries, botId, sessionId, sessions.known])
  const all = view.items
  const steps = view.steps
  // Rota-03b: a resposta que abre cada turno e a entrada do usuário onde o Hub guarda o recibo.
  const lastReceipts = useRef(new Map<string, ReceiptAnchor>())
  const receipts = useMemo(() => (lastReceipts.current = stableAnchors(lastReceipts.current, view.anchors)), [view])
  // The newest 40 items first; older pages come in near the top.
  const [firstShown, setFirstShown] = useState<string | null>(null)
  const [loadingEarlier, setLoadingEarlier] = useState(false)
  const found = firstShown ? all.findIndex((i) => i.id === firstShown) : -1
  const start = found >= 0 ? found : all.length > PAGE ? all.length - PAGE : 0
  const items = all.slice(start)
  // D249 R3: a pending approval card on screen already says the bot waits for the person.
  const approvalShown = items.some((i) => i.kind === 'entry' && i.entry.kind === 'permission' && i.entry.data.status === 'pending')
  const decision = lastDecision(items)
  const more = start > 0 || store.canLoadOlder(botId)
  // Following the newest message, or reading history: scrolling up releases the bottom,
  // arriving back at the end (or Jump to latest) restores it. Content growth alone never changes it.
  const [following, setFollowing] = useState(true)
  const followingRef = useRef(true)
  followingRef.current = following
  const scroller = useRef<HTMLDivElement>(null)
  const lastScrollTop = useRef(0)
  const keepPlace = useRef<number | null>(null)
  const last = items[items.length - 1]
  const arrivals = useArrivals([...items.map((i) => i.id), ...(live ? ['working'] : [])], following)
  // D432 (views/fork/novas): a visita (marca local, novas congeladas, leitura em sequência) fica no módulo do fork.
  const seenList = useMemo(() => all.flatMap((i) => (i.kind === 'entry' && (i.entry.kind === 'user' || i.entry.kind === 'agent') ? [{ id: i.id, at: i.entry.createdAt, mine: i.entry.kind === 'user', rev: i.entry.rev }] : [])), [all])
  const visible = useVisibleRows(scroller, `${items.length}:${items[0]?.id ?? ''}:${last?.id ?? ''}`)
  const openSessions = useMemo(() => (sessions.principal ? new Set(sessions.open.map((x) => x.id)) : null), [sessions.principal, sessions.open])
  const { unseen, lineId, below } = useVisita({ botId, sessionId, messages: seenList, unread: bot?.unread ?? 0, following, streamRev: store.streamRev, hasOlder: store.canLoadOlder(botId), visible, openSessions })
  const pill = jumpPill(following, items.length > 0, below)
  const jump = usePresence(pill.shown, 200)
  const timeline = useMemo<TimelineMessage[]>(() => {
    const messages = items.flatMap((i) => (i.kind === 'entry' && (i.entry.kind === 'user' || i.entry.kind === 'agent') ? [{ id: i.id, entry: i.entry }] : []))
    return messages.map(({ id, entry }) => ({ id, time: relativeTime.time(entry.createdAt), text: entry.data.text ?? '', mine: entry.kind === 'user', unread: unseen.has(id) }))
  }, [items, unseen])

  const toBottom = (smooth = false) => {
    const el = scroller.current
    if (el) el.scrollTo({ top: el.scrollHeight, behavior: smooth ? 'smooth' : 'auto' })
  }

  useEffect(() => {
    if (firstShown || !items[0]) return
    // A 1ª nova antes das 40 mais recentes: a página começa um pouco antes dela (só o que o app já tem; o que o Hub
    // ainda não mandou não é buscado aqui, e aí a linha fica na 1ª carregada).
    const line = lineId ? all.findIndex((i) => i.id === lineId) : -1
    setFirstShown(line >= 0 && line < start ? all[Math.max(0, line - 2)]!.id : items[0].id)
  }, [firstShown, items, lineId])

  useAbrirNaLinha(scroller, lineId, items.length > 0 && !!firstShown, followingRef, () => setFollowing(false))

  const showEarlier = async () => {
    if (loadingEarlier) return
    setLoadingEarlier(true)
    const el = scroller.current
    keepPlace.current = el ? el.scrollHeight - el.scrollTop : null
    let list = all
    let at = start
    if (at === 0) {
      const anchor = list[0]?.id
      await store.loadOlder(botId)
      list = chatView(store.allEntries(botId), botId, sessionId, sessions.known).items
      at = anchor ? Math.max(0, list.findIndex((i) => i.id === anchor)) : 0
    }
    if (at > 0) setFirstShown(list[Math.max(0, at - PAGE)]!.id)
    setLoadingEarlier(false)
  }

  // Earlier messages going in above keep the reader's place; a follower stays at the bottom.
  useLayoutEffect(() => {
    const el = scroller.current
    if (!el) return
    const action = afterChange(followingRef.current, keepPlace.current !== null)
    if (action === 'restore') {
      el.scrollTop = el.scrollHeight - keepPlace.current!
      keepPlace.current = null
    } else if (action === 'bottom') toBottom()
  })

  // A message the user sends brings the chat back to the newest.
  const lastIsUser = last?.kind === 'entry' && last.entry.kind === 'user'
  useEffect(() => {
    if (lastIsUser) setFollowing(true)
  }, [last?.id, lastIsUser])

  // Rows that grow after layout (images, the text size) keep a follower at the bottom.
  useEffect(() => {
    const el = scroller.current
    if (!el) return
    const observer = new ResizeObserver(() => {
      if (followingRef.current) toBottom()
    })
    // `border-box`: o espaço do campo (--composer-height, bandeja da fila inclusa) é padding; crescer também rola.
    observer.observe(el.firstElementChild!, { box: 'border-box' })
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  return (
    <div className="transcript-frame">
      <div
        className="transcript"
        ref={scroller}
        role="log"
        aria-label={t('conversa.mensagens.lista')}
        onScroll={(e) => {
          const el = e.currentTarget
          const atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < 8
          if (el.scrollTop < lastScrollTop.current - 1 && !atEnd) setFollowing(false)
          else if (atEnd) setFollowing(true)
          lastScrollTop.current = el.scrollTop
          if (more && el.scrollTop < 200 && !loadingEarlier) void showEarlier()
        }}
      >
        <div className="transcript-content">
          {more ? (
            <button className="load-earlier" disabled={loadingEarlier} onClick={() => void showEarlier()}>
              {loadingEarlier ? <Spinner size={14} /> : t('conversa.mensagens.anteriores')}
            </button>
          ) : null}
          {items.length === 0 && bot ? <div className="transcript-intro">{isGroup(bot) ? <GroupIntroCard group={bot} /> : <IntroCard bot={bot} />}</div> : null}
          {items.map((item) => (
            <Fragment key={item.id}>
            {item.id === lineId ? <div className="chat-novas" role="separator" aria-label={t('conversa.navegacao.novasRotulo')}><span aria-hidden>{t('conversa.navegacao.novas')}</span></div> : null}
            <div data-chat-item={item.id} className={arrivals.has(item.id) ? rowIn(item) : undefined}>
              <Row item={item} chat={bot} openTrace={openTrace} onReply={onReply} openRoutine={openRoutine} openBotChat={openBotChat} steps={item.kind === 'entry' ? steps.byReply.get(item.entry.id) : undefined} receipt={item.kind === 'entry' ? receipts.get(item.entry.id) : undefined} />
            </div>
            </Fragment>
          ))}
          {compactacao ? <MarcaCompactando compactacao={compactacao} botId={botId} sessionId={sessionId} /> : null}
          {bot && live && compactacao?.fase !== 'andamento' ? (
            <div className={arrivals.has('working') ? 'chat-working row-in' : 'chat-working'}>
              <WorkingIndicator bot={bot} thinking={store.currentThinking(botId)} blocks={workingBlocks(steps.pending, store.bots)} approvalShown={approvalShown} decision={decision} />
            </div>
          ) : null}
          <div className="chat-end" />
        </div>
      </div>
      <ChatTimeline messages={timeline} visible={visible} scroller={scroller} onJump={() => setFollowing(false)} />
      {jump.mounted ? (
        <div className={`jump-to-latest ${jump.shown ? 'shown' : ''}`} inert={!jump.shown}>
          {/* D432: "Ir para o fim" ou "N novas" (com o ponto em accent); sobe com o campo por --composer-height. */}
          <button className={`jump-pill${pill.count ? ' has-new' : ''}`} onClick={() => { setFollowing(true); toBottom(!reduceMotion()) }}>
            {pill.count ? <span className="dot" aria-hidden /> : null}
            <Icon name="arrow.down" size={11} weight="semibold" scaled={false} />
            {pill.count ? t.plural('conversa.navegacao.fimNovas', pill.count) : t('conversa.navegacao.fim')}
          </button>
        </div>
      ) : null}
    </div>
  )
})

const Row = memo(function Row({ item, chat, openTrace, onReply, openRoutine, openBotChat, steps, receipt }: { item: ChatItem; chat: Bot | null; openTrace: () => void; onReply?: (e: Entry) => void; openRoutine: (id: string | null) => void; openBotChat: (peerId: string) => void; steps?: TurnSteps; receipt?: ReceiptAnchor }) {
  const t = useT()
  if (item.kind === 'separator') {
    return <TimeSeparator date={item.date} />
  }
  if (item.kind === 'exchanges') return <BotMessageRow exchanges={item.exchanges} open={openBotChat} />
  const e = item.entry
  if (e.kind === 'notice' && e.data.routineId) {
    return (
      <button className="chat-routine-notice" onClick={() => openRoutine(e.data.routineId!)}>
        <Icon name="clock.arrow.circlepath" size={10} />
        {routineNoticeText(t, e.data.text)}
      </button>
    )
  }
  return <ChatRow entry={e} groupStart={item.groupStart} chat={chat} openTrace={openTrace} onReply={onReply} steps={steps} receipt={receipt} />
})

/** The start of an empty group chat: who's in it and how the room works. */
function GroupIntroCard({ group }: { group: Bot }) {
  const store = useStore()
  const t = useT()
  const members = store.members(group)
  const names = members.map((m) => m.name)
  return (
    <div className="intro-card">
      <GroupAvatar members={members} size={72} />
      <Text as="div" size="title2" weight="semibold" tone="text">{group.name}</Text>
      {group.name !== names.join(', ') ? <Text as="div" size="subheadline" tone="secondary">{names.join(' · ')}</Text> : null}
      {group.description ? <Text as="div" size="subheadline" tone="secondary">{group.description}</Text> : null}
      <Text as="div" size="callout" tone="secondary" className="intro-card-hint balanced">{t('conversa.mensagens.intro.grupo')}</Text>
    </div>
  )
}

function IntroCard({ bot }: { bot: Bot }) {
  const store = useStore()
  const t = useT()
  return (
    <div className="intro-card">
      <BotAvatar bot={bot} size={72} />
      <Text as="div" size="title2" weight="semibold" tone="text">{bot.name}</Text>
      <Text as="div" size="callout" tone="secondary" className="intro-card-path">
        {bot.managedWorkspace ? t('conversa.mensagens.intro.espacoPessoal', { motor: ptBackend(store.backendName(bot.backend)) }) : t('conversa.mensagens.intro.pasta', { motor: ptBackend(store.backendName(bot.backend)), pasta: bot.cwd })}
      </Text>
      {bot.description ? <Text as="div" size="subheadline" tone="secondary">{bot.description}</Text> : null}
      <Text as="div" size="callout" tone="secondary" className="intro-card-hint">{t('conversa.mensagens.intro.bot')}</Text>
    </div>
  )
}

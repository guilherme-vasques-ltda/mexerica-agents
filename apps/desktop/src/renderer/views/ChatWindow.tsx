// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useSpaceShortcuts } from './fork/spaces/use-space-shortcuts'
import { useAtalhoBerga } from './fork/atalho-berga/use-atalho'
import { AvisoAtualizacao } from './fork/atualizacao/AvisoAtualizacao'
import { useSincronizarAtalhoVoz } from './fork/voz/entrada-voz'
import { focoBerga } from './fork/atalho-berga/foco'
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import type { WindowCommand } from '@shared/ipc'
import { showsComputerAccess } from '@shared/fork-features'
import { escopoDoSpace, type ConversaDisponivel, type LadoALadoEscopo, type LadoALadoRef } from '@shared/fork-lado-a-lado'
import { isSSHConnecting, showsChat as shouldShowChat } from '@shared/ssh-startup'
import { isGroup, type Bot } from '@shared/models'
import { CharacterAvatar } from '../components/Avatar'
import { configDaFrutinha } from '@shared/fork-frutinhas'
import { Button, ResizeHandle } from '../components/Controls'
import { Dialog, ModalHeader, Sheet, useEscape, type MenuItem } from '../components/Overlay'
import { Icon } from '../components/Icon'
import { HStack } from '../components/Stack'
import { Text } from '../components/Text'
import { listEqual, useModel, useModels, useSelectMany } from '../lib/observable'
import { canCreateOn, requireCreationStore, resolveCreationStore } from '../lib/creation-target'
import { railSpaces, railStore } from '../lib/fork-spaces-rail'
import { SpaceBotCreation } from '../store/fork-space-bot-creation'
import { prefs, stepTextSize, usePref, DEFAULT_TEXT_SIZE } from '../lib/prefs'
import { clampSidebarWidth, SIDEBAR_WIDTH } from '../lib/sidebar-layout'
import { atalhoAria, comAtalho, macDesteAparelho, teclaDeBusca, webDesteAparelho } from '../lib/atalhos'
import { useApp, useAppShell, StoreContext } from '../store/context'
import { analytics } from '../store/analytics'
import { refKey, sameRef, parseRef, type BotRef } from '../store/app-model'
import type { BotStore } from '../store/bot-store'
import { useComputerRoster } from '../store/computer-roster'
import { useSpaceView } from './fork/spaces/use-space-view'
import { PersonalReadingVisible } from './fork/spaces/reading-visibility'
import { moveSpace, spacesOrigin, type SpacesOrigin } from '../store/fork-spaces'
import { BotRoster } from './BotRoster'
import { SpaceRail } from './shell/SpaceRail'
import { SpaceSheet } from './fork/spaces/SpaceSheet'
import { spaceAllows } from '../lib/fork-space-capabilities'
import { SpaceScopeStatus } from './fork/spaces/SpaceScopeStatus'
import { SpaceSelector } from './fork/spaces/SpaceSelector'
import { JanelaHeader } from './shell/JanelaHeader'
import { BibliotecaView, libraryTarget, type LibraryTarget } from './fork/biblioteca/BibliotecaView'
import { libraryNavigation } from './fork/biblioteca/biblioteca-acoes'
import { leavesFloating, toggledSidebar } from './shell/sidebar-toggle'
import { computerSelection, useComputerMenu } from './ComputerFilterHeader'
import { extrasAllowed } from './fork/sessoes/ganchos'
import { ThreadView } from './thread/ThreadView'
import { tabMemory } from './fork/abas/memoria'
import { ladoALadoStore } from '../store/fork-lado-a-lado'
import { QuadroLadoALado } from './fork/lado-a-lado/QuadroLadoALado'
import { acaoSelecaoBarraLateral, painelFocoAposFechar } from './fork/lado-a-lado/quadro-helpers'
import { NewBotView, NewGroupView } from './bots/NewBotView'
import { BotSettingsPage as BotEditorView } from './fork/bot-settings/BotSettingsPage'
import { GroupEditorView } from './bots/GroupEditorView'
import { OpenBotSettings, type BotSettingsHandle, type BotSettingsSection } from './fork/bot-settings/bot-settings-entry'
import { extensoesScreen } from './fork/extensoes/tela'
import { ApprovalSheet } from './settings/ApprovalSheet'
import { SearchPalette } from './SearchPalette'
import { AccountEntryGate } from './AccountEntryGate'
import { ComputerAccessView } from './computer-access/ComputerAccessView'
import { HostStateView } from './HostStateView'
import { StartupRecovery } from './StartupRecovery'
export { EmptyState } from './HostStateView'
import { OpenVoiceSettings } from './call/CallView'
import { AbrirComputadores } from './fork/computador/abrir'
import { AccountPanelLayer, DesktopActionMenu } from './PanelMenus'
import { useTraySummary } from './tray-summary'
import { superficieIdioma, useIdioma, useT } from '../lib/fork-i18n/estado'
import { comNos, no } from './settings/rotulo'
import { useIslandSummary } from './island-summary'
import { useSSH, useSSHAttachments } from './settings/ssh-model'
import { useForkCloudAttachments } from './fork/cloud/use-fork-cloud'
import { WithRoutinesScreen } from './rotinas-central/camada'
import { routinesScreen } from './rotinas-central/navigation'
import { settingsScreen, type SettingsPage } from './fork/ajustes/tela'
import { WithTimesScreen } from './fork/times/camada'
import { timesScreen } from './fork/times/navigation'
import { WithTorreScreen } from './fork/torre/camada'
import { torreScreen } from './fork/torre/lugar-atual'
import { RosterSearch, rosterSearch, useRosterFilter } from './sidebar-busca/RosterSearch'
import { DonnaCard } from './fork/donna/DonnaCard'
import { useDefaultSelection } from './fork/donna/use-default-selection'
import { principalListEmpty, shownHubsOffline } from '../lib/principal'
import { WallpaperLayers } from './settings/AppearanceSettings'
import './chat-window.css'
import './sidebar-header.css'
import './shell/sidebar.css'
import { useSidebarSlide } from './shell/sidebar-slide'
import { cru, ErroFrase, frase, textoDe, type Frase } from '../lib/fork-i18n/frase'

/**
 * Native desktop chat: roster on the left, the conversation on the right (Grok Bot's desktop layout).
 * Login obrigatório (dono, 31/10/2026): numa versão com login, nada do app monta antes da conta
 * confirmada pelo processo principal (a versão sem login abre direto, dono 05/11/2026), nem os anexos SSH e da nuvem, os comandos da janela, os resumos da bandeja e da ilha ou
 * as aprovações. O portão (AccountEntryGate) mostra as boas-vindas com a entrada no lugar.
 */
export function ChatWindow() {
  return (
    <AccountEntryGate>
      <ChatShell />
    </AccountEntryGate>
  )
}

/** O app em si: só o portão o monta, com a conta confirmada. Sair ou perder a sessão o desmonta inteiro. */
function ChatShell() {
  const app = useAppShell()
  const t = useT()
  const idioma = useIdioma()
  const [accessSetup, setAccessSetup] = usePref(prefs.computerAccessSetup)
  const [showAccess, setShowAccess] = useState(false)
  useSSHAttachments(app)
  useSincronizarAtalhoVoz()
  // D171: o Hub pela nuvem, só com `cloudAccess` e anexo qualificado pelo Main; o SSH vem primeiro.
  useForkCloudAttachments(app)
  const ssh = useSSH()
  const showsChat = shouldShowChat(app.host.state.kind, app.computers.length, ssh.state)
  // Sem host neste computador (D76), não há acesso a este computador para configurar.
  const accessShown = showsComputerAccess(app.host.state)
  const needsAccess = accessSetup === null && accessShown
  const finishAccess = (ready: boolean, onboarding: boolean) => {
    setAccessSetup(ready ? 'ready' : 'later')
    analytics.track({ event: 'computer_access_finished', ready, onboarding })
  }

  useEffect(
    () =>
      window.codync.app.onCommand((c: WindowCommand) => {
        switch (c.kind) {
          case 'computerAccess':
            // Fork: sem "Computer access" (sem host local, D76), o comando não abre a folha.
            if (showsComputerAccess(app.host.state)) setShowAccess(true)
            return
          case 'reviewApprovals':
            return app.reviewApprovals()
          case 'setTextSize':
            return prefs.textSize.set(c.size)
          case 'stepTextSize':
            return prefs.textSize.set(c.up === null ? DEFAULT_TEXT_SIZE : stepTextSize(prefs.textSize.get(), c.up))
          case 'setUsageIconStyle':
            return prefs.usageIconStyle.set(c.style)
          case 'openBot': {
            const ref = parseRef(c.ref)
            if (ref) app.select(ref)
            routinesScreen.close(false)
            timesScreen.close(false)
            torreScreen.close(false)
            settingsScreen.close(false)
            extensoesScreen.close(false)
            return
          }
          case 'openSettings':
            // ⌘, (menu do app) chega mesmo com a janela inerte: só abre com a conversa na tela e sem camada na frente
            // (folha, diálogo, menu, menu da conta), senão Ajustes montaria por baixo dela e tomaria o Esc seguinte.
            if (!needsAccess && showsChat && !document.querySelector(LAYERS_IN_FRONT)) settingsScreen.show()
            return
          case 'stopBot': {
            const ref = parseRef(c.ref)
            if (ref) app.spaceMirrors.resolve(app.spaceMirrors.infer(ref))?.stop(ref.botId)
            return
          }
          case 'refresh':
            return app.refresh()
          case 'setRemoteScreen':
            void app.setRemoteScreen(c.on)
            return
          case 'respondPermission': {
            const ref = parseRef(c.ref)
            const store = ref ? app.spaceMirrors.resolve(app.spaceMirrors.infer(ref)) : null
            const entry = ref ? store?.allEntries(ref.botId).find((e) => e.id === c.entryId) : undefined
            if (store && entry) store.respond(entry, c.optionId)
            return
          }
          case 'sendMessage': {
            const ref = parseRef(c.ref)
            const resultado = ref ? app.spaceMirrors.resolve(app.spaceMirrors.infer(ref))?.send(c.text, ref.botId, { contentOrigin: c.contentOrigin, nonce: c.nonce }) : undefined
            // MX-214: a Ilha só fecha e limpa o texto com um aceite EXPLÍCITO (destino válido). Destino ausente não responde:
            // vale o prazo e a Ilha mantém o texto ("não confirmado").
            if (c.replyId && resultado) window.codync.island.sendResult(c.replyId, resultado)
            return
          }
        }
      }),
    [app, needsAccess, showsChat],
  )

  const approval = app.currentApproval
  return (
    <div className="chat-window">
      <Summaries />
      {needsAccess ? (
        <ComputerAccessView onContinue={(ready) => finishAccess(ready, true)} />
      ) : showsChat ? (
        <WithRoutinesScreen><WithTimesScreen><WithTorreScreen><ChatSplitView /></WithTorreScreen></WithTimesScreen></WithRoutinesScreen>
      ) : (
        <HostStateView />
      )}
      {!needsAccess && showsChat ? null : <div className="window-drag drag" />}
      <Sheet language={superficieIdioma(idioma)} open={showAccess && accessShown} onClose={() => setShowAccess(false)} width={660} height={Math.min(820, window.innerHeight - 80)}>
        <ModalHeader icon="hand.raised" title={t('navegacao.janela.acessoComputador')} closeTitle={t('navegacao.janela.fechar')} />
        <ComputerAccessView onContinue={(ready) => { finishAccess(ready, false); setShowAccess(false) }} />
      </Sheet>
      {/* Approving a device: the person types the code the device shows; Approve opens only when it matches (spec §4.2 B). Closing puts it off. */}
      <Sheet language={superficieIdioma(idioma)} open={approval !== null || app.confirmation !== null} onClose={() => (app.confirmation ? app.dismissConfirmation() : approval && app.deferApproval(approval))}>
        {app.confirmation ? <ApprovalSheet key={app.confirmation.approval.id} approval={app.confirmation.approval} confirmCode={app.confirmation.code} />
          : approval ? <ApprovalSheet key={approval.id} approval={approval} /> : null}
      </Sheet>
    </div>
  )
}

/** Keeps the tray and the approval island current: it follows every store, so it renders nothing. */
function Summaries() {
  const app = useApp()
  useTraySummary(app)
  useIslandSummary(app)
  return null
}

interface BotTarget {
  bot: Bot
  store: BotStore
}

interface EditTarget {
  selection: { computerId: string; botId: string } | null
  id: string
  bot: Bot
  store: BotStore
  section?: BotSettingsSection
}

/** A new bot or group chat being made, and the computer it goes to. */
interface CreateTarget {
  kind: 'bot' | 'group'
  computerId: string
  space?: SpaceBotCreation
}

/** Barra05: the sidebar from 200 to 360px, 280 by default (`SIDEBAR_WIDTH`); under 900px of window it collapses. */
const SIDEBAR_MIN = SIDEBAR_WIDTH.min
const NARROW_WINDOW = 900

function ChatSplitView() {
  const app = useAppShell()
  const t = useT()
  const idioma = useIdioma()
  // D454: menus e diálogos desta tela abrem em portal no body (limite do legado): levam o idioma e a direção dela.
  const idiomaJanela = useIdioma()
  const superficie = superficieIdioma(idiomaJanela)
  const ssh = useSSH()
  const connecting = ssh.state.profiles.some((p) => isSSHConnecting(ssh.status(p.id))) || [...app.stores.values()].some((s) => s.shownConnection.kind === 'connecting')
  // D165 fase B: the sidebar is open or collapsed to the rail (no icon-only column); below 900px of
  // window it collapses by itself, and a collapsed sidebar floats back over the chat from the rail's edge.
  const [collapsedPref, setCollapsed] = usePref(prefs.sidebarCompact)
  const [peek, setPeek] = useState(false)
  const asideRef = useRef<HTMLElement>(null)
  const detailRef = useRef<HTMLElement>(null)
  const [sidebarWidth, setSidebarWidth] = usePref(prefs.sidebarWidth)
  const [hidden, setHidden] = usePref(prefs.hiddenComputers)
  const { items: computerItems } = useComputerMenu(hidden, setHidden, () => settingsScreen.show('computers'))
  const [searching, setSearching] = useState(false)
  const [creating, setCreating] = useState<CreateTarget | null>(null)
  const [computerMenu, setComputerMenu] = useState(false)
  const [botsMenu, setBotsMenu] = useState(false)
  const [contextBot, setContextBot] = useState<{ target: BotTarget; x: number; y: number; extra: MenuItem[]; reorder: boolean } | null>(null)
  const [editing, setEditing] = useState<EditTarget | null>(null)
  const editorHandle = useRef<BotSettingsHandle>(null)
  const openEditor = (bot: Bot, store: BotStore, section?: BotSettingsSection) => {
    const open = () => setEditing({ id: crypto.randomUUID(), bot, store, selection: app.selection, section })
    if (editorHandle.current) editorHandle.current.leave(open)
    else open()
  }
  // Uma seleção na barra lateral também respeita as alterações pendentes do editor.
  useEffect(() => {
    if (!editing || isGroup(editing.bot) || sameRef(editing.selection, app.selection)) return
    const next = app.selection
    app.select(editing.selection)
    editorHandle.current?.leave(() => { setEditing(null); app.select(next) })
  }, [editing, app.selection])
  const [confirmDelete, setConfirmDelete] = useState<BotTarget | null>(null)
  const [newSessionBot, setNewSessionBot] = useState<BotTarget | null>(null)
  // ajustes-tela-01: Ajustes é uma tela (fork/ajustes), não mais uma folha desta janela.
  // O botão de ajustes da chamada abre Voz na aba Ao vivo.
  const openVoiceSettings = useCallback(() => settingsScreen.show('voice', 'vivo'), [])
  const openComputersSettings = useCallback(() => settingsScreen.show('computers'), [])
  const [showAccount, setShowAccount] = useState(false)
  const [windowSize, setWindowSize] = useState({ width: window.innerWidth, height: window.innerHeight })
  const listRef = useRef<HTMLDivElement>(null)
  const group = editing && isGroup(editing.bot) ? editing.store.bots.get(editing.bot.id) : undefined
  useEffect(() => {
    if (editing && isGroup(editing.bot) && !group) setEditing(null)
  }, [editing, group])

  useEffect(() => {
    const onResize = () => setWindowSize({ width: window.innerWidth, height: window.innerHeight })
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [setSidebarWidth])

  const sheetHeight = Math.max(420, windowSize.height - 76)
  const narrow = windowSize.width < NARROW_WINDOW
  const collapsed = collapsedPref || narrow
  const compact = false
  // A narrow window keeps the sidebar collapsed: the toggle shows or hides it floating over the chat.
  const toggleSidebar = () => {
    const next = toggledSidebar({ narrow, peek, collapsed: prefs.sidebarCompact.get(), covered: routinesScreen.open || timesScreen.open || torreScreen.open || settingsScreen.open || extensoesScreen.open })
    setPeek(next.peek)
    if (!narrow) setCollapsed(next.collapsed)
  }
  const stores = app.computers.filter(c => !app.spaceMirrors.active || c.id === app.spaceMirrors.active.computerId).map((c) => app.store(c.id)).filter((s): s is BotStore => !!s)
  // MX-4 (C9): a janela segue uma fatia de cada loja, não o `changed()` inteiro — um evento de
  // conversa não renderiza mais a moldura. Sub-modelos (Sessões, Times…) seguem como antes.
  useSelectMany(stores, () => stores.flatMap(shellStoreSig), listEqual)
  useModels(stores.map((s) => s.sessions))
  useModels(stores.map((s) => s.principal))
  useModels(stores.map((s) => s.timesRoster))
  useModels(stores.map((s) => s.botGroups))
  const shown = new Set(computerSelection(app.computers.map((c) => c.id), hidden).shown)
  const onlineStores = stores.filter((s) => shown.has(s.computer.id) && s.connection.kind === 'online' && !s.mismatch)
  const selectedStore = app.selectedStore
  // D169 (store/fork-spaces): the Hub whose spaces the rail shows, the Spaces sheet with the origin it opened on
  // (a fresh one each time it opens) and what the rail could not do.
  const [spaceSheet, setSpaceSheet] = useState<{ origin: SpacesOrigin; id: string | null; seq: number; mode?: 'pick' | 'principal' | 'delete' } | null>(null)
  // R1 do #575: guarda as chaves (e o texto cru do Hub); a tradução sai ao desenhar, no idioma de agora.
  const [spaceProblem, setSpaceProblem] = useState<{
    chaveTitulo: 'navegacao.janela.spaces' | 'navegacao.janela.moverSpaceFalhou' | 'navegacao.janela.biblioteca'
    // D507 (fatia 9c): o erro de mover vem como `Frase` do store de spaces; tudo traduzido ao desenhar.
    message: Frase
  } | null>(null)
  const moving = useRef(false)
  const spaceSheets = useRef(0)
  const noSpacesLink = { chaveTitulo: 'navegacao.janela.spaces', message: { chave: 'navegacao.janela.spacesSemHub' } } as const
  const openSpaceSheet = (store: BotStore, id: string | null, mode: 'pick' | 'principal' | 'delete' = 'pick') => {
    const origin = spacesOrigin(store)
    if (origin) setSpaceSheet({ origin, id, seq: ++spaceSheets.current, mode })
    else setSpaceProblem(noSpacesLink)
  }
  const moveRailSpace = (store: BotStore, id: string, delta: -1 | 1) => {
    if (moving.current) return
    const origin = spacesOrigin(store)
    if (!origin) return setSpaceProblem(noSpacesLink)
    moving.current = true
    void moveSpace(origin, id, delta)
      .catch((e: unknown) => setSpaceProblem({ chaveTitulo: 'navegacao.janela.moverSpaceFalhou', message: e instanceof ErroFrase ? e.frase : cru(e instanceof Error ? e.message : String(e)) }))
      .finally(() => { moving.current = false })
  }
  // BN-03 (components/BibliotecaEspecialistas): a biblioteca do space de onde abriu; editor só no Pessoal.
  const [library, setLibrary] = useState<{ target: LibraryTarget; store: BotStore; seq: number } | null>(null)
  const libraries = useRef(0)
  const openLibrary = (store: BotStore | null | undefined, spaceId?: string) => {
    const target = store ? libraryTarget(store, spaceId) : null
    if (target && store) setLibrary({ target, store, seq: ++libraries.current })
    else setSpaceProblem({ chaveTitulo: 'navegacao.janela.biblioteca', message: { chave: 'navegacao.janela.semBiblioteca' } })
  }
  const { spaceView, spacesStore, openSpaceRecord, openSpace, closeSpace } = useSpaceView(app, railStore(onlineStores, selectedStore))
  const [selectorOpen, setSelectorOpen] = useState(false)
  useSpaceShortcuts({
    ids: [...(spacesStore?.spaces?.spaces ?? [])].sort((a, b) => a.position - b.position).map(s => s.id),
    activeId: app.spaceMirrors.active?.spaceId ?? null,
    available: () => !layersOpen && shellOwnsKeyboard(),
    select: (id) => {
      for (const screen of [settingsScreen, routinesScreen, timesScreen, torreScreen, extensoesScreen]) screen.close(false)
      if (spacesStore) id === spacesStore.spaces?.personalSpaceId ? closeSpace() : openSpace(spacesStore, id)
    },
    open: () => {
      setSelectorOpen(v => !v)
      if (narrow) setPeek(true)
      else setCollapsed(false)
    },
  })
  // Times (components/TimesView) ocupa a janela como Rotinas: conta como tela aberta nos mesmos lugares.
  const timesOpen = useModel(timesScreen).open
  // VG-02: o Mission Board também ocupa a janela, nos mesmos lugares.
  const torreOpen = useModel(torreScreen).open
  const routinesOpen = useModel(routinesScreen).open || timesOpen || torreOpen
  // Abrir no Mission Board só navega: a conversa do bot no Pessoal, ou exatamente a conversa indicada no space.
  torreScreen.navigate = (store, target) => {
    if (target.kind === 'space') return openSpace(store, target.spaceId, target.conversationId)
    closeSpace()
    app.select({ computerId: store.computer.id, botId: target.botId })
  }
  // D249 E3: Extensões is a screen like Ajustes and counts wherever it does.
  const extensionsOpen = useModel(extensoesScreen).open
  const settingsOpen = useModel(settingsScreen).open || extensionsOpen
  const createStore = creating ? app.store(creating.computerId) : null
  const computerRoster = useComputerRoster(app, shown)
  const roster = useRosterFilter(computerRoster)
  // Abertura no Berga (docs/features/janela-do-mac.md#ao-abrir): o Hub do trilho, senão o primeiro online da barra.
  const shownStores = roster.principalSections.map((s) => s.store)
  const startupHub = railStore(onlineStores, selectedStore) ?? shownStores.find((s) => onlineStores.includes(s)) ?? shownStores[0] ?? null
  // Só os bots visíveis recebidos decidem; a paleta (que reinclui a principal oculta) só protege a escolha da pessoa.
  const startup = useDefaultSelection(app, { items: computerRoster.items, palette: roster.items, rows: roster.navigable, hub: startupHub, stores: shownStores, connecting,
    paused: !!editing || !!creating || routinesOpen || settingsOpen || searching })

  const quadroModel = useModel(ladoALadoStore)
  // O escopo vem da identidade do Space ativo, não do registro carregado: durante a reconexão
  // o registro (ou a lista de Spaces) falta por um instante e o quadro não pode cair no pessoal.
  const espelhoAtivo = app.spaceMirrors.active
  const escopoLadoALado = useMemo<LadoALadoEscopo | null>(() => {
    if (!espelhoAtivo) return null
    const hostId = espelhoAtivo.hostId ?? spacesStore?.hello?.hostId
    return { computerId: espelhoAtivo.computerId, spaceId: espelhoAtivo.spaceId, ...(hostId ? { hostId } : {}) }
  }, [espelhoAtivo?.computerId, espelhoAtivo?.spaceId, espelhoAtivo?.hostId, spacesStore?.hello?.hostId])
  const quadroEstado = quadroModel.estado(escopoLadoALado)
  const rosterParaReconciliar = app.roster
  const rosterKeysReconciliar = rosterParaReconciliar.map((i) => refKey(i.ref)).join('|')
  const sessoesVersionReconciliar = stores.map((s) => s.sessions.getVersion()).join(',')
  const prontosKeyReconciliar = stores
    .filter((s) => s.connection.kind === 'online' && !s.mismatch)
    .map((s) => s.computer.id)
    .sort()
    .join(',')
  // A carência corrente re-executa o efeito quando expira, sem depender de outro evento chegar.
  const [carenciaSeq, setCarenciaSeq] = useState(0)
  useEffect(() => {
    const agora = Date.now()
    const computadoresProntos = new Set<string>()
    let carencia = 0
    for (const s of stores) {
      if (s.connection.kind !== 'online' || s.mismatch) continue
      // Carência do roster (store/fork-empty-roster.ts): logo após (re)conectar, cada bot chega
      // num evento próprio e um roster parcial não é prova de ausência. Enquanto algum computador
      // online ainda está na carência, nada é reconciliado; quando todos expiram, vale o roster
      // real (com as sessões carregadas de cada conversa).
      const restante = s.emptyWatch.remaining(agora)
      if (restante !== null && restante > 0) {
        carencia = Math.max(carencia, restante)
        continue
      }
      computadoresProntos.add(s.computer.id)
    }
    if (carencia > 0) {
      const timer = setTimeout(() => setCarenciaSeq((n) => n + 1), carencia + 50)
      return () => clearTimeout(timer)
    }
    const disponiveis: ConversaDisponivel[] = rosterParaReconciliar.map((item) => {
      const lista = item.store.sessions.of(item.bot.id)
      return lista
        ? { ref: item.ref, sessionIds: new Set(lista.filter((s) => !s.principal).map((s) => s.id)) }
        : { ref: item.ref }
    })
    ladoALadoStore.reconciliar(escopoLadoALado, disponiveis, computadoresProntos)
    return undefined
  }, [escopoLadoALado, prontosKeyReconciliar, sessoesVersionReconciliar, rosterKeysReconciliar, carenciaSeq])

  const ultimaSeqRef = useRef(app.selectionSeq)
  const ignorarSeqRef = useRef<number | null>(null)
  const prevStartupKindRef = useRef(startup.kind)
  const prevEscopoKeyRef = useRef(escopoDoSpace(escopoLadoALado))
  const prevQuadroAtivoRef = useRef(quadroEstado.ativo)

  const selecionarPeloQuadro = useCallback((ref: LadoALadoRef) => {
    const seq = app.selectionSeq
    app.select(ref)
    // `select` pode devolver sem incrementar (hubId divergente do hostId atual): sem avanço, o
    // token não fica órfão engolindo a próxima seleção genuína da barra lateral.
    ignorarSeqRef.current = app.selectionSeq !== seq ? app.selectionSeq : null
  }, [app])

  useEffect(() => {
    const seq = app.selectionSeq
    const escopoAtualKey = escopoDoSpace(escopoLadoALado)
    const mudouEscopo = prevEscopoKeyRef.current !== escopoAtualKey
    const acabouDeAtivar = !prevQuadroAtivoRef.current && quadroEstado.ativo
    const veioDeAutoStartup = prevStartupKindRef.current === 'select' && startup.kind === 'keep'
    prevStartupKindRef.current = startup.kind
    prevEscopoKeyRef.current = escopoAtualKey
    prevQuadroAtivoRef.current = quadroEstado.ativo

    if (seq === ultimaSeqRef.current) return
    ultimaSeqRef.current = seq

    if (ignorarSeqRef.current === seq) {
      ignorarSeqRef.current = null
      return
    }
    if (!quadroEstado.ativo || startup.kind !== 'keep' || mudouEscopo || acabouDeAtivar || veioDeAutoStartup || !app.selection) return
    const acao = acaoSelecaoBarraLateral(quadroEstado, app.selection, escopoLadoALado)
    if (acao.kind === 'focar') ladoALadoStore.focar(escopoLadoALado, acao.panelId)
    else if (acao.kind === 'adicionar') ladoALadoStore.abrir(escopoLadoALado, acao.ref)
    else if (acao.kind === 'trocar') ladoALadoStore.trocar(escopoLadoALado, acao.panelId, acao.ref, null)
  }, [app.selectionSeq, app.selection, quadroEstado, startup.kind, escopoLadoALado])

  const fecharTelasCobertas = () => {
    for (const screen of [settingsScreen, routinesScreen, timesScreen, torreScreen, extensoesScreen]) screen.close(false)
  }
  const refDaConversa = (bot: Bot, store: BotStore): LadoALadoRef => ({
    computerId: store.computer.id,
    botId: bot.id,
    ...(store.spaceId ? { spaceId: store.spaceId } : {}),
    ...(store.hello?.hostId ? { hubId: store.hello.hostId } : {}),
  })
  const aplicarSessaoIndividual = (botId: string, sessionId: string | null) => {
    const memoria = tabMemory(botId)
    memoria.open = sessionId
    if (sessionId && memoria.closed.has(sessionId)) {
      const next = new Set(memoria.closed)
      next.delete(sessionId)
      memoria.closed = next
    }
  }
  const abrirConversaLadoALado = (bot: Bot, store: BotStore) => {
    const executar = () => {
      fecharTelasCobertas()
      setEditing(null)
      ladoALadoStore.abrir(escopoLadoALado, refDaConversa(bot, store), {
        sessionId: tabMemory(bot.id).open,
        inicial: app.selection,
        inicialSessionId: app.selection ? tabMemory(app.selection.botId).open : null,
      })
    }
    if (editing && !isGroup(editing.bot) && editorHandle.current) editorHandle.current.leave(executar)
    else executar()
  }
  const alternarLadoALado = () => {
    const executar = () => {
      fecharTelasCobertas()
      setEditing(null)
      if (quadroEstado.ativo) {
        const foco = painelFocoAposFechar(quadroEstado)
        if (foco) aplicarSessaoIndividual(foco.ref.botId, foco.sessionId)
        ladoALadoStore.sair(escopoLadoALado)
        if (foco && !sameRef(app.selection, foco.ref)) selecionarPeloQuadro(foco.ref)
        return
      }
      const inicial = app.selection ?? app.roster[0]?.ref ?? null
      ladoALadoStore.entrar(escopoLadoALado, inicial, inicial ? tabMemory(inicial.botId).open : null)
    }
    if (editing && !isGroup(editing.bot) && editorHandle.current) editorHandle.current.leave(executar)
    else executar()
  }

  const create = (kind: CreateTarget['kind'], computerId?: string) => {
    // C2: only global creation may fall back. An explicit unavailable target stays explicit.
    const target = resolveCreationStore(onlineStores, [selectedStore, app.local], computerId)
    if (target) setCreating({ kind, computerId: target.computer.id, ...(kind === 'bot' && openSpaceRecord && spacesStore ? { space: new SpaceBotCreation(spacesStore, openSpaceRecord.id, (bot, hubId) => { target.receiveCreatedBot(bot, hubId) }) } : {}) })
    else if (computerId !== undefined) setCreating({ kind, computerId })
  }
  const creationAvailability = creating && createStore ? {
    error: shown.has(createStore.computer.id) && canCreateOn(createStore) ? null : t('navegacao.janela.criacaoHubIndisponivel'),
    check: () => {
      creating.space?.check()
      requireCreationStore(app.store(creating.computerId), createStore)
      if (!computerSelection(app.computers.map((c) => c.id), prefs.hiddenComputers.get()).shown.includes(creating.computerId)) throw new Error(t('navegacao.janela.criacaoHubIndisponivel'))
    },
  } : undefined
  const computerChoice = creating && !creating.space && createStore && onlineStores.includes(createStore) && onlineStores.length > 1
    ? { options: onlineStores.map((s) => ({ id: s.computer.id, label: s.hostName })), selection: creating.computerId, onChange: (computerId: string) => setCreating({ ...creating, computerId }) }
    : null

  // MX-4 (C12): a assinatura do comando é única; o handler lê o estado vivo por ref — o efeito sem
  // deps reinstalava o listener a cada render (um por evento do Hub).
  const onWindowCommand = useRef<(c: WindowCommand) => void>(() => {})
  onWindowCommand.current = (c) => {
    // Rotinas keeps this view mounted for drafts/scroll. Read the live page state at delivery;
    // inert hides its DOM, but cannot suspend callbacks. The rail's toggle stays global.
    if ((routinesScreen.open || timesScreen.open || torreScreen.open || settingsScreen.open || extensoesScreen.open) && (c.kind === 'newBot' || c.kind === 'search')) return
    if (c.kind === 'newBot' && onlineStores.length) create('bot')
    if (c.kind === 'search' && (openSpaceRecord || !rosterSearch.focus(collapsed && !peek))) setSearching(true)
    if (c.kind === 'toggleSidebar') toggleSidebar()
  }
  useEffect(() => window.codync.app.onCommand((c) => onWindowCommand.current(c)), [])

  useAtalhoBerga(() => {
    const personal = spacesStore ?? selectedStore
    return {
      // spaceId continua explícito mesmo se o registro estiver indisponível: jamais cair no Pessoal.
      space: app.spaceMirrors.active ? { id: app.spaceMirrors.active.spaceId, ready: !!openSpaceRecord, live: selectedStore?.connection.kind === 'online',
        principalId: openSpaceRecord?.principalBotId ?? null, conversations: [...(selectedStore?.conversations.values() ?? [])] } : null,
      personal: personal ? { computerId: personal.computer.id, ready: personal.connection.kind === 'online' && !personal.mismatch && personal.principal.phase === 'ready',
        principalId: personal.principal.id, hasBot: !!personal.principal.id && personal.bots.has(personal.principal.id) } : null,
    }
  }, () => !layersOpen && !spaceSheet && !spaceProblem && !library && !editing && !creating &&
    !document.querySelector('[aria-modal="true"], [role="menu"]') &&
    (settingsScreen.open || routinesScreen.open || timesScreen.open || torreScreen.open || extensoesScreen.open || shellOwnsKeyboard()), (target) => {
    for (const screen of [settingsScreen, routinesScreen, timesScreen, torreScreen, extensoesScreen]) screen.close(false)
    if (target.kind === 'space') {
      if (spacesStore && openSpaceRecord) { app.select({ computerId: spacesStore.computer.id, spaceId: openSpaceRecord.id, botId: target.id }); focoBerga.show(spacesStore.computer.id, target.id) }
    } else {
      app.select({ computerId: target.computerId, botId: target.botId })
      focoBerga.show(target.computerId, target.botId)
    }
  }, () => setSpaceProblem({ chaveTitulo: 'navegacao.janela.spaces', message: { chave: 'ajustes.berga.semPrincipal' } }))

  // D169 (SidebarHeader): ⌘K also goes to Buscar bots, like the menu's ⌘F — only while the shell owns the
  // keyboard. A sheet or dialog (everything behind it inert), a menu, the palette or Rotinas keep the key:
  // nothing opens over them and their focus stays (revisão R1, F5).
  const layersOpen = searching || computerMenu || botsMenu || contextBot !== null || showAccount
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!teclaDeBusca(e, macDesteAparelho()) || e.defaultPrevented) return
      if (routinesScreen.open || timesScreen.open || torreScreen.open || settingsScreen.open || extensoesScreen.open || layersOpen || !shellOwnsKeyboard()) return
      e.preventDefault()
      if (openSpaceRecord || !rosterSearch.focus(collapsed && !peek)) setSearching(true)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [collapsed, peek, layersOpen, openSpaceRecord])

  // Screenshot/UI checks (development builds): open a screen at launch.
  const debugOpened = useRef(false)
  useEffect(() => {
    const target = window.codync.debugOpen
    if (!target || debugOpened.current) return
    if (target === 'newBot' || target === 'newGroup') {
      if (!onlineStores.length) return
      debugOpened.current = true
      create(target === 'newBot' ? 'bot' : 'group')
    } else if (target === 'plugins') {
      if (!app.local) return
      debugOpened.current = true
      extensoesScreen.show('connectors', app.local.computer.id)
    } else if (target === 'computers') {
      debugOpened.current = true
      settingsScreen.show('computers')
    } else if (target.startsWith('ajustes:')) {
      // ajustes-tela-01: `ajustes:<página>[:<aba>]` abre a tela de Ajustes, para prints.
      debugOpened.current = true
      const [, page, aba] = target.split(':')
      settingsScreen.show(page as SettingsPage, aba)
    } else {
      const item = app.roster.find((i) => i.bot.name === target)
      if (!item) return
      debugOpened.current = true
      app.select(item.ref)
    }
  })

  /** D249 E3: Extensões is a screen; each shortcut opens its own page (Aplicativos, Conectores, Skills). */
  const marketplaceComputer = () => (selectedStore ?? app.local ?? onlineStores[0])?.computer.id ?? null

  // Passo 12: o painel direito em Sessões é do executor B (`store.openSessionsPanel`); sem ele na main, nada acontece.
  const sessionsPanel = stores.some((s) => typeof (s as BotStore & { openSessionsPanel?: unknown }).openSessionsPanel === 'function')
  const openSessions = (bot: Bot, store: BotStore) => {
    app.select({ computerId: store.computer.id, botId: bot.id, ...(store.spaceId ? { spaceId: store.spaceId } : {}) })
    ;(store as BotStore & { openSessionsPanel?: (botId: string) => void }).openSessionsPanel?.(bot.id)
  }
  // UI-01: "Nova sessão" cria uma extra (abre a conversa e cria nela); só com o Hub anunciando `create`.
  const startSession = (bot: Bot, store: BotStore) => {
    app.select({ computerId: store.computer.id, botId: bot.id, ...(store.spaceId ? { spaceId: store.spaceId } : {}) })
    store.sessions.requestNew(bot.id)
  }
  const canStartSession = (bot: Bot, store: BotStore) => spaceAllows(store.spaceId, 'sessions') && extrasAllowed(store, bot) && store.sessions.can(bot.id, 'create') && store.sessions.can(bot.id, 'send')
  const botActions = (bot: Bot, store: BotStore, reorder = true): MenuItem[] => [
    ...(bot.id !== store.principal.protectedId ? [
      ...(!isGroup(bot) ? [
        { title: t('navegacao.janela.menu.organizar'), disabled: !spaceAllows(store.spaceId, 'teams'), detail: store.spaceId ? t('spaces.selector.unavailable') : undefined, icon: 'person.2', action: () => timesScreen.show(null, store.computer.id) },
        ...(reorder ? [
          { title: t('navegacao.janela.menu.subir'), detail: '⌥↑', action: () => roster.moveBot({ computerId: store.computer.id, botId: bot.id }, -1) },
          { title: t('navegacao.janela.menu.descer'), detail: '⌥↓', action: () => roster.moveBot({ computerId: store.computer.id, botId: bot.id }, 1) },
        ] : []),
      ] : []),
      { title: t(bot.pinned ? 'navegacao.janela.menu.desafixar' : 'navegacao.janela.menu.fixar'), icon: 'pin', action: () => store.setPinned(bot, !bot.pinned) },
    ] : []),
    { title: t('navegacao.janela.menu.lida'), icon: 'bell.badge', action: () => store.markAllRead(bot.id) },
    { title: t('conversa.ladoALado.abrirNoMenu'), icon: 'donna.ladoalado', action: () => abrirConversaLadoALado(bot, store) },
    { title: t(isGroup(bot) ? 'navegacao.janela.menu.ajustesCanal' : 'navegacao.janela.menu.ajustesBot'), icon: 'square.and.pencil', divider: true, action: () => openEditor(bot, store) },
    { title: t('navegacao.janela.menu.copiarId'), icon: 'square.on.square', divider: true, action: () => window.codync.app.copy(bot.id) },
    // DON-903: sessions.new.label / sessions.restart.menu. O item legado (`newSession`) recomeça a principal, sem resumo.
    ...(canStartSession(bot, store) ? [{ title: t('sessions.new.label'), icon: 'plus', action: () => startSession(bot, store) }] : []),
    { title: t('navegacao.janela.menu.recomecar'), icon: 'arrow.counterclockwise', action: () => setNewSessionBot({ bot, store }) },
    ...(bot.id !== store.principal.protectedId ? [
      { title: t('navegacao.janela.menu.ocultar'), icon: 'eye.slash', divider: true, action: () => store.setHidden(bot, true) },
      { title: t('navegacao.janela.menu.apagar'), icon: 'trash', destructive: true, action: () => setConfirmDelete({ bot, store }) },
    ] : []),
  ]

  // ↑/↓ in the roster move the selection; Alt+↑/↓ moves the selected bot itself.
  const onListKey = (e: React.KeyboardEvent) => {
    if (!(e.target instanceof Element) || !listRef.current?.contains(e.target)) return
    if (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') return
    // Cabeçalhos de computadores, Times e Grupos/Bots keep their own keys: no selection move from them.
    if (e.target instanceof HTMLElement && e.target.closest('.computer-roster-heading, .times-team-heading, .sidebar-section-head')) return
    if (e.altKey) {
      e.preventDefault()
      const key = e.target instanceof HTMLElement ? e.target.closest<HTMLElement>('[data-roster-row]')?.dataset.rosterRow : undefined
      const ref = roster.navigable.find((i) => refKey(i.ref) === key)?.ref ?? app.selection
      if (ref) roster.moveBot(ref, e.key === 'ArrowUp' ? -1 : 1)
      return
    }
    const list = listRef.current
    if (!list) return
    const rows = [...list.querySelectorAll<HTMLElement>('[data-times-header], [data-roster-row]')].filter((el) => !el.closest('[hidden], [inert], [aria-hidden="true"]'))
    if (!rows.length) return
    e.preventDefault()
    const focused = e.target instanceof HTMLElement ? e.target.closest<HTMLElement>('[data-times-header], [data-roster-row]') : null
    const index = focused ? rows.indexOf(focused) : rows.findIndex((el) => el.dataset.rosterRow === (app.selection ? refKey(app.selection) : ''))
    const next = rows[Math.max(0, Math.min(rows.length - 1, index + (e.key === 'ArrowDown' ? 1 : -1)))]!
    next.focus()
    const item = roster.navigable.find((i) => refKey(i.ref) === next.dataset.rosterRow)
    if (item) app.select(item.ref)
  }

  // The sidebar's edge drags its width from 200 to 360; pulled well under the minimum it collapses; a double click resets it.
  // While dragging, the width goes straight onto the aside (one write per frame, no React render per pointer move);
  // letting go saves it once. The keyboard's ←/→ steps save at once.
  const [resizing, setResizing] = useState(false)
  const liveWidth = useRef<{ width: number | null; frame: number; dragging: boolean }>({ width: null, frame: 0, dragging: false })
  const resizeSidebar = (proposed: number) => {
    const live = liveWidth.current
    if (proposed < SIDEBAR_MIN - 30) { live.width = null; return setCollapsed(true) }
    const next = clampSidebarWidth(proposed)
    if (!live.dragging) return setSidebarWidth(next)
    live.width = next
    if (!live.frame) live.frame = requestAnimationFrame(() => {
      live.frame = 0
      if (live.width !== null && asideRef.current) asideRef.current.style.width = `${live.width}px`
    })
  }
  const dragSidebar = (on: boolean) => {
    const live = liveWidth.current
    live.dragging = on
    setResizing(on)
    if (on || live.width === null) return
    cancelAnimationFrame(live.frame)
    live.frame = 0
    setSidebarWidth(live.width)
    live.width = null
  }

  const errorMessage = app.errorMessage
  const mismatchStores = stores.filter((s) => shown.has(s.computer.id) && s.mismatch)
  const selected = app.selection
  const selectedBot = selected && selectedStore?.bots.get(selected.botId)

  // D249 R5: every computer the bar shows is offline (a hidden one connecting or online doesn't hold it back).
  const offline = shownHubsOffline(shownStores.map((s) => s.shownConnection))
  const offlineHub = shownStores[0]?.hostName ?? t('navegacao.janela.seuComputador')
  const offlineReason = shownStores.map((s) => s.shownConnection).find((c) => c.kind === 'offline')?.message ?? null
  // Auditoria D249, F7: with the whole list empty the conversation side says it once; the sidebar keeps only titles.
  const quietEmpty = startup.kind === 'empty' && !roster.filtering
  const listEmpty = quietEmpty || openSpaceRecord ? null : principalListEmpty(t, roster.filtering, roster.principalSections.length > 0, offline)
  let detail: ReactNode
  // D169 (AppShell, LayoutVariants): the sidebar button sits at the conversation's top left. The bot
  // editor has its own header there; ⌃⌘S, the menu and the edge keep the sidebar reachable from it.
  const detailToggle = !(editing && !isGroup(editing.bot))
  const quadroAtivo = !(editing && !isGroup(editing.bot)) && quadroEstado.ativo && quadroEstado.paineis.length > 0 && startup.kind === 'keep'
  if (editing && !isGroup(editing.bot)) {
    detail = <StoreContext.Provider value={editing.store}>
      <BotEditorView key={editing.id} ref={editorHandle} botId={editing.bot.id!} initialSection={editing.section} onClose={() => setEditing(null)}
        onConversation={() => { app.select({ computerId: editing.store.computer.id, botId: editing.bot.id! }); setEditing(null) }} />
    </StoreContext.Provider>
  } else if (quadroAtivo) {
    detail = (
      <QuadroLadoALado
        app={app}
        escopo={escopoLadoALado}
        estado={quadroEstado}
        itens={computerRoster.items}
        aviso={quadroModel.aviso}
        onLimparAviso={() => ladoALadoStore.limparAviso()}
        sidebarOpen={!collapsed || peek}
        sidebarButtonInset={detailToggle && !(collapsed && peek)}
        onCollapseSidebar={() => {
          if (narrow) setPeek(false)
          else setCollapsed(true)
        }}
        onAbrir={(ref, sessionId) => ladoALadoStore.abrir(escopoLadoALado, ref, { sessionId: sessionId ?? null })}
        onRemover={(panelId) => ladoALadoStore.remover(escopoLadoALado, panelId)}
        onTrocar={(panelId, ref, sessionId) => ladoALadoStore.trocar(escopoLadoALado, panelId, ref, sessionId ?? null)}
        onAtualizarSessao={(panelId, sessionId) => ladoALadoStore.atualizarSessao(escopoLadoALado, panelId, sessionId)}
        onMover={(panelId, delta) => ladoALadoStore.mover(escopoLadoALado, panelId, delta)}
        onFocar={(panelId) => ladoALadoStore.focar(escopoLadoALado, panelId)}
        onCiclarFoco={(delta) => ladoALadoStore.ciclarFoco(escopoLadoALado, delta)}
        onSair={(focusRef, sessionId) => {
          const foco = painelFocoAposFechar(quadroEstado)
          const alvo = focusRef ?? foco?.ref ?? null
          const sessaoAlvo = focusRef ? (sessionId ?? null) : (foco?.sessionId ?? null)
          if (alvo) aplicarSessaoIndividual(alvo.botId, sessaoAlvo)
          ladoALadoStore.sair(escopoLadoALado)
          if (alvo && !sameRef(selected, alvo)) selecionarPeloQuadro(alvo)
        }}
      />
    )
  } else if (selected && selectedStore && selectedBot && startup.kind === 'keep') {
    detail = (
      <StoreContext.Provider value={selectedStore}>
        {/* MX-4 (C10): a montagem é por conversa, sem o hubId do refKey — ele chega com o hello e
            remontava a tela aberta (rolagem e aba perdidas). Se o Hub troca, selectedStore zera e a árvore sai. */}
        <ThreadView key={chatKey(selected)} botId={selected.botId} />
      </StoreContext.Provider>
    )
  } else if (!onlineStores.length && ssh.state.profiles.length) {
    detail = <StartupRecovery manage={() => settingsScreen.show('computers')} />
  } else if (offline) {
    // D249 R5: every computer of the bar is down and none is connecting: say so once, with the way out.
    detail = (
      <div className="empty-detail" role="alert">
        <div className="detail-drag drag" />
        <Icon name="wifi.slash" size={28} color="var(--context-ink-2, var(--ink-2))" />
        <Text as="div" size="title2" weight="semibold">{t('navegacao.barra.principal.semHub')}</Text>
        <Text as="div" tone="secondary" className="empty-detail-message">{comNos(t('navegacao.janela.offlineDetalhe', { hub: no('hub') }), { hub: <bdi dir="auto">{offlineHub}</bdi> })}</Text>
        {offlineReason ? <Text as="div" size="caption" tone="secondary" className="empty-detail-message">{offlineReason}</Text> : null}
        <HStack gap={8}>
          <Button onClick={() => app.refresh()}>{t('navegacao.janela.tentarDeNovo')}</Button>
          <Button kind="secondary" onClick={openComputersSettings}>{t('navegacao.janela.verComputadores')}</Button>
        </HStack>
      </div>
    )
  } else if (startup.kind === 'select' || startup.kind === 'wait') {
    detail = <div className="empty-detail"><div className="detail-drag drag" /></div>
  } else {
    detail = (
      <div className="empty-detail">
        <div className="detail-drag drag" />
        <HStack align="stretch" className="empty-detail-crew">
          {/* D478: as frutinhas no lugar dos 3 robôs fixos (decorativas: o texto ao lado diz o que é). */}
          <CharacterAvatar shape="blob" color="blue" config={configDaFrutinha('assistente')} size={60} mood="working" label="" />
          <CharacterAvatar shape="squircle" color="orange" config={configDaFrutinha('developer')} size={60} label="" />
          <CharacterAvatar shape="teardrop" color="violet" config={configDaFrutinha('marketing')} size={60} mood="working" label="" />
        </HStack>
        <Text as="div" size="title2" weight="semibold">{t(connecting ? 'navegacao.janela.vazio.conectando' : 'navegacao.janela.vazio.titulo')}</Text>
        <Text as="div" tone="secondary" className="empty-detail-message">{t(connecting ? 'navegacao.janela.vazio.conectandoDetalhe' : 'navegacao.janela.vazio.detalhe')}</Text>
        {startup.kind === 'empty' ? <Button onClick={() => create('bot')} disabled={!onlineStores.length}>
          {t('navegacao.janela.novoBot')}
        </Button> : null}
      </div>
    )
  }

  const width = clampSidebarWidth(sidebarWidth)
  const sidebarShown = !collapsed || peek
  // A352-1: a barra fixa anima a largura ao abrir e fechar (shell/sidebar-slide.ts); fechando, fica montada até o fim.
  const { mounted: asideMounted, closing } = useSidebarSlide(asideRef, { collapsed, peek, width })
  // A floating sidebar stays while one of its menus, the palette or a panel is open, and while focus is
  // in it (typing in Buscar bots): it leaves when both the pointer and the focus have left (D165 revisão B).
  // Pointer and focus are read from the aside's own DOM node, never remembered: a menu in a React portal
  // isn't inside it (B6), and an aside that unmounted leaves nothing stale behind (B5).
  const keepPeek = computerMenu || botsMenu || searching || contextBot !== null || showAccount
  const leavePeek = (nextFocus?: Element | null) => {
    if (!collapsed || keepPeek) return
    const aside = asideRef.current
    if (!aside) return setPeek(false)
    const focused = nextFocus !== undefined ? nextFocus : document.activeElement
    if (leavesFloating({ collapsed, layersOpen: keepPeek, pointerInside: aside.matches(':hover'), focusInside: focused !== null && aside.contains(focused) })) setPeek(false)
  }
  // A menu or panel that closes while pointer and focus are elsewhere lets the floating sidebar go.
  useEffect(() => {
    if (!keepPeek) leavePeek()
  }, [keepPeek])
  // What had focus when the floating sidebar opened (outside it), for Escape to give it back.
  const focusBeforePeek = useRef<Element | null>(null)
  useEffect(() => {
    if (!peek) return
    const active = document.activeElement
    focusBeforePeek.current = active && active !== document.body && !asideRef.current?.contains(active) ? active : null
  }, [peek])
  // Escape dismisses the floating sidebar wherever focus is, through the shared Escape stack: a menu,
  // the palette or a panel above it takes the key first. Focus returns to the sidebar button; where there
  // is none (the bot editor), to what had it before the sidebar opened, else to the editor's area — never to
  // a removed, hidden or inert element (revisão R1, F3).
  // A barra que flutuava quando Rotinas ou Ajustes cobriu a janela sai: escondida atrás, não toma o Esc da tela.
  const covered = routinesOpen || settingsOpen
  useEffect(() => { if (covered) setPeek(false) }, [covered])
  useEscape(collapsed && peek && !keepPeek && !covered, () => {
    const inside = document.activeElement?.closest('aside.sidebar.floating')
    setPeek(false)
    if (inside) requestAnimationFrame(() => {
      const target = [document.querySelector('[data-sidebar-toggle]'), focusBeforePeek.current].find(canTakeFocus)
      if (target) return target.focus()
      // The editor's area, focusable only while it holds this focus: clicks on the conversation stay as they were.
      const area = detailRef.current
      if (!area) return
      area.tabIndex = -1
      area.addEventListener('blur', () => area.removeAttribute('tabindex'), { once: true })
      area.focus()
    })
  }, asideRef)
  const openMarketplaceAt = (tab: 'connectors' | 'skills') => {
    prefs.marketplaceTab.set(tab)
    if (tab !== 'skills') prefs.marketplaceApplicationTab.set(tab)
    extensoesScreen.show(tab, marketplaceComputer())
  }
  return (
    <OpenBotSettings.Provider value={(store, botId, section) => { const bot = store.bots.get(botId); if (bot) openEditor(bot, store, section) }}>
    <div className={`split ${showAccount || contextBot ? 'covered' : ''}${openSpaceRecord ? ' in-space' : ''}`}>
      <WallpaperLayers />
      {/* MX-122 v3: a faixa do topo da janela, com o seletor de Space à esquerda e a logo no centro. */}
      <JanelaHeader>
        {spacesStore ? <SpaceSelector app={app} root={spacesStore} active={openSpaceRecord} onOpen={id => openSpace(spacesStore, id)} onPersonal={closeSpace}
          onCreate={() => openSpaceSheet(spacesStore, null)} onEdit={id => openSpaceSheet(spacesStore, id)} open={selectorOpen} setOpen={setSelectorOpen} /> : null}
      </JanelaHeader>
      <SpaceRail
        accountOpen={showAccount}
        onAccount={() => setShowAccount((s) => !s)}
        onSkills={() => openMarketplaceAt('skills')}
        onConnectors={() => openMarketplaceAt('connectors')}
        onUsage={() => settingsScreen.show('usage')}
        onSettings={() => settingsScreen.show('general')}
        approvals={app.approvals.length}
        onApprovals={() => app.reviewApprovals()}
        extensionsAvailable={onlineStores.length > 0}
        torreStore={spacesStore}
        ladoALado={{
          active: quadroEstado.ativo,
          available: app.roster.length > 0 && startup.kind === 'keep',
          onToggle: alternarLadoALado,
        }}
        spaces={spacesStore ? {
          items: railSpaces(spacesStore.spaces),
          activeId: openSpaceRecord?.id ?? null,
          computerId: spacesStore.computer.id,
          onOpen: (id) => openSpace(spacesStore, id),
          onPersonal: closeSpace,
          keys: () => !layersOpen && shellOwnsKeyboard(),
          onCreate: () => openSpaceSheet(spacesStore, null),
          onEdit: (id) => openSpaceSheet(spacesStore, id),
          onMove: (id, delta) => moveRailSpace(spacesStore, id, delta),
        } : null}
      />
      {collapsed && !peek ? <div className="sidebar-peek-edge" aria-hidden onMouseEnter={() => setPeek(true)} /> : null}
      {asideMounted ? (
      <aside
        ref={asideRef}
        className={`sidebar glass-panel ${collapsed && !closing ? 'floating' : ''} ${resizing ? 'resizing' : ''}`}
        style={closing ? { width: 0, marginLeft: 0 } : { width }}
        aria-label={t('navegacao.janela.barraLateral')}
        onMouseLeave={() => leavePeek()}
        onBlur={(e) => leavePeek(e.relatedTarget instanceof Element ? e.relatedTarget : null)}
      >
        {/* Barra05: sai o cabeçalho "Pessoal +"; a barra começa pela Donna. Criar fica no + de cada seção e os
            computadores no filtro da busca. A faixa de cima só move a janela. MX-122 v3: o seletor de Space subiu
            para a faixa do topo da janela (JanelaHeader); o Berga é a primeira coisa da barra. */}
        <div className="sidebar-drag drag" aria-hidden />
        <div className="donna-cards" aria-label={t('navegacao.janela.botsPrincipais')}>
          {roster.principalSections.map(({ computer, store }) => <DonnaCard key={computer.id} store={store} compact={compact} many={roster.principalSections.length > 1} listEmpty={quietEmpty}
            onActivate={openSpaceRecord && spacesStore ? () => openSpaceSheet(spacesStore, openSpaceRecord.id, 'principal') : undefined} edit={(bot) => openEditor(bot, store)}
            newSession={(bot) => setNewSessionBot({ bot, store })} createSession={canStartSession} startSession={(bot) => startSession(bot, store)} onSessions={sessionsPanel && !openSpaceRecord ? (bot) => openSessions(bot, store) : undefined} />)}
        </div>
        <RosterSearch roster={roster} computers={app.computers.length > 1 ? computerItems : undefined} onMenuOpenChange={setComputerMenu} />
        <div className="roster" ref={listRef} tabIndex={0} onKeyDown={onListKey}>
          <BotRoster roster={roster} compact={compact} onContext={(bot, store, x, y, extra, reorder) => setContextBot({ target: { bot, store }, x, y, extra, reorder })} onCreate={onlineStores.length ? create : undefined}
            onLibrary={!openSpaceRecord ? (id) => openLibrary(app.store(id)) : undefined} onSessions={sessionsPanel && !openSpaceRecord ? openSessions : undefined} onMenuOpenChange={setBotsMenu} quietEmpty={quietEmpty} />
          {roster.sections.every((section) => section.items.length === 0) && mismatchStores.length === 0 && listEmpty !== null ? (
            <div className="roster-empty">
              <Text as="div" size="body" weight="medium">{listEmpty}</Text>
            </div>
          ) : null}
        </div>
      </aside>
      ) : null}
      {collapsed ? null : <ResizeHandle label={t('navegacao.janela.redimensionar')} width={width} min={SIDEBAR_MIN - 40} max={SIDEBAR_WIDTH.max} edge="trailing" onResize={resizeSidebar} onReset={() => setSidebarWidth(SIDEBAR_WIDTH.default)} onDragging={dragSidebar} />}
      <main className="detail glass-panel" ref={detailRef}>
        {detailToggle ? (
          // Beside the floating sidebar while it is over the conversation, so the same button closes it (revisão R1, F2).
          <div className={`detail-sidebar-toggle no-drag${collapsed && peek ? ' beside-floating' : ''}`} style={{ ['--floating-width' as string]: `${width}px` }}>
            <button
              className="mac-chat-button ringed"
              data-sidebar-toggle
              title={comAtalho(t(sidebarShown ? 'navegacao.janela.recolherDica' : 'navegacao.janela.mostrarDica'), 'barraLateral', macDesteAparelho(), webDesteAparelho())}
              aria-label={t(sidebarShown ? 'navegacao.janela.recolher' : 'navegacao.janela.mostrar')}
              aria-pressed={sidebarShown}
              aria-keyshortcuts={atalhoAria('barraLateral', macDesteAparelho(), webDesteAparelho()) ?? undefined}
              onClick={toggleSidebar}
            >
              <Icon name="sidebar.left" size={16} weight="medium" />
            </button>
          </div>
        ) : null}
        {/* The call bar's settings open this window's Settings at Voice chat. */}
        <PersonalReadingVisible.Provider value={!routinesOpen && !settingsOpen}>
          <OpenVoiceSettings.Provider value={openVoiceSettings}><AbrirComputadores.Provider value={openComputersSettings}>
          {openSpaceRecord && spacesStore ? <SpaceScopeStatus app={app} root={spacesStore} space={openSpaceRecord} legacy={spaceView}
            onActivate={() => openSpaceSheet(spacesStore, openSpaceRecord.id, 'principal')}>{detail}</SpaceScopeStatus> : detail}
        </AbrirComputadores.Provider></OpenVoiceSettings.Provider>
        </PersonalReadingVisible.Provider>
        {/* D169 (28/10): the window's drag regions are applied in document order, so the conversation's top chrome
            (a drag strip, after the button) took the button back and a click only moved the window. This hole,
            last in the detail, gives the button's square back; the button keeps its place in the Tab order. */}
        {detailToggle ? <div aria-hidden className={`detail-sidebar-toggle is-hole no-drag${collapsed && peek ? ' beside-floating' : ''}`} style={{ ['--floating-width' as string]: `${width}px` }} /> : null}
      </main>
      {/* Nível 1 de atualizações: a pílula discreta da nova versão, no canto, fora das faixas de arrastar. */}
      <AvisoAtualizacao />

      {/* ajustes-tela-01: fora da `.split` (que isola as camadas), o menu da conta abre também sobre Ajustes e Rotinas. */}
      {showAccount ? createPortal(
        <AccountPanelLayer
          compact={false}
          approvals={app.approvals.length}
          onDismiss={() => setShowAccount(false)}
          onUsage={() => {
            setShowAccount(false)
            settingsScreen.show('usage')
          }}
          onSettings={() => {
            setShowAccount(false)
            settingsScreen.show('general')
          }}
        />, document.body,
      ) : null}
      {contextBot ? (
        <div className="context-layer idioma-superficie" {...superficie} onMouseDown={(e) => e.target === e.currentTarget && setContextBot(null)}>
          <DesktopActionMenu
            items={[...contextBot.extra, ...botActions(contextBot.target.bot, contextBot.target.store, contextBot.reorder).map((item, i) => (i === 0 && contextBot.extra.length ? { ...item, divider: true } : item))]}
            onDismiss={() => setContextBot(null)}
            style={{ left: Math.min(Math.max(8, contextBot.x), windowSize.width - 268), top: Math.min(Math.max(8, contextBot.y), Math.max(8, windowSize.height - 344)) }}
          />
        </div>
      ) : null}

      <SearchPalette open={searching} items={app.allRoster} onPick={(ref) => { closeSpace(); app.select(ref) }} onClose={() => setSearching(false)} />
      <Sheet language={superficieIdioma(idioma)} open={!!group} onClose={() => setEditing(null)} width={420} height={Math.min(680, sheetHeight)}>
        {editing && group ? (
          <StoreContext.Provider value={editing.store}>
            <GroupEditorView key={editing.id} group={group} />
          </StoreContext.Provider>
        ) : null}
      </Sheet>
      <Sheet language={superficieIdioma(idioma)} open={creating !== null} onClose={() => setCreating(null)} maxHeight={sheetHeight} label={creating && !createStore ? t('navegacao.janela.criacaoIndisponivel') : undefined}>
        {creating && createStore ? (
          <StoreContext.Provider value={createStore}>
            {/* A group's bots belong to its computer: another computer starts it over. */}
            {creating.kind === 'bot' ? <NewBotView key={JSON.stringify([creating.computerId, creating.space?.spaceId])} computer={computerChoice} spaceCreation={creating.space} availability={creationAvailability} onAbrirSpace={(id, botId) => openSpace(createStore, id, botId)} /> : <NewGroupView key={creating.computerId} computer={computerChoice} availability={creationAvailability} />}
          </StoreContext.Provider>
        ) : creating ? <div className="create-sheet idioma-superficie" {...superficie}>
          <Text role="alert" tone="danger">{t('navegacao.janela.criacaoHubIndisponivel')}</Text>
          <Button kind="secondary" onClick={() => setCreating(null)}>{t('navegacao.janela.cancelar')}</Button>
        </div> : null}
      </Sheet>
      <Sheet language={superficieIdioma(idioma)} open={spaceSheet !== null} onClose={() => setSpaceSheet(null)} width={Math.min(760, windowSize.width - 80)} maxHeight={sheetHeight} label={t(spaceSheet?.id ? 'navegacao.janela.editarSpace' : 'navegacao.janela.novoSpace')}>
        {spaceSheet ? <SpaceSheet key={spaceSheet.seq} origin={spaceSheet.origin} id={spaceSheet.id} initialMode={spaceSheet.mode} onClose={() => setSpaceSheet((current) => current?.seq === spaceSheet.seq ? null : current)} onDeleted={() => setSpaceProblem({ chaveTitulo: 'navegacao.janela.spaces', message: frase('spaces.delete.pronto') })} /> : null}
      </Sheet>
      <Sheet language={superficieIdioma(idioma)} open={library !== null} onClose={() => setLibrary(null)} maxHeight={sheetHeight}>
        {library ? (() => {
          const { store, target } = library
          const nav = libraryNavigation(target, {
            close: () => setLibrary(null),
            openEditor: (botId) => {
              const bot = store.bots.get(botId)
              if (bot) openEditor(bot, store)
              return !!bot
            },
            selectPersonal: (botId) => app.select({ computerId: store.computer.id, botId }),
            closeSpace,
            openSpace: (spaceId, conversationId) => openSpace(store, spaceId, conversationId),
          })
          return <BibliotecaView key={library.seq} target={target} onEditor={nav.onEditor} onConversation={nav.onConversation} />
        })() : null}
      </Sheet>
      <Dialog open={spaceProblem !== null} title={t(spaceProblem?.chaveTitulo ?? 'navegacao.janela.spaces')} message={spaceProblem ? textoDe(spaceProblem.message, idiomaJanela) : null} actions={[]} cancel={t('navegacao.janela.ok')} language={superficie} onClose={() => setSpaceProblem(null)} />
      <Dialog
        open={newSessionBot !== null}
        title={t('navegacao.janela.recomecar.titulo')}
        message={newSessionBot ? t('navegacao.janela.recomecar.mensagem', { bot: newSessionBot.bot.name }) : t('navegacao.janela.recomecar.mensagemBot')}
        actions={newSessionBot ? [{ title: t('navegacao.janela.recomecar.acao'), action: () => newSessionBot.store.newSession(newSessionBot.bot.id) }] : []}
        cancel={t('navegacao.janela.cancelar')}
        language={superficie}
        onClose={() => setNewSessionBot(null)}
      />
      <Dialog
        open={confirmDelete !== null}
        title={confirmDelete ? t('navegacao.janela.apagar.titulo', { bot: confirmDelete.bot.name }) : t('navegacao.janela.apagar.tituloBot')}
        message={t(confirmDelete && isGroup(confirmDelete.bot) ? 'navegacao.janela.apagar.canalFica' : 'navegacao.janela.apagar.arquivosFicam')}
        actions={confirmDelete ? [{ title: t(isGroup(confirmDelete.bot) ? 'navegacao.janela.apagar.canal' : 'navegacao.janela.apagar.bot'), destructive: true, action: () => confirmDelete.store.delete(confirmDelete.bot) }] : []}
        cancel={t('navegacao.janela.cancelar')}
        language={superficie}
        onClose={() => setConfirmDelete(null)}
      />
      <Dialog open={errorMessage !== null} title={t('navegacao.janela.erro')} message={errorMessage} actions={[]} cancel={t('navegacao.janela.ok')} language={superficie} onClose={() => app.clearErrors()} />
    </div>
    </OpenBotSettings.Provider>
  )
}

/** Identidade da conversa para montar a ThreadView: sem o hubId do refKey, que chega com o hello (MX-4 C10). */
const chatKey = (ref: BotRef) => `${ref.computerId}/${ref.spaceId ?? ''}/${ref.botId}`

/**
 * A fatia de uma loja que a moldura lê (MX-4 C9): conexão, versão, espaços, conversas e os bots
 * (nome, não lidas, horário e estado vivem no objeto — a AvatarBotMap troca a referência a cada
 * mudança). Fora dela ficam entradas e rascunhos, que a conversa assina por conta própria.
 */
function shellStoreSig(s: BotStore): readonly unknown[] {
  return [
    s.connection, s.shownConnection.kind, s.mismatch?.kind ?? null, s.hostVersion, s.hello,
    s.spaces, s.conversations, s.screen, s.lastError, s.accessRequests,
    ...s.bots.values(),
  ]
}

/** What can be open in front of the window and owns Escape (ajustes-tela-01): sheets, dialogs, menus and the account menu. */
const LAYERS_IN_FRONT = '[aria-modal], [role="dialog"], [role="alertdialog"], [role="menu"], .context-layer'

/** A focus target that is still there: connected, shown, not inert and not inside the closing sidebar. */
function canTakeFocus(el: Element | null | undefined): el is HTMLElement {
  return el instanceof HTMLElement && el.isConnected && el.getClientRects().length > 0 && !el.closest('[inert]') && !el.closest('aside.sidebar')
}

/** The chat window has the keyboard: no sheet or dialog made it inert, and focus is not in a dialog or menu. */
function shellOwnsKeyboard() {
  if (document.querySelector('.split')?.closest('[inert]')) return false
  const active = document.activeElement
  return !(active instanceof Element && active.closest('[role="dialog"], [role="alertdialog"], [role="menu"], [aria-modal="true"]'))
}

// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { spaceAllows } from '../lib/fork-space-capabilities'
import { useCallback, useEffect, useId, useRef, useState, type ReactNode, type RefObject } from 'react'
import { superficieIdioma, useIdioma, useT } from '../lib/fork-i18n/estado'
import { createPortal } from 'react-dom'
import { isGroup, screenAgent, type Bot } from '@shared/models'
import { moveInOrder } from '../lib/computer-roster'
import { mergeOrder } from '../lib/busca'
import { pointerDrag, type Point } from '../lib/pointer-drag'
import { reduceMotion } from '../lib/theme'
import { useSlide } from '../lib/slide'
import { sameRef, refKey, type RosterItem } from '../store/app-model'
import type { BotStore } from '../store/bot-store'
import type { FilteredRoster } from './sidebar-busca/RosterSearch'
import type { TimesSnapshot } from '../../shared/fork-times'
import { StoreContext, useApp } from '../store/context'
import { BotRow, rowDescription, type RowTag } from './BotRow'
import { Icon } from '../components/Icon'
import { DropdownMenu } from '../components/Controls'
import type { MenuItem } from '../components/Overlay'
import { prefs, usePref } from '../lib/prefs'
import { canCreateOn } from '../lib/creation-target'
import { expandLegacySections, splitChannels, visibleSections, type SidebarSection } from '../lib/sidebar-layout'
import { ComputerRosterSection } from './ComputerRosterSection'
import { UpdateNeededCard } from './UpdateNeededCard'
import { TimesSidebarList, type TeamRowOptions } from './fork/times-sidebar/TimesSidebarList'
import { TeamRoom } from './fork/times-sidebar/TeamRoom'
import { AgentGroups, agentsAddItems, moveToGroupItems } from './fork/times-sidebar/AgentGroups'
import { isOpenDesktop } from '@shared/fork-capacidades'
import { updatePresentation } from './fork/times-sidebar/prefs'
import { setTimesOpen, teamsToReveal, type TimesGroup } from '../lib/times-roster'
import { botGroupTarget, groupOf, UNGROUPED } from '../lib/bot-groups'
import { Fold, warmFold } from './fork/times-sidebar/Fold'
import { rosterSearch } from './sidebar-busca/RosterSearch'
import { useModel } from '../lib/observable'
import { timesScreen } from './fork/times/navigation'

/** A bot being dragged among its computer's bots, and the order it has made so far. */
interface BotDrag {
  item: RosterItem
  ids: string[]
  domain: string | null
  scope: number
  snapshot: TimesSnapshot | null
  teamId: string | null
}

/** The lifted copy of a dragged row: its width, and where in it the pointer holds it. */
interface Lift {
  item: RosterItem
  width: number
  grab: Point
  at: Point
  seq: number
}

export function BotRoster({ roster, compact, onContext, onCreate, onLibrary, onSessions, onMenuOpenChange, quietEmpty = false }: {
  roster: FilteredRoster
  compact: boolean
  /** `extra`: what the sidebar adds to the bot's menu (the team's block, Sessões, Mover para grupo). */
  onContext: (bot: Bot, store: BotStore, x: number, y: number, extra: MenuItem[], reorder: boolean) => void
  /** The + in a section's heading: a new channel (group) or bot, on that section's computer. */
  onCreate?: (kind: 'bot' | 'group', computerId: string) => void
  /** Barra05: Biblioteca de especialistas, no + de Agentes. */
  onLibrary?: (computerId: string) => void
  /**
   * Passo 12 (gancho do executor B, `store.openSessionsPanel(botId)`): dois cliques nas linhas sem expansor, ⌃↩ e
   * "Sessões de …" no menu abrem o painel direito na aba Sessões. Ausente, nada acontece.
   */
  onSessions?: (bot: Bot, store: BotStore) => void
  /** The + of Bots opened or closed its menu: the floating sidebar stays while it is open. */
  onMenuOpenChange?: (open: boolean) => void
  /** Auditoria D249, F7: the whole list is empty and the conversation side says so; sections show only titles. */
  quietEmpty?: boolean
}) {
  const t = useT()
  const idioma = useIdioma()
  const app = useApp()
  const [order] = usePref(prefs.sidebarSectionOrder)
  const [hiddenSections] = usePref(prefs.sidebarHiddenSections)
  const [foldedSaved, setFolded] = usePref(prefs.sidebarFoldedSections)
  // Barra04: a antiga Bots vale para Equipes e Agentes.
  const folded = expandLegacySections(foldedSaved)
  const filtering = roster.filtering ?? false
  const { query } = useModel(rosterSearch)
  // Sala da equipe (passo 11): só em memória, um computador por vez.
  const [room, setRoom] = useState<{ computerId: string; teamId: string } | null>(null)
  const leaveRoom = useCallback(() => setRoom(null), [])
  // O grupo de Agentes sob o bot que está sendo arrastado.
  const [agentDrop, setAgentDrop] = useState<string | null>(null)
  const selected = app.selection
  const [hovered, setHovered] = useState<string | null>(null)
  const [dragged, setDragged] = useState<string | null>(null)
  const [botDrag, setBotDragState] = useState<BotDrag | null>(null)
  // The drag's handlers outlive the render that started it, so they read the latest drag here.
  const drag = useRef<BotDrag | null>(null)
  const setBotDrag = (next: BotDrag | null) => {
    drag.current = next
    setBotDragState(next)
  }
  const [lift, setLift] = useState<Lift | null>(null)
  const ghost = useRef<HTMLDivElement>(null)
  const grab = useRef<Point>({ x: 0, y: 0 })
  const settle = useRef<ReturnType<typeof setTimeout> | null>(null)
  // Near the roster's top or bottom edge a drag scrolls it, from the last point the pointer was at.
  const pointer = useRef<Point>({ x: 0, y: 0 })
  const edgeScroll = useRef(0)
  const itemsByKey = useRef(new Map<string, RosterItem>())
  itemsByKey.current = new Map(roster.sections.flatMap((s) => s.items).map((item) => [refKey(item.ref), item]))

  /** A section's rows, in the order a drag in progress has them. */
  const ordered = (computerId: string, items: RosterItem[]) => {
    if (botDrag?.item.ref.computerId !== computerId) return items
    const rank = new Map(botDrag.ids.map((id, index) => [id, index]))
    return [...items].sort((a, b) => (rank.get(a.bot.id) ?? items.indexOf(a)) - (rank.get(b.bot.id) ?? items.indexOf(b)))
  }

  // The row lifts: a copy follows the pointer from where it was grabbed; the row's slot stays open.
  const liftRow = (item: RosterItem, siblings: RosterItem[], from: Point, agent = false) => {
    // A lift during the last one's settle takes over from it.
    if (settle.current) clearTimeout(settle.current)
    settle.current = null
    const rect = rowRect(item)
    const model = item.store.timesRoster
    const group = roster.timesByComputer.get(item.ref.computerId)?.groups.find((g) => g.rows.some((i) => i.bot.id === item.bot.id))
    if (!agent && !isGroup(item.bot) && (!group || item.bot.id === group.chiefId)) return
    setBotDrag({ item, ids: siblings.map((s) => s.bot.id), domain: model.domain, scope: model.scope, snapshot: model.snapshot, teamId: agent ? AGENT_DRAG : group?.id ?? null })
    if (!rect) return
    grab.current = { x: from.x - rect.left, y: from.y - rect.top }
    setLift((last) => ({ item, width: rect.width, grab: grab.current, at: from, seq: (last?.seq ?? 0) + 1 }))
  }

  const dragOver = (at: Point, under: Element | null) => {
    pointer.current = at
    if (ghost.current) ghost.current.style.translate = ghostTranslate(at, grab.current)
    // Agentes: arrastar muda o bot de grupo (D114); o alvo é o grupo sob o ponteiro, do mesmo computador.
    if (drag.current?.teamId === AGENT_DRAG) setAgentDrop(botGroupTarget(under, drag.current.item.ref.computerId) ?? null)
    else makeRoom(under)
    if (!edgeScroll.current) scrollAtEdge()
  }

  const scrollAtEdge = () => {
    const current = drag.current
    const scroller = current && rowElement(current.item)?.closest<HTMLElement>('.roster')
    if (!scroller) return void (edgeScroll.current = 0)
    const box = scroller.getBoundingClientRect()
    const { x, y } = pointer.current
    const reach = Math.max(0, box.top + EDGE - y) - Math.max(0, y - (box.bottom - EDGE))
    if (!reach) return void (edgeScroll.current = 0)
    scroller.scrollTop -= Math.round((reach / EDGE) * EDGE_SPEED)
    makeRoom(document.elementFromPoint(x, y))
    edgeScroll.current = requestAnimationFrame(scrollAtEdge)
  }

  // Any row of the same computer is a target; only rows with the same pin state make room.
  const makeRoom = (under: Element | null) => {
    const current = drag.current
    const row = under?.closest<HTMLElement>('[data-roster-row]')
    const item = row && itemsByKey.current.get(row.dataset.rosterRow!)
    if (!current || !row || !item || current.item.ref.computerId !== item.ref.computerId) return
    // A row still sliding out of the way would be hit again where it was.
    if (item.bot.id === current.item.bot.id || item.bot.pinned !== current.item.bot.pinned || isGroup(item.bot) !== isGroup(current.item.bot) || row.getAnimations().length) return
    if (!isGroup(item.bot)) {
      const projection = roster.timesByComputer.get(item.ref.computerId)
      const group = projection?.groups.find((g) => g.id === current.teamId)
      if (!group?.rows.some((i) => i.bot.id === item.bot.id) || item.bot.id === group.chiefId
        || projection?.parents[item.bot.id] !== projection?.parents[current.item.bot.id]) return
      const model = item.store.timesRoster
      if (model.domain !== current.domain || model.scope !== current.scope || model.snapshot !== current.snapshot) return
    }
    setBotDrag({ ...current, ids: moveInOrder(current.ids, current.item.bot.id, item.bot.id) })
  }

  const endDrag = () => {
    cancelAnimationFrame(edgeScroll.current)
    edgeScroll.current = 0
    settle.current = null
    setBotDrag(null)
    setLift(null)
    setAgentDrop(null)
  }

  // The copy settles into the row's new slot (when it's in view), then the row shows again.
  const drop = () => {
    const current = drag.current
    if (!current) return endDrag()
    cancelAnimationFrame(edgeScroll.current)
    if (current.teamId === AGENT_DRAG) {
      const target = agentDrop
      const snapshot = current.item.store.botGroups.snapshot
      const from = snapshot ? (groupOf(snapshot, current.item.bot.id) ?? UNGROUPED) : null
      if (target && snapshot && spaceAllows(current.item.store.spaceId, 'teams') && target !== from) void current.item.store.botGroups.edit({ op: 'place', botId: current.item.bot.id, groupId: target === UNGROUPED ? null : target }).catch(() => {})
      return endDrag()
    }
    if (isGroup(current.item.bot)) {
      // Grupos conserva a escrita atual no Hub, somente entre grupos visíveis.
      const ids = mergeOrder(current.item.store.roster.map((bot) => bot.id), current.ids)
      if (ids.some((id, index) => id !== current.item.store.roster[index]?.id)) current.item.store.reorder(ids)
    } else {
      const model = current.item.store.timesRoster
      if (model.domain !== current.domain || model.scope !== current.scope || model.snapshot !== current.snapshot) return endDrag()
      const group = roster.timesByComputer.get(current.item.ref.computerId)?.groups.find((g) => g.id === current.teamId)
      if (!group) return endDrag()
      updatePresentation(current.domain, (p) => ({ ...p, members: { ...p.members, [group.id]: mergeOrder(group.items.map((i) => i.bot.id), current.ids) } }))
    }
    const row = rowElement(current.item)
    // Measured where the row ends up, not partway through its slide.
    for (const animation of row?.getAnimations() ?? []) animation.finish()
    const slot = row?.getBoundingClientRect()
    const view = row?.closest('.roster')?.getBoundingClientRect()
    const visible = slot && view && slot.bottom > view.top && slot.top < view.bottom
    if (!slot || !visible || !ghost.current || reduceMotion()) return endDrag()
    ghost.current.classList.add('settling')
    ghost.current.style.translate = `${slot.left}px ${slot.top}px`
    settle.current = setTimeout(endDrag, SETTLE_MS)
  }

  /** One roster row. `team`: Equipes passes the row's level and tag; `agent`: Agentes (drag changes the group). */
  const row = (item: RosterItem, siblings: RosterItem[], options: { level?: number; tag?: RowTag | null; lead?: boolean; group?: TimesGroup<RosterItem>; onDoubleClick?: () => void; describe?: string[]; agent?: boolean } = {}) => {
    const key = refKey(item.ref)
    const isSelected = sameRef(item.ref, selected)
    // The held row shows as hovered, or as an empty slot while its lifted copy follows the pointer.
    const held = botDrag?.item.bot.id === item.bot.id && botDrag.item.ref.computerId === item.ref.computerId
    const many = roster.sections.length > 1
    const described = [item.bot.name, ...(options.describe ?? []), options.tag && !options.lead ? options.tag.text : null, many ? t('navegacao.barra.emHub', { hub: item.store.hostName }) : null,
      item.bot.unread > 0 ? t.plural('navegacao.barra.naoLidas', item.bot.unread) : null, rowDescription(t, idioma, item.bot, item.store)].filter(Boolean)
    const sessions = onSessions && !isGroup(item.bot) ? () => onSessions(item.bot, item.store) : undefined
    const extra = (): MenuItem[] => [
      ...(sessions ? [{ title: t('navegacao.barra.sessoesDe', { bot: item.bot.name }), detail: '⌃↩', action: sessions }] : []),
      ...(options.lead && options.group ? teamMenu(item.store, options.group) : []),
      ...(options.agent ? moveToGroupItems(item.store, item.bot) : []),
    ]
    // Revealing a bot found by search keeps its teams open (TeamGroup: "abrir alguém escondido abre a equipe dele").
    const open = () => {
      if (options.group?.temporary || (options.group && !options.lead && filtering)) reveal(item)
      app.select(item.ref)
    }
    return (
      <RosterRow
        key={key}
        hovered={hovered === key || held}
        compact={compact}
        label={described.join(', ')}
        title={described.join(' · ')}
        selected={isSelected}
        dragKey={key}
        onHover={(on) => setHovered((h) => (on ? key : h === key ? null : h))}
        onClick={open}
        // Linhas sem expansor: dois cliques abrem as sessões (painel direito). No chefe, abrem ou fecham a equipe.
        onDoubleClick={options.onDoubleClick ?? sessions}
        onKey={(e) => {
          if (e.key === 'Enter' && e.ctrlKey && sessions) { e.preventDefault(); e.stopPropagation(); sessions() }
        }}
        // Mover para cima/baixo só vale para canais e membros de equipe (a ordem local); chefes e agentes não reordenam.
        onContext={(x, y) => onContext(item.bot, item.store, x, y, extra(), isGroup(item.bot) || siblings.length > 1)}
        lifted={held && lift !== null}
        drag={pointerDrag({ start: (from) => liftRow(item, siblings, from, options.agent), over: dragOver, drop, cancel: endDrag })}
      >
        <BotRow bot={item.bot} store={item.store} compact={compact} usingComputer={screenAgent(item.store.screen) === item.bot.id} level={options.level ?? 1} tag={options.tag?.text ?? null} tone={options.tag?.tone ?? 'l0'} lead={options.lead ?? false} />
      </RosterRow>
    )
  }

  /** The team's own block in its chief's menu (TeamGroup: Entrar na equipe, Mostrar/Esconder, Organizar no quadro). */
  const teamMenu = (store: BotStore, group: TimesGroup<RosterItem>): MenuItem[] => [
    { title: t('navegacao.barra.entrarEquipe', { equipe: group.name }), detail: '⇧↩', divider: true, action: () => setRoom({ computerId: store.computer.id, teamId: group.id }) },
    ...(!group.temporary && group.below.length ? [{ title: t(group.open ? 'navegacao.barra.esconderMembros' : 'navegacao.barra.mostrarMembros'), detail: group.open ? '←' : '→', action: () => {
      const domain = store.timesRoster.domain
      updatePresentation(domain, (p) => setTimesOpen(p, group.id, !group.open))
    } }] : []),
    { title: t('navegacao.janela.menu.organizar'), disabled: !spaceAllows(store.spaceId, 'teams'), icon: 'person.2', action: () => timesScreen.show(null, store.computer.id) },
  ]

  /** Opens and keeps open the teams above a bot (and its own, unless it is the chief). */
  const reveal = (item: RosterItem) => {
    const model = item.store.timesRoster
    const ids = teamsToReveal(model.snapshot, item.bot.id)
    if (ids.length) updatePresentation(model.domain, (p) => ids.reduce((next, id) => setTimesOpen(next, id, true), p))
  }

  // A bot opened from outside the list (a notification, the palette) that sits in a closed team: open its teams.
  useEffect(() => {
    if (!selected || filtering) return
    const projection = roster.timesByComputer.get(selected.computerId)
    if (!projection || projection.rows.some((i) => i.bot.id === selected.botId)) return
    const item = roster.items.find((i) => sameRef(i.ref, selected))
    if (item) reveal(item)
  }, [selected?.computerId, selected?.botId])

  const sections = roster.sections.map(({ computer, store, items }, index) => {
    const addDisabled = !canCreateOn(store)
    // Sempre montado: o cartão decide sozinho se aparece, e a confirmação de que voltou ao normal precisa de quem a mostre.
    const notice = !compact ? <StoreContext.Provider value={store}><UpdateNeededCard /></StoreContext.Provider> : null
    const parts = splitChannels(items)
    const projection = roster.timesByComputer.get(computer.id)!
    const sectionFold = (id: SidebarSection) => ({
      folded: !filtering && folded.includes(id),
      toggle: () => setFolded(toggled(folded, id)),
    })
    const create = onCreate && !filtering && !addDisabled ? onCreate : undefined
    // Sala da equipe: a lista mostra só ela (Canais e Agentes saem enquanto se está na sala).
    if (room?.computerId === computer.id) {
      const roomProjection = roster.roomFor(computer.id)
      const rows = roomProjection ? <TeamRoom projection={roomProjection} teamId={room.teamId} filtering={filtering} query={query}
        onGo={(teamId) => (teamId ? setRoom({ computerId: computer.id, teamId }) : leaveRoom())}
        onOrganize={() => timesScreen.show(null, store.computer.id)}
        onOpen={(item) => app.select(item.ref)}
        renderRow={(item, tag) => row(item, [], { level: 1, tag, describe: tag ? [tag.text] : [] })} /> : null
      return <div key={computer.id}>{notice}{rows}</div>
    }
    const blocks: Record<SidebarSection, ReactNode> = {
      channels: parts.channels.length || (!filtering && onCreate) ? (
        <SidebarSectionBlock key="channels" id="channels" foldKey={`section|${computer.id}|channels`} title={t('navegacao.barra.secao.canais')} icon="number" count={parts.channels.length} {...sectionFold('channels')}
          onAdd={create ? () => create('group', computer.id) : undefined} addTitle={t('navegacao.barra.novoCanal')} addDisabled={addDisabled} filtering={filtering}>
          {ordered(computer.id, parts.channels).map((item) => row(item, parts.channels))}
          {!parts.channels.length && !quietEmpty ? <p className="sidebar-section-empty">{t('navegacao.barra.semCanais')}</p> : null}
        </SidebarSectionBlock>
      ) : null,
      teams: !isOpenDesktop(window.codync) && (projection.groups.length || !filtering) ? (
        <SidebarSectionBlock key="teams" id="teams" foldKey={`section|${computer.id}|teams`} title={t('navegacao.barra.secao.equipes')} icon="person.2" count={projection.groups.length} {...sectionFold('teams')}
          onAdd={!filtering ? () => timesScreen.show(null, computer.id) : undefined} addTitle={t('navegacao.barra.novaEquipe')} addDisabled={!spaceAllows(store.spaceId, 'teams')} filtering={filtering}>
          {!spaceAllows(store.spaceId, 'teams') ? <p className="sidebar-section-empty">{t('spaces.selector.unavailable')}</p> : <TimesSidebarList store={store} projection={projection} compact={compact}
            onEnter={(group) => setRoom({ computerId: computer.id, teamId: group.id })}
            onOpen={(item) => app.select(item.ref)}
            arrange={(members) => ordered(computer.id, members)}
            renderRow={(item, o: TeamRowOptions) => row(item, o.lead ? [] : o.group.rows.filter((i) => i.bot.id !== o.group.chiefId), o)} />}
        </SidebarSectionBlock>
      ) : null,
      agents: projection.simple.length || !filtering ? (
        <SidebarSectionBlock key="agents" id="agents" foldKey={`section|${computer.id}|agents`} title={t('navegacao.barra.secao.agentes')} icon="donna.bot" count={projection.simple.length} {...sectionFold('agents')}
          addTitle={t('navegacao.barra.novoAgente')} addDisabled={addDisabled} filtering={filtering}
          menu={!filtering && !addDisabled ? () => agentsAddItems(store, create ? () => create('bot', computer.id) : undefined, onLibrary ? () => onLibrary(computer.id) : undefined) : undefined}
          onMenuOpenChange={onMenuOpenChange}>
          <AgentGroups store={store} items={projection.simple} compact={compact} filtering={filtering} dropTarget={botDrag?.item.ref.computerId === computer.id ? agentDrop : null}
            renderRow={(item) => row(item, [], { agent: true, tag: item.bot.title ? { text: item.bot.title, tone: 'l0' } : null })} />
          {!projection.simple.length && !filtering && !quietEmpty ? <p className="sidebar-section-empty">{t('navegacao.barra.semEquipe')}</p> : null}
        </SidebarSectionBlock>
      ) : null,
    }
    const rows = <>{visibleSections(order, hiddenSections).map((id) => blocks[id])}</>
    if (!roster.grouped) return <div key={computer.id}>{notice}{rows}</div>
    return (
      <ComputerRosterSection key={computer.id} store={store} compact={compact}
        collapsed={roster.collapsed.includes(computer.id)} toggle={() => roster.toggle(computer.id)}
        previous={roster.sections[index - 1]?.computer.id} next={roster.sections[index + 1]?.computer.id}
        dragged={dragged} setDragged={setDragged} move={roster.move} notice={notice}>
        {rows}
        {!items.length && !store.mismatch && !compact && store.shownConnection.kind !== 'online' ? <div className="computer-roster-empty">{store.connectionLabel}</div> : null}
      </ComputerRosterSection>
    )
  })
  return (
    <>
      {sections}
      {lift ? <RosterGhost key={lift.seq} lift={lift} ghost={ghost} compact={compact} /> : null}
    </>
  )
}

/** The lifted row under the pointer (iOS's lifted cell): a smaller, translucent copy that follows it. */
function RosterGhost({ lift, ghost, compact }: { lift: Lift; ghost: RefObject<HTMLDivElement | null>; compact: boolean }) {
  return createPortal(
    <div ref={ghost} className="roster-ghost" style={{ width: lift.width, translate: ghostTranslate(lift.at, lift.grab) }}>
      <div className={`roster-row roster-ghost-card ${compact ? 'compact' : ''}`} style={{ transformOrigin: `${lift.grab.x}px ${lift.grab.y}px` }}>
        <RosterBotRow item={lift.item} compact={compact} />
      </div>
    </div>,
    document.body,
  )
}

function RosterBotRow({ item, compact }: { item: RosterItem; compact: boolean }) {
  return <BotRow bot={item.bot} store={item.store} compact={compact} usingComputer={screenAgent(item.store.screen) === item.bot.id} />
}

/** One roster button: drags to reorder, and slides when the others move. */
function RosterRow({ hovered, lifted, compact, label, title, selected, dragKey, onHover, onClick, onDoubleClick, onKey, onContext, drag, children }: {
  hovered: boolean
  /** Its copy is under the pointer: the row keeps its slot empty. */
  lifted: boolean
  compact: boolean
  label: string
  title: string
  selected: boolean
  dragKey: string
  onHover: (on: boolean) => void
  onClick: () => void
  onDoubleClick?: () => void
  onKey?: (e: React.KeyboardEvent<HTMLButtonElement>) => void
  onContext: (x: number, y: number) => void
  drag: ReturnType<typeof pointerDrag>
  children: ReactNode
}) {
  const ref = useRef<HTMLButtonElement>(null)
  useSlide(ref)
  return (
    <button
      ref={ref}
      className={`roster-row ${compact ? 'compact' : ''} ${hovered ? 'hovered' : ''} ${selected ? 'selected' : ''} ${lifted ? 'lifted' : ''}`}
      aria-label={label}
      aria-current={selected || undefined}
      title={title}
      data-roster-row={dragKey}
      onMouseEnter={() => onHover(true)}
      onMouseLeave={() => onHover(false)}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      onKeyDown={onKey}
      onContextMenu={(e) => {
        e.preventDefault()
        onContext(e.clientX, e.clientY)
      }}
      {...drag}
    >
      {children}
    </button>
  )
}

const toggled = (list: string[], id: string) => (list.includes(id) ? list.filter((x) => x !== id) : [...list, id])

/**
 * A sidebar section (Canais, Equipes, Agentes): the icon tells them apart, the title folds it (the chevron shows only on
 * hover, focus or folded) and the + makes one more. Folding animates the body's height (lib/fold.ts).
 */
function SidebarSectionBlock({ id, foldKey, title, icon, count, folded, toggle, onAdd, addTitle, addDisabled, filtering, menu, onMenuOpenChange, children }: {
  id: SidebarSection
  foldKey: string
  title: string
  icon: string
  count: number
  folded: boolean
  toggle: () => void
  onAdd?: () => void
  addTitle: string
  addDisabled: boolean
  filtering: boolean
  /** With it, the + opens these choices instead of running `onAdd`. */
  menu?: () => MenuItem[]
  onMenuOpenChange?: (open: boolean) => void
  children: ReactNode
}) {
  // Unique per block: each computer has its own Canais, Equipes and Agentes (C4).
  const body = `${useId()}-${id}`
  const t = useT()
  const superficie = superficieIdioma(useIdioma())
  const unavailable = t('navegacao.barra.criarIndisponivel')
  return (
    <section className={`sidebar-section is-${id}${folded ? ' is-folded' : ''}${id === 'teams' ? '' : ' idioma-superficie'}`} {...(id === 'teams' ? {} : superficie)} aria-label={title}>
      <div className="sidebar-section-head idioma-superficie" {...superficie}>
        {/* D169 (ChannelList): the title shows no count; VoiceOver still hears it. */}
        <button className="sidebar-section-label" aria-expanded={!folded} aria-controls={body} disabled={filtering} onClick={toggle}
          onPointerEnter={() => warmFold(foldKey)} onFocus={() => warmFold(foldKey)}
          aria-label={t('navegacao.barra.secaoAria', { secao: title, n: count })} title={t(folded ? 'navegacao.barra.mostrarSecao' : 'navegacao.barra.recolherSecao', { secao: title.toLocaleLowerCase() })}>
          <Icon name={icon} size={12} weight="medium" className="sidebar-section-icon" />
          <span>{title}</span>
          <Icon name="chevron.down" size={8} weight="semibold" className="sidebar-section-chevron" />
        </button>
        {menu ? (
          <DropdownMenu language={superficie} className="sidebar-section-add" title={addDisabled ? unavailable : addTitle} disabled={addDisabled} items={menu} onOpenChange={onMenuOpenChange}>
            <Icon name="plus" size={11} weight="medium" />
          </DropdownMenu>
        ) : onAdd ? (
          <button className="sidebar-section-add" title={addDisabled ? unavailable : addTitle} aria-label={addTitle} disabled={addDisabled} onClick={onAdd}>
            <Icon name="plus" size={11} weight="medium" />
          </button>
        ) : null}
      </div>
      <Fold open={!folded} foldKey={foldKey} id={body} className="sidebar-section-body">{children}</Fold>
    </section>
  )
}

/** A dragged agent's marker: the drag changes its group instead of reordering (Agentes, D114). */
const AGENT_DRAG = '$agent'

/** How long the lifted copy takes to settle into its slot (`--morph`). */
const SETTLE_MS = 200

/** How close to the roster's edge a drag starts scrolling it, and how fast (points per frame). */
const EDGE = 36
const EDGE_SPEED = 12

function rowElement(item: RosterItem): HTMLElement | null {
  return document.querySelector<HTMLElement>(`[data-roster-row="${CSS.escape(refKey(item.ref))}"]`)
}

/** Where a row is on screen now. */
function rowRect(item: RosterItem): DOMRect | undefined {
  return rowElement(item)?.getBoundingClientRect()
}

/** The lifted copy's `translate` that keeps the point it was grabbed by under the pointer. */
function ghostTranslate(at: Point, grab: Point): string {
  return `${at.x - grab.x}px ${at.y - grab.y}px`
}

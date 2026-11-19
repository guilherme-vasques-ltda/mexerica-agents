// Mexerica D280: busca, desenho e teclado partilham a projeção de Times salva.
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
// A preferência do filtro e o recolhimento ficam neste aparelho.
import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { Icon } from '../../components/Icon'
import { AnchoredMenu, type MenuItem } from '../../components/Overlay'
import { ALL, matchesWords, mergeOrder, searchWords } from '../../lib/busca'
import { moveInOrder } from '../../lib/computer-roster'
import { paletteProjections } from '../../lib/principal'
import { moveTimesMember, NO_TEAM, nextTimesSibling, projectTimes, teamKey, type TimesProjection } from '../../lib/times-roster'
import { presentationFor, timesFilter, timesPresentations, updatePresentation } from '../fork/times-sidebar/prefs'
import { Observable, useModel, useModels } from '../../lib/observable'
import { prefs, usePref } from '../../lib/prefs'
import { atalhoAria, atalhoDoApp, macDesteAparelho } from '../../lib/atalhos'
import { navigableRows, splitChannels, visibleSections } from '../../lib/sidebar-layout'
import type { BotRef, RosterItem } from '../../store/app-model'
import type { useComputerRoster } from '../../store/computer-roster'
import { useApp } from '../../store/context'
import './roster-search.css'

type Roster = ReturnType<typeof useComputerRoster>

interface SectionChoice {
  id: string
  name: string
  computerId: string
  sectionId: string
}

export type FilteredRoster = Roster & {
  principalSections: Roster['sections']
  filtering: boolean
  timesByComputer: Map<string, TimesProjection<RosterItem>>
  reorderTimes: (item: RosterItem, target: RosterItem) => void
  teamChoices: SectionChoice[]
  sectionId: string
  setSection: (id: string) => void
  /** Barra04, sala da equipe: a projeção daquele computador com todas as equipes abertas (a busca vale dentro dela). */
  roomFor: (computerId: string) => TimesProjection<RosterItem> | null
}

// Fatia 1 (D454): as chaves do catálogo (navegacao.barra.busca.*); o texto sai com `t` no idioma do app.
const texts = {
  search: 'navegacao.barra.busca.buscar',
  searchLabel: 'navegacao.barra.busca.rotulo',
  clear: 'navegacao.barra.busca.limpar',
  allSections: 'navegacao.barra.busca.todas',
  agents: 'navegacao.barra.secao.agentes',
  filter: 'navegacao.barra.busca.filtrar',
  region: 'navegacao.barra.busca.regiao',
} as const

/** O texto da busca (não fica guardado) e o campo, para o ⌘F. */
class RosterQuery extends Observable {
  query = ''
  input: HTMLInputElement | null = null

  set(query: string) {
    this.query = query
    this.changed()
  }

  /** ⌘F: foca a busca da barra lateral aberta. No trilho compacto (sem a busca) fica a paleta. */
  focus(compact: boolean): boolean {
    const input = this.input
    if (compact || !input || !input.isConnected || input.closest('[inert]')) return false
    input.focus()
    input.select()
    return true
  }
}

export const rosterSearch = new RosterQuery()
/** A mesma projeção determina desenho, filtro e navegação; DTO não amplia o roster. */
export function useRosterFilter(roster: Roster): FilteredRoster {
  const { query } = useModel(rosterSearch)
  const t = useT()
  const [savedSection, setSection] = usePref(timesFilter)
  const [savedPresentations] = usePref(timesPresentations)
  const [sectionOrderPref] = usePref(prefs.sidebarSectionOrder)
  const [hiddenSections] = usePref(prefs.sidebarHiddenSections)
  const [foldedSections] = usePref(prefs.sidebarFoldedSections)
  useModels(roster.sections.map((section) => section.store.timesRoster))
  useModels(roster.sections.map((section) => section.store.principal))
  const projections = paletteProjections(roster.sections)
  const teamChoices: SectionChoice[] = projections.flatMap(({ section, others }) => {
    const model = section.store.timesRoster
    if (!model.snapshot || !model.domain) return []
    // Barra04: as equipes do topo e Agentes (os bots sem equipe).
    const host = roster.sections.length > 1 ? ` · ${section.store.hostName}` : ''
    const roots = projectTimes(splitChannels(others).bots, model.snapshot, { ...presentationFor(model.domain, savedPresentations), filter: null }, '', section.store.principal.protectedId).groups.filter((g) => g.parentId === null)
    return [...roots.map((g) => ({ id: g.id, name: g.name })), { id: NO_TEAM, name: t(texts.agents) }].map((g) => ({
      id: JSON.stringify([model.domain, model.scope, g.id]), name: g.name + host, computerId: section.computer.id, sectionId: g.id,
    }))
  })
  const choice = teamChoices.find((section) => section.id === savedSection)
  const sectionId = choice?.id ?? ALL
  const words = searchWords(query)
  const filtering = words.length > 0 || sectionId !== ALL
  const timesByComputer = new Map<string, TimesProjection<RosterItem>>()
  const sections = projections.map(({ section, others }) => {
    const { channels, bots } = splitChannels(others)
    const model = section.store.timesRoster
    const pref = { ...presentationFor(model.domain, savedPresentations), filter: choice?.computerId === section.computer.id ? choice.sectionId : null }
    const times = projectTimes(bots, model.snapshot, pref, query, section.store.principal.protectedId)
    // Com filtro de outro computador, não fabricar Sem time nem mostrar grupos fora do filtro.
    if (choice && choice.computerId !== section.computer.id) {
      times.groups = []; times.rows = []; times.simple = []
    }
    timesByComputer.set(section.computer.id, times)
    return { ...section, items: [...(choice ? [] : channels.filter((i) => matchesWords(words, [i.bot.name]))), ...times.rows] }
  }).filter((section) => !filtering || section.items.length > 0 || (timesByComputer.get(section.computer.id)?.groups.length ?? 0) > 0)
  const navigable = sections.flatMap((section) => {
    if (roster.grouped && roster.collapsed.includes(section.computer.id)) return []
    const { channels } = splitChannels(section.items)
    const folded = filtering ? [] : foldedSections
    const times = timesByComputer.get(section.computer.id)
    return navigableRows(visibleSections(sectionOrderPref, hiddenSections), folded, { channels, teams: times?.groups.flatMap((g) => g.rows) ?? [], agents: times?.simple ?? [] })
  })
  const reorderTimes = (item: RosterItem, target: RosterItem) => {
    if (item.store !== target.store || item.ref.computerId !== target.ref.computerId) return
    const projection = timesByComputer.get(item.ref.computerId)
    const model = item.store.timesRoster
    if (!projection || !model.snapshot) return
    // A resposta pode ter mudado a origem/vínculos desde o começo do gesto.
    const current = projectTimes(splitChannels(projections.find((p) => p.section.store === item.store)?.others ?? []).bots,
      model.snapshot, { ...presentationFor(model.domain), filter: choice?.computerId === item.ref.computerId ? choice.sectionId : null }, query, item.store.principal.protectedId)
    updatePresentation(model.domain, (pref) => moveTimesMember(current, pref, item.bot.id, target.bot.id))
  }
  const roomFor = (computerId: string) => {
    const p = projections.find((x) => x.section.computer.id === computerId)
    const model = p?.section.store.timesRoster
    if (!p || !model?.snapshot) return null
    const open = model.snapshot.nodes.map((n) => teamKey(n.id))
    return projectTimes(splitChannels(p.others).bots, model.snapshot, { ...presentationFor(model.domain, savedPresentations), open, filter: null }, query, p.section.store.principal.protectedId)
  }
  return {
    ...roster, items: projections.flatMap((p) => p.palette), principalSections: roster.sections,
    sections, navigable, filtering, teamChoices, sectionId, setSection, timesByComputer, reorderTimes, roomFor,
    moveBot(ref: BotRef, offset: number) {
      const section = sections.find((s) => s.computer.id === ref.computerId)
      if (!section || !navigable.some((i) => i.ref.computerId === ref.computerId && i.bot.id === ref.botId)) return
      const { channels } = splitChannels(section.items)
      const group = channels.some((i) => i.bot.id === ref.botId) ? channels
        : timesByComputer.get(ref.computerId)?.groups.find((g) => g.rows.some((i) => i.bot.id === ref.botId))?.rows ?? []
      const index = group.findIndex((item) => item.bot.id === ref.botId)
      const item = group[index]
      if (!item) return
      const target = channels.includes(item) ? group[index + offset]
        : nextTimesSibling(timesByComputer.get(ref.computerId), ref.botId, offset)
      if (!target || target.bot.pinned !== item.bot.pinned) return
      if (channels.includes(item)) {
        section.store.reorder(mergeOrder(section.store.roster.map((bot) => bot.id), moveInOrder(group.map((i) => i.bot.id), ref.botId, target.bot.id)))
      } else reorderTimes(item, target)
    },
  }
}

/**
 * Barra04 (SidebarHeader): a busca logo abaixo do Berga, com o filtro num ícone dentro do campo; o filtro ativo vira
 * um chip removível dentro dela. O menu do filtro tem Todas as equipes, cada equipe do topo e Agentes e, com mais
 * de um computador, os computadores (o filtro que ficava no cabeçalho, `computers`).
 */
export function RosterSearch({ roster, computers, onMenuOpenChange }: { roster: FilteredRoster; computers?: () => MenuItem[]; onMenuOpenChange?: (open: boolean) => void }) {
  const app = useApp()
  const t = useT()
  const superficie = superficieIdioma(useIdioma())
  const { query } = useModel(rosterSearch)
  const input = useRef<HTMLInputElement>(null)
  const filterButton = useRef<HTMLButtonElement>(null)
  const [menu, setMenuState] = useState(false)
  const setMenu = (open: boolean) => { setMenuState(open); onMenuOpenChange?.(open) }
  useEffect(() => {
    const current = input.current
    rosterSearch.input = current
    return () => {
      if (rosterSearch.input === current) rosterSearch.input = null
    }
  }, [])

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      if (!query) return input.current?.blur()
      e.preventDefault()
      e.stopPropagation()
      rosterSearch.set('')
    } else if ((e.key === 'ArrowDown' || e.key === 'Enter') && roster.navigable.length) {
      // Para a lista: abre o primeiro bot que bate e as setas seguem nela.
      e.preventDefault()
      app.select(roster.navigable[0]!.ref)
      document.querySelector<HTMLElement>('.roster')?.focus()
    }
  }
  const chosen = roster.teamChoices.find((s) => s.id === roster.sectionId) ?? null
  const items = (): MenuItem[] => [
    { title: t(texts.allSections), selected: !chosen, action: () => roster.setSection(ALL) },
    ...roster.teamChoices.map((s) => ({ title: s.name, selected: s.id === roster.sectionId, action: () => roster.setSection(s.id) })),
    ...(computers ? computers().map((item, i) => (i === 0 ? { ...item, divider: true } : item)) : []),
  ]
  const kbd = !query && !chosen
  return (
    <div className="roster-search idioma-superficie" {...superficie} role="search" aria-label={t(texts.region)}>
      <div className="roster-search-field">
        <Icon name="magnifyingglass" size={12} color="var(--context-ink-2, var(--ink-2))" />
        {chosen ? <span className="roster-search-chip">
          <span>{chosen.name}</span>
          <button aria-label={t('navegacao.barra.busca.tirarFiltro', { filtro: chosen.name })} title={t('navegacao.barra.busca.tirarFiltro', { filtro: chosen.name })} onClick={() => { roster.setSection(ALL); input.current?.focus() }}>
            <Icon name="xmark" size={8} weight="semibold" />
          </button>
        </span> : null}
        <input ref={input} value={query} placeholder={t(texts.search)} spellCheck={false} aria-label={t(texts.searchLabel)} aria-keyshortcuts={atalhoAria('busca', macDesteAparelho()) ?? undefined}
          onChange={(e) => rosterSearch.set(e.target.value)} onKeyDown={onKey} />
        {kbd ? <kbd className="roster-search-kbd" aria-hidden>{atalhoDoApp('busca', macDesteAparelho())}</kbd> : null}
        {query ? <button className="roster-search-clear" aria-label={t(texts.clear)} title={t(texts.clear)} onClick={() => { rosterSearch.set(''); input.current?.focus() }}>
          <Icon name="xmark.circle.fill" size={12} />
        </button> : null}
        {roster.teamChoices.length || computers ? <button ref={filterButton} className={`roster-search-filter${chosen ? ' is-on' : ''}`} aria-haspopup="menu" aria-expanded={menu}
          aria-label={chosen ? t('navegacao.barra.busca.filtro', { filtro: chosen.name }) : t(texts.filter)} title={chosen ? t('navegacao.barra.busca.filtro', { filtro: chosen.name }) : t(texts.filter)} onClick={() => setMenu(!menu)}>
          <Icon name="slider.horizontal.3" size={13} />
        </button> : null}
      </div>
      <AnchoredMenu open={menu} onClose={() => setMenu(false)} anchor={filterButton} items={items} language={superficie} />
    </div>
  )
}

// Barra04/05 (dono, 05/11/2026; docs/reference/redesenho/components/TeamGroup, times-lider-20261105/implementacao.md
// passo 6): Equipes em até 3 níveis. Cada equipe é uma linha só, a do chefe; a seta na calha da borda abre os membros
// e as subequipes, recolhidas por padrão. Um clique no chefe abre a conversa; dois cliques abrem ou fecham a equipe.
// Com o ponteiro parado (ou foco pelo teclado) no chefe, a segunda linha vira "Entrar na equipe ›", que leva à sala da equipe.
import { useRef, type KeyboardEvent, type ReactNode } from 'react'
import { BotAvatar } from '../../../components/Avatar'
import { Icon } from '../../../components/Icon'
import { Fold, warmFold } from './Fold'
import { moveTimesGroup, setTimesOpen, type TimesGroup, type TimesProjection } from '../../../lib/times-roster'
import { useModel } from '../../../lib/observable'
import type { RosterItem } from '../../../store/app-model'
import type { BotStore } from '../../../store/bot-store'
import type { RowTag } from '../../BotRow'
import { timesScreen } from '../times/navigation'
import { updatePresentation } from './prefs'
import './times-sidebar.css'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { frase, plural, textoDe, type Frase } from '../../../lib/fork-i18n/frase'
import { localeDe } from '../../../lib/fork-i18n/index'
import { ltr } from '@shared/bidi'

/** O que a lista pede à barra para desenhar uma linha de bot (a mesma `RosterRow` dos Canais e Agentes). */
export interface TeamRowOptions {
  level: number
  tag: RowTag | null
  lead: boolean
  /** A equipe da linha: a do chefe, ou a de quem é membro. */
  group: TimesGroup<RosterItem>
  /** Dois cliques: no chefe, abrem ou fecham a equipe. */
  onDoubleClick?: () => void
  /** Rótulo extra para o leitor e a dica (cargo, quantos bots a linha recolhida representa). */
  describe: string[]
}

const same = (members: RosterItem[]) => members

/** D454 (fatia 9c): o cargo do chefe pelo nível da equipe, como em `chiefTitle` (lib/times-roster), no idioma da tela. */
const CARGOS = [frase('times.quadro.barra.cargo.1'), frase('times.quadro.barra.cargo.2'), frase('times.quadro.barra.cargo.3')] as const
export const cargoDe = (level: number, equipe: string): Frase => frase('times.quadro.barra.cargoDe', { cargo: CARGOS[Math.min(Math.max(level, 1), 3) - 1]!, equipe })

const tone = (level: number) => `l${Math.min(Math.max(level, 1), 3)}` as RowTag['tone']

export function TimesSidebarList({ store, projection, compact, renderRow, onEnter, onOpen, arrange = same }: {
  store: BotStore
  projection: TimesProjection<RosterItem>
  compact: boolean
  /** The members in the order a drag in progress has them. */
  arrange?: (members: RosterItem[]) => RosterItem[]
  renderRow: (item: RosterItem, options: TeamRowOptions) => ReactNode
  /** Entrar na equipe: a lateral vira a sala dela (BotRoster). */
  onEnter: (group: TimesGroup<RosterItem>) => void
  /** A linha de referência do chefe de outra equipe abre a conversa dele. */
  onOpen: (item: RosterItem) => void
}) {
  const model = useModel(store.timesRoster)
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const organize = () => timesScreen.show(null, store.computer.id)
  const hora = model.readAt ? ltr(new Date(model.readAt).toLocaleTimeString(localeDe(idioma), { hour: '2-digit', minute: '2-digit' })) : null
  const notice = model.phase === 'loading' ? frase('times.quadro.barra.abrindo')
    : model.phase === 'owner' ? frase('times.quadro.barra.soDono')
      : model.phase === 'offline' ? (model.snapshot ? (hora ? frase('times.quadro.barra.offlineUltimaHora', { hora }) : frase('times.quadro.barra.offlineUltima')) : frase('times.quadro.barra.offlineSemLeitura'))
        : model.phase === 'error' ? frase('times.quadro.barra.erro') : null
  const roots = projection.groups.filter((g) => g.parentId === null)
  // R1-01 do #594: a seção de Equipes da barra fica fora da superfície do BotRoster; a raiz declara o idioma.
  return <div className="times-sidebar idioma-superficie" {...superficieIdioma(idioma)} role="group" aria-label={tx(frase('times.quadro.barra.rotulo'))}>
    {notice && !compact ? <div className="times-roster-notice" role="status">{tx(notice)}{model.phase === 'error' || model.phase === 'owner' ? <button onClick={() => model.retry()}>{tx(frase('times.quadro.barra.tentar'))}</button> : null}</div> : null}
    {model.snapshot && !model.snapshot.nodes.length && !compact ? <div className="times-roster-notice">{tx(frase('times.quadro.barra.nenhuma'))} <button onClick={organize}>{tx(frase('times.quadro.barra.organizar'))}</button></div> : null}
    {roots.map((group) => <Team key={group.id} group={group} store={store} projection={projection} compact={compact} renderRow={renderRow} onEnter={onEnter} onOpen={onOpen} arrange={arrange} />)}
  </div>
}

function Team({ group, store, projection, compact, renderRow, onEnter, onOpen, arrange }: {
  arrange: (members: RosterItem[]) => RosterItem[]
  group: TimesGroup<RosterItem>
  store: BotStore
  projection: TimesProjection<RosterItem>
  compact: boolean
  renderRow: (item: RosterItem, options: TeamRowOptions) => ReactNode
  onEnter: (group: TimesGroup<RosterItem>) => void
  onOpen: (item: RosterItem) => void
}) {
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  // Uma closure de gesto antigo não muda outra origem depois de reconexão/troca de Hub.
  const domain = store.timesRoster.domain, scope = store.timesRoster.scope, snapshot = store.timesRoster.snapshot
  const valid = () => store.timesRoster.domain === domain && store.timesRoster.scope === scope && store.timesRoster.snapshot === snapshot
  const lead = useRef<HTMLDivElement>(null)
  const foldKey = `${domain ?? ''}|${group.id}`
  const toggle = (open = !group.open) => {
    // Busca abre só enquanto dura: não grava nem fecha à força o que ela abriu.
    if (!valid() || (group.temporary && !open)) return
    if (open === group.open) return
    // O estado vale na hora (o corpo anima a partir de onde está): um segundo clique no meio inverte.
    updatePresentation(domain, (p) => setTimesOpen(p, group.id, open))
  }
  const peers = projection.groups.filter((g) => g.parentId === group.parentId)
  const move = (offset: number) => {
    const target = peers[peers.findIndex((g) => g.id === group.id) + offset]
    if (target && valid()) updatePresentation(domain, (p) => moveTimesGroup(projection, p, group.id, target.id))
  }
  const chief = group.chiefId ? group.items.find((i) => i.bot.id === group.chiefId) ?? null : null
  const ref = group.chiefRef
  const members = group.members
  const kids = projection.groups.filter((g) => g.parentId === group.id)
  const hidden = group.below.length
  const where = [group.path, group.name].filter(Boolean).join(' › ')
  const attention = group.below.filter((i) => i.bot.status === 'needsInput').map((i) => i.bot.name)
  const unread = group.below.reduce((sum, i) => sum + i.bot.unread, 0)
  const summary = !group.open && hidden ? [
    tx(plural('times.quadro.barra.maisBots', hidden)),
    attention.length ? tx(plural('times.quadro.barra.precisam', attention.length, { nomes: attention.join(', ') })) : '',
    unread ? tx(plural('times.quadro.barra.naoLidas', unread)) : '',
  ].filter(Boolean) : []
  const toggleLabel = tx(plural(group.open ? 'times.quadro.barra.esconder' : 'times.quadro.barra.mostrar', hidden, { equipe: group.name }))
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (!(e.target instanceof HTMLElement) || !e.target.matches('[data-roster-row], [data-times-header]')) return
    if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) { e.preventDefault(); e.stopPropagation(); move(e.key === 'ArrowUp' ? -1 : 1) }
    else if (e.key === 'Enter' && e.shiftKey) { e.preventDefault(); e.stopPropagation(); onEnter(group) }
    else if (e.key === 'ArrowRight') {
      e.preventDefault(); e.stopPropagation()
      if (!group.open) toggle(true)
      else lead.current?.nextElementSibling?.querySelector<HTMLElement>('[data-roster-row], [data-times-header]')?.focus()
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault(); e.stopPropagation()
      if (group.open && !group.temporary) toggle(false)
      else parentLead(lead.current)?.focus()
    }
  }
  const enter = <button className="times-enter" tabIndex={-1} aria-label={tx(frase('times.quadro.barra.entrarRotulo', { equipe: group.name }))} title={tx(frase('times.quadro.barra.entrarDica', { equipe: group.name }))} onClick={() => onEnter(group)}>
    {tx(frase('times.quadro.barra.entrar'))}<Icon name="chevron.right" size={9} weight="semibold" className="espelhar-rtl" />
  </button>
  return <div className={`times-team${group.temporary ? ' is-search-open' : ''}`} data-level={Math.min(group.level, 3)}>
    {/* O ponteiro ou o foco no título montam já o corpo recolhido (Fold), para abrir sem esperar. */}
    <div ref={lead} className={`times-lead${chief ? ' has-chief' : ' no-chief'}`} onKeyDown={onKey}
      onPointerEnter={() => warmFold(foldKey)} onFocus={() => warmFold(foldKey)}>
      {/* Aberta só pela busca, a seta não teria o que fazer (não fecha nem grava): fica de fora. */}
      {!compact && hidden && !group.temporary ? <button className="times-exp" tabIndex={-1} aria-expanded={group.open} aria-label={toggleLabel} title={toggleLabel} onClick={() => toggle()}>
        <Icon name="chevron.down" size={9} weight="semibold" />
      </button> : null}
      {chief ? renderRow(chief, { level: group.level, tag: { text: group.name, tone: tone(group.level) }, lead: true, group, onDoubleClick: () => toggle(), describe: [tx(cargoDe(group.level, where)), ...summary] })
        : ref ? <ChiefRefRow group={group} compact={compact} expanded={hidden ? group.open : undefined} onOpen={onOpen} onDoubleClick={() => toggle()} />
        : <button className="times-teamrow" data-times-header data-times-group={group.id} aria-expanded={group.open}
          aria-label={tx(plural('times.quadro.barra.semChefeAria', hidden, { equipe: where }))} title={tx(frase('times.quadro.barra.semChefeDica', { equipe: where }))} onClick={() => toggle()}>
          <span className={`times-team-tile tag-${tone(group.level)}`} aria-hidden><Icon name="person.2" size={13} weight="medium" /></span>
          {compact ? null : <span className={`bot-row-tag tag-${tone(group.level)} is-team`} dir="auto">{group.name}</span>}
        </button>}
      {compact ? null : enter}
    </div>
    {hidden || (!chief && !ref) ? <Members foldKey={foldKey} open={group.open}>
      {!chief && !ref && !projection.filtering && !compact ? <p className="times-chief-hint">{tx(frase(group.checkChief ? 'times.quadro.barra.confiraChefe' : 'times.quadro.barra.semChefe'))} <button onClick={() => { if (valid()) timesScreen.show(null, store.computer.id) }}>{tx(frase('times.quadro.barra.organizar'))}</button></p> : null}
      {arrange(members).map((item) => renderRow(item, { level: group.level + 1, tag: item.bot.title ? { text: item.bot.title, tone: tone(group.level + 1) } : null, lead: false, group, describe: [] }))}
      {kids.map((kid) => <Team key={kid.id} group={kid} store={store} projection={projection} compact={compact} renderRow={renderRow} onEnter={onEnter} onOpen={onOpen} arrange={arrange} />)}
    </Members> : null}
  </div>
}

/** Chefe gravado que é membro de outra equipe: a linha de referência (barra e sala). Abre a conversa dele; não é membro. */
export function ChiefRefRow({ group, compact, expanded, onOpen, onDoubleClick }: {
  group: TimesGroup<RosterItem>
  compact: boolean
  expanded?: boolean
  onOpen: (item: RosterItem) => void
  onDoubleClick?: () => void
}) {
  const idioma = useIdioma()
  const ref = group.chiefRef!
  const where = [group.path, group.name].filter(Boolean).join(' › ')
  const cargo = cargoDe(group.level, where)
  return <button className="times-teamrow times-ref" data-times-header data-times-group={group.id} aria-expanded={expanded}
    aria-label={textoDe(frase('times.quadro.barra.refAria', { nome: ref.item.bot.name, cargo, casa: ref.home }), idioma)}
    title={textoDe(frase('times.quadro.barra.refDica', { nome: ref.item.bot.name, cargo, casa: ref.home }), idioma)}
    onClick={() => onOpen(ref.item)} onDoubleClick={onDoubleClick}>
    <span className="times-team-tile times-ref-face" aria-hidden><BotAvatar bot={ref.item.bot} size={24} /></span>
    {compact ? null : <span className="times-ref-text">
      <span className="times-ref-line"><span className="times-ref-name" dir="auto">{ref.item.bot.name}</span><span className={`bot-row-tag tag-${tone(group.level)} is-team`} dir="auto">{group.name}</span></span>
      <span className="times-ref-home">{textoDe(frase('times.quadro.barra.refCasa', { casa: ref.home }), idioma)}</span>
    </span>}
  </button>
}

/** Membros e subequipes, com a ramificação; cresce ao abrir e fica montado ao recolher (Fold). ← num membro volta ao chefe. */
function Members({ foldKey, open, children }: { foldKey: string; open: boolean; children: ReactNode }) {
  return <div className="times-members-keys" onKeyDown={(e) => {
    // Só as linhas de membro desta equipe; as subequipes cuidam das próprias teclas.
    const body = e.currentTarget.firstElementChild
    if (e.key !== 'ArrowLeft' || e.altKey || !(e.target instanceof HTMLElement) || e.target.parentElement !== body) return
    e.preventDefault(); e.stopPropagation()
    e.currentTarget.previousElementSibling?.querySelector<HTMLElement>('[data-roster-row], [data-times-header]')?.focus()
  }}><Fold open={open} foldKey={foldKey} className="times-members" role="group">{children}</Fold></div>
}

/** A linha da equipe-mãe de uma linha de equipe. */
function parentLead(lead: HTMLElement | null): HTMLElement | null {
  const members = lead?.parentElement?.parentElement
  if (!members?.classList.contains('times-members')) return null
  return members.parentElement?.previousElementSibling?.querySelector<HTMLElement>('[data-roster-row], [data-times-header]') ?? null
}

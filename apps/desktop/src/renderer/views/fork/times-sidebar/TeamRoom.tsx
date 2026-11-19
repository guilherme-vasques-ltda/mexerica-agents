// Barra04 (dono, 05/11/2026; implementacao.md passo 11): a sala da equipe. A lateral mostra só a equipe: a barra
// "‹ Equipes" (ou "‹ equipe-mãe") com a etiqueta dela, o chefe e os membros em linhas grandes (as de Agentes, com o
// cargo ou a função na etiqueta) e cada subequipe como separador com o caminho relativo, que entra nela.
// Esc, ← numa linha e o botão sobem um nível. A busca procura dentro da sala. O estado fica só em memória.
import { useEffect, useRef, type ReactNode } from 'react'
import { Icon } from '../../../components/Icon'
import { useEscape } from '../../../components/Overlay'
import type { TimesGroup, TimesProjection } from '../../../lib/times-roster'
import type { RosterItem } from '../../../store/app-model'
import type { RowTag } from '../../BotRow'
import { cargoDe, ChiefRefRow } from './TimesSidebarList'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { frase, textoDe, type Frase } from '../../../lib/fork-i18n/frase'

/**
 * Teclas da sala: ← numa linha sobe um nível. ⌥↑/↓ na referência do chefe de outra equipe não reordena nada (revisão
 * #379 R2-1): sem isso, o atalho da lista (ChatWindow) moveria o bot selecionado.
 */
export function roomKey(e: { key: string; altKey: boolean; target: unknown; preventDefault: () => void; stopPropagation: () => void }, up: () => void) {
  if (!(e.target instanceof HTMLElement)) return
  if (e.key === 'ArrowLeft' && !e.altKey && e.target.matches('[data-roster-row]')) { e.preventDefault(); e.stopPropagation(); up() }
  else if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown') && e.target.closest('.times-ref')) { e.preventDefault(); e.stopPropagation() }
}

const tone = (level: number) => `l${Math.min(Math.max(level, 1), 3)}` as RowTag['tone']

export function TeamRoom({ projection, teamId, filtering, query, renderRow, onGo, onOrganize, onOpen }: {
  /** A projeção com todas as equipes abertas (`FilteredRoster.roomFor`). */
  projection: TimesProjection<RosterItem>
  teamId: string
  filtering: boolean
  query: string
  renderRow: (item: RosterItem, tag: RowTag | null, chief: boolean) => ReactNode
  /** Entra noutra equipe (subequipe ou a mãe); null volta para a lista. */
  onGo: (teamId: string | null) => void
  onOrganize: () => void
  /** A linha de referência do chefe de outra equipe abre a conversa dele. */
  onOpen: (item: RosterItem) => void
}) {
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const root = projection.groups.find((g) => g.id === teamId)
  const parent = root?.parentId ? projection.groups.find((g) => g.id === root.parentId) ?? null : null
  const back = useRef<HTMLButtonElement>(null)
  const up = () => onGo(root?.parentId ?? null)
  useEscape(!!root, up, back)
  // Entrar (ou subir) leva o foco à barra da sala, como no protótipo; as setas seguem dali para as linhas.
  useEffect(() => { back.current?.focus({ preventScroll: true }) }, [teamId])
  // A equipe sumiu (outro Hub, quadro mudou): a sala fecha sozinha.
  useEffect(() => { if (!root && !filtering) onGo(null) }, [root, filtering, onGo])
  if (!root) return null
  let any = false
  const block = (t: TimesGroup<RosterItem>, path: string): ReactNode => {
    const chief = t.chiefId ? t.rows.find((i) => i.bot.id === t.chiefId) ?? null : null
    const members = t.rows.filter((i) => i !== chief)
    if (chief || members.length) any = true
    const kids = projection.groups.filter((g) => g.parentId === t.id)
    const inner = path ? `${path} › ${t.name}` : t.name
    return <div key={t.id} className="room-block">
      {t !== root ? <div className="room-sub">
        <button className="room-sub-name" title={tx(frase('times.quadro.barra.sala.entrarEm', { equipe: t.name }))} onClick={() => onGo(t.id)}>
          {path ? <bdi className="room-sub-path" dir="auto">{path} › </bdi> : null}<bdi dir="auto">{t.name}</bdi>
          <Icon name="chevron.right" size={9} weight="semibold" className="room-sub-go" />
        </button>
      </div> : null}
      {chief ? renderRow(chief, { text: tx(cargoDe(t.level, t.name)), tone: 'l0' }, t === root) : t.chiefRef ? <ChiefRefRow group={t} compact={false} onOpen={onOpen} /> : null}
      {members.map((item) => renderRow(item, item.bot.title ? { text: item.bot.title, tone: 'l0' } : null, false))}
      {kids.map((kid) => block(kid, t === root ? '' : inner))}
    </div>
  }
  const body = block(root, '')
  // R1-01 do #594: a sala é montada fora das seções do BotRoster; a raiz declara o idioma.
  return <div className="team-room idioma-superficie" {...superficieIdioma(idioma)} onKeyDown={(e) => roomKey(e, up)}>
    <div className="room-bar">
      <button ref={back} className="room-back" aria-label={tx(parent ? frase('times.quadro.barra.sala.voltarPara', { equipe: parent.name }) : frase('times.quadro.barra.sala.voltarEquipes'))} onClick={up}>
        <Icon name="chevron.left" size={11} weight="semibold" className="espelhar-rtl" /><bdi dir="auto">{parent ? parent.name : tx(frase('times.quadro.barra.rotulo'))}</bdi>
      </button>
      <span className={`room-tag tag-${tone(root.level)}`} dir="auto">{root.name}</span>
    </div>
    {!root.chiefId && !root.chiefRef && !filtering ? <p className="times-chief-hint">{tx(frase('times.quadro.barra.semChefe'))} <button onClick={onOrganize}>{tx(frase('times.quadro.barra.organizar'))}</button></p> : null}
    {body}
    {query && !any ? <p className="sidebar-section-empty">{tx(frase('times.quadro.barra.sala.nadaEncontrado', { equipe: root.name, busca: query }))}</p> : null}
  </div>
}

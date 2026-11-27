import { spaceAllows } from '../../lib/fork-space-capabilities'
import { useRef, useState } from 'react'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import type { Space } from '@shared/fork-spaces'
import { Icon } from '../../components/Icon'
import { AnchoredMenu, type MenuItem } from '../../components/Overlay'
import { useModel } from '../../lib/observable'
import { UpdateBadge } from '../UpdateButton'
import { routinesScreen } from '../rotinas-central/navigation'
import { isOpenDesktop } from '@shared/fork-capacidades'
import { timesScreen } from '../fork/times/navigation'
import { torreScreen } from '../fork/torre/navigation'
import { TorreRailButton } from '../fork/torre/RailButton'
import type { BotStore } from '../../store/bot-store'
import { settingsScreen } from '../fork/ajustes/tela'
import { extensoesScreen } from '../fork/extensoes/tela'
import './space-rail.css'

// D165, fase B (docs/reference/redesenho/components/SpaceRail): the 60px rail at the window's left.
// Pessoal first and fixed, then, at the foot, Rotinas, Aplicativos and the gear with Pendências, Uso, Skills,
// Conectores, Conta e suporte and Ajustes.
// D169: the sidebar button left the rail for the conversation's top left (components/AppShell). The Hub's
// spaces follow Pessoal (`spaces`, from store/fork-spaces): a click opens the space (components/SpaceView), its
// context menu has Editar and Mover, + creates one, and ⌘1 (Pessoal) to ⌘9 open them in order. With a space open,
// the bar is on its tile; Pessoal always goes (back) to Pessoal. The account menu is "Conta e suporte" in the gear's menu.

/** Barra05: os ícones do rodapé da faixa (Mission Board, Rotinas, Aplicativos, engrenagem). */
export const RAIL_ICON = 16

export interface RailActions {
  /** The account menu (Uso, Suporte, Ajustes, the account when the cloud is on): "Conta e suporte" in the gear's menu. */
  onAccount: () => void
  accountOpen: boolean
  onSkills: () => void
  onConnectors: () => void
  onUsage: () => void
  onSettings: () => void
  /** Devices waiting for approval on this computer's Hub (`app.approvals`). */
  approvals: number
  onApprovals: () => void
  /** Marketplace needs a connected Hub. */
  extensionsAvailable: boolean
  /** D169: the rail Hub's spaces after Pessoal and what the rail can ask of them; absent, none show. */
  spaces?: RailSpaces | null
  /** VG-02: o Hub da faixa, para o ponto do Mission Board (views/fork/torre/badge.ts); ausente, sem ponto. */
  torreStore?: BotStore | null
  /** Quadro Lado a lado: alterna o quadro no escopo atual (Pessoal ou Space). */
  ladoALado?: { active: boolean; available: boolean; onToggle: () => void } | null
}

export interface RailSpaces {
  items: Space[]
  /** The open space, null while Pessoal is current. */
  activeId: string | null
  /** The computer whose Hub these spaces are (Times opens on it, never on another Hub). */
  computerId?: string
  onOpen: (id: string) => void
  onPersonal: () => void
  /** Whether the window's shell has the keyboard now (no sheet, dialog, menu or palette in front): ⌘1…⌘9 wait otherwise. */
  keys: () => boolean
  onCreate: () => void
  onEdit: (id: string) => void
  onMove: (id: string, delta: -1 | 1) => void
}

export function SpaceRail(props: RailActions) {
  // One place is current at a time: Rotinas while its screen is open, Pessoal otherwise.
  // Times (components/TimesView) é um lugar como Rotinas: com ele aberto, o Pessoal não é o lugar atual.
  const timesOpen = useModel(timesScreen).open
  // VG-02 (98A): o Mission Board é um lugar próprio, como Rotinas.
  const torreOpen = useModel(torreScreen).open
  const routinesOpen = useModel(routinesScreen).open || timesOpen || torreOpen
  // ajustes-tela-01: Ajustes is a place too (its screen), like Rotinas: neither Pessoal nor a space is current then.
  // D249 E3: Extensões is a place too; Aplicativos is current on its page, the gear on Conectores and Skills.
  const extensions = useModel(extensoesScreen)
  const settingsOpen = useModel(settingsScreen).open
  const [gearOpen, setGearOpen] = useState(false)
  const t = useT()
  const superficie = superficieIdioma(useIdioma())
  const gear = useRef<HTMLButtonElement>(null)
  const spaces = props.spaces ?? null
  const active = spaces?.activeId ?? null
  // Num Space com pessoas, o que o Space não tem some do menu (nada de item cinza); no Pessoal tudo existe.
  const gearItems = (): MenuItem[] => [
    ...(props.approvals > 0 ? [{ title: t('navegacao.barra.faixa.pendencias'), icon: 'bell.badge', detail: String(props.approvals), action: props.onApprovals }] : []),
    { title: t('navegacao.janela.conta.uso'), icon: 'gauge.with.dots.needle.33percent', action: props.onUsage },
    // Times abre no Hub do contexto: o do space aberto; no Pessoal, o do bot selecionado (TimesView decide).
    ...(!isOpenDesktop(window.codync) && spaceAllows(active, 'teams') ? [{ title: t('navegacao.barra.secao.equipes'), icon: 'person.2', action: () => { routinesScreen.close(false); settingsScreen.close(false); timesScreen.show(active, active ? spaces?.computerId ?? null : null, spaces?.computerId ?? null) } }] : []),
    ...(!isOpenDesktop(window.codync) && active && spaceAllows(active, 'routines') ? [{ title: t('navegacao.barra.faixa.rotinas'), icon: 'calendar', action: () => routinesScreen.show() }] : []),
    ...(active && spaceAllows(active, 'projects') ? [{ title: t('projetos.entrada'), icon: 'folder', action: () => torreScreen.show(null, spaces?.computerId ?? null) }] : []),
    ...(props.extensionsAvailable ? [
      ...(!isOpenDesktop(window.codync) && spaceAllows(active, 'skills') ? [{ title: t('navegacao.barra.faixa.skills'), icon: 'lightbulb', action: props.onSkills }] : []),
      ...(spaceAllows(active, 'connectors') ? [{ title: t('navegacao.barra.faixa.conectores'), icon: 'puzzlepiece.extension', action: props.onConnectors }] : []),
    ] : []),
    // D169 (28/10, pedido do dono): the account menu's own place, now that Pessoal on the rail only navigates.
    { title: t('navegacao.barra.faixa.conta'), icon: 'person.crop.circle', divider: true, action: props.onAccount },
    { title: t('navegacao.janela.conta.ajustes'), icon: 'gearshape', action: props.onSettings },
  ]
  return (
    <nav className="space-rail glass-panel idioma-superficie" {...superficie} aria-label={t('navegacao.barra.faixa.rotulo')}>
      <div className="space-rail-lights drag" aria-hidden />
      {/* D169 (28/10, pedido do dono): Pessoal is a place, like the spaces under it: it always goes (back) to Pessoal,
          from a space or Rotinas, with its draft, selection and focus kept. The account has "Conta e suporte". */}
      <div className="space-rail-gap" />
      {props.ladoALado ? (
        <button
          className={`space-rail-button${props.ladoALado.active ? ' is-active' : ''}`}
          title={t(props.ladoALado.active ? 'conversa.ladoALado.sair' : 'conversa.ladoALado.entrar')}
          aria-label={t(props.ladoALado.active ? 'conversa.ladoALado.sair' : 'conversa.ladoALado.entrar')}
          aria-pressed={props.ladoALado.active}
          disabled={!props.ladoALado.available}
          onClick={props.ladoALado.onToggle}
        >
          <Icon name="donna.ladoalado" size={RAIL_ICON} weight="light" />
        </button>
      ) : null}
      {/* Barra05: o rodapé, depois de um separador de 18px: Mission Board, Rotinas, Aplicativos e a engrenagem, em alvos
          de 32×28 com ícones de 16px no mesmo peso (`light`), SF Symbols no Mac e Lucide no resto (components/Icon). */}
      <span className="space-rail-sep" aria-hidden />
      {/* VG-02 (98A): Mission Board, acima de Rotinas, no Hub e space da faixa. `torre-open` recebe o foco ao fechar. */}
      {spaceAllows(active, 'projects')
        ? <TorreRailButton store={props.torreStore ?? null} spaceId={active} active={torreOpen} onOpen={() => torreScreen.show(active, spaces?.computerId ?? null)} /> : null}
      {/* `rc-open`: closing Rotinas brings focus back here (rotinas-central/navigation.ts). */}
      {!isOpenDesktop(window.codync) && spaceAllows(active, 'routines') ? <button className="space-rail-button rc-open" title={t('navegacao.barra.faixa.rotinas')} aria-label={t('navegacao.barra.faixa.rotinas')} aria-current={routinesOpen && !timesOpen && !torreOpen ? 'page' : undefined} onClick={() => { timesScreen.close(false); settingsScreen.close(false); routinesScreen.show() }}>
        <Icon name="calendar" size={RAIL_ICON} weight="light" />
      </button> : null}
      <button className="space-rail-button" title={t('navegacao.barra.faixa.conectores')} aria-label={t('navegacao.barra.faixa.conectores')} aria-current={extensions.open && extensions.page === 'connectors' ? 'page' : undefined} disabled={!props.extensionsAvailable} onClick={props.onConnectors}>
        <Icon name="square.grid.2x2" size={RAIL_ICON} weight="light" />
      </button>
      <button
        ref={gear}
        className={`space-rail-button times-open${gearOpen || props.accountOpen || timesOpen || settingsOpen || (extensions.open && extensions.page !== 'connectors') ? ' is-active' : ''}`}
        aria-current={timesOpen ? 'page' : undefined}
        data-ajustes-volta
        title={props.approvals > 0 ? t.plural('navegacao.barra.faixa.engrenagemPendencias', props.approvals) : t('navegacao.barra.faixa.engrenagem')}
        aria-label={props.approvals > 0 ? t.plural('navegacao.barra.faixa.engrenagemPendenciasAria', props.approvals) : t('navegacao.barra.faixa.engrenagem')}
        aria-haspopup="menu"
        aria-expanded={gearOpen}
        onClick={() => setGearOpen((o) => !o)}
      >
        <Icon name="gearshape" size={RAIL_ICON} weight="light" />
        {props.approvals > 0 ? <span className="space-rail-dot" aria-hidden /> : null}
      </button>
      <AnchoredMenu open={gearOpen} onClose={() => setGearOpen(false)} anchor={gear} items={gearItems} language={superficie} />
      <UpdateBadge size={32} />
    </nav>
  )
}

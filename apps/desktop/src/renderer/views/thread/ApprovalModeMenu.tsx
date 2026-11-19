import { useEffect, useRef, type CSSProperties, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { Icon } from '../../components/Icon'
import { anchorPlacement, useEscape } from '../../components/Overlay'
import { superficieIdioma, useIdioma, tradutor } from '../../lib/fork-i18n/estado'
import './approval-mode-menu.css'

// D169 (components/ApprovalModeMenu): the shield's menu, as in the reference, over the Hub's real contract.
// Only the legacy level applies today (`ask`, `auto`); the new levels of `donnaRules` are prepared_only and
// the composer isn't one of their surfaces. So nothing here changes a level: the other legacy level and the
// last item open the bot's settings on Comportamento, where the change goes through that screen's checks.

type LevelId = 'ask' | 'auto' | 'auto_approve' | 'full_access'

interface Level {
  id: LevelId
  icon: string
  soon?: boolean
}

// Título e descrição em conversa.aprovacao.nivel.<id>.* (catálogo).
const LEVELS: Level[] = [
  { id: 'ask', icon: 'hand.raised' },
  { id: 'auto', icon: 'checkmark.shield' },
  { id: 'auto_approve', icon: 'shield', soon: true },
  { id: 'full_access', icon: 'exclamationmark.triangle', soon: true },
]

/** The level the shield shows: `auto` is the legacy "Liberar", never the new "Auto aprovar". */
export const shieldLevel = (permission: string) => LEVELS.find((l) => l.id === (permission === 'auto' ? 'auto' : 'ask'))!

export function ApprovalModeMenu({ open, anchor, botName, permission, onSettings, onClose, place }: {
  open: boolean
  anchor: RefObject<HTMLElement | null>
  botName: string
  permission: string
  /** Opens the bot's settings on Comportamento. */
  onSettings: () => void
  onClose: () => void
  /** Fork (D473): a posição já medida e presa ao escudo (null = ainda sem medida); sem isto, ao lado do escudo como antes. */
  place?: CSSProperties | null
}) {
  const panel = useRef<HTMLDivElement>(null)
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const current = shieldLevel(permission).id
  useEscape(open, onClose, panel)
  // Focus starts on the current level; arrows move between enabled items.
  useEffect(() => {
    if (!open) return
    requestAnimationFrame(() => panel.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus())
  }, [open])
  if (!open || !anchor.current || place === null) return null
  const style = place ?? anchorPlacement(anchor.current.getBoundingClientRect()).style
  const items = () => [...(panel.current?.querySelectorAll<HTMLElement>('.approval-mode-item:not([aria-disabled="true"])') ?? [])]
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Tab') { onClose(); return }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
    e.preventDefault()
    const list = items(), at = list.indexOf(document.activeElement as HTMLElement)
    list[(at + (e.key === 'ArrowDown' ? 1 : -1) + list.length) % list.length]?.focus()
  }
  const settings = () => { onClose(); onSettings() }
  return createPortal(
    <div className="anchored shown" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={panel} className="anchored-panel approval-mode-menu idioma-superficie" {...superficieIdioma(idioma)} style={style} role="menu" aria-label={t('conversa.aprovacao.menu.aria', { bot: botName })} onKeyDown={onKey}>
        <p className="approval-mode-title">{t('conversa.aprovacao.menu.titulo', { bot: botName })}</p>
        {LEVELS.map((level) => {
          const checked = level.id === current
          const title = t(`conversa.aprovacao.nivel.${level.id}.titulo`)
          const label = t(checked ? 'conversa.aprovacao.menu.atual' : level.soon ? 'conversa.aprovacao.menu.emBreveAria' : 'conversa.aprovacao.menu.mudar', { nivel: title })
          return (
            <button
              key={level.id}
              className="approval-mode-item"
              role={checked || level.soon ? 'menuitemradio' : 'menuitem'}
              aria-checked={checked || level.soon ? checked : undefined}
              aria-disabled={level.soon || undefined}
              aria-label={label}
              tabIndex={-1}
              onClick={() => { if (level.soon) return; if (checked) onClose(); else settings() }}
            >
              <span className="approval-mode-icon"><Icon name={level.icon} size={16} /></span>
              <span className="approval-mode-text">
                <b>{title}{level.soon ? <span className="approval-mode-tag">{t('conversa.aprovacao.menu.emBreve')}</span> : null}</b>
                <small>{t(`conversa.aprovacao.nivel.${level.id}.detalhe`)}</small>
              </span>
              <span className="approval-mode-trail">
                {checked ? <Icon name="checkmark" size={14} weight="semibold" /> : level.soon ? null : <Icon name="chevron.right" size={12} />}
              </span>
            </button>
          )
        })}
        <div className="approval-mode-sep" role="separator" />
        <button className="approval-mode-item" role="menuitem" tabIndex={-1} onClick={settings}>
          <span className="approval-mode-icon"><Icon name="gearshape" size={16} /></span>
          <span className="approval-mode-text"><b>{t('conversa.aprovacao.menu.configuracoes')}</b></span>
          <span className="approval-mode-trail"><Icon name="chevron.right" size={12} /></span>
        </button>
      </div>
    </div>,
    document.body,
  )
}

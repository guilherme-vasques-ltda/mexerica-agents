import { useRef, useState, type KeyboardEvent } from 'react'
import { CardForm, CardSection, Switch } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Text } from '../../components/Text'
import { pointerDrag } from '../../lib/pointer-drag'
import { prefs, usePref } from '../../lib/prefs'
import { expandLegacySections, moveSection, sectionOrder, SIDEBAR_SECTIONS, type SidebarSection } from '../../lib/sidebar-layout'
import { appearancePrefs } from '../../lib/appearance'
import { readTagColors, TAG_COLORS, TAG_SLOTS } from '../../lib/appearance-choices'
import './sidebar-settings.css'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import { isMac } from './parts'
import { semParametros } from './rotulo'

// D165 fase C (components/SidebarSettings): Ajustes → Barra lateral. The sections' order (drag by the
// handle, or focus + ⌥↑/⌥↓) and which show; a hidden section leaves the sidebar without losing anything.
// Barra04 (dono, 05/11/2026): Canais, Equipes e Agentes, e as Cores das etiquetas por nível (padrão Grafite).

export function SidebarSettings() {
  const t = useT()
  const tx = semParametros(t)
  const idioma = useIdioma()
  const [saved, setOrder] = usePref(prefs.sidebarSectionOrder)
  const [hiddenSaved, setHidden] = usePref(prefs.sidebarHiddenSections)
  // Quem tinha Bots escondida esconde Equipes e Agentes.
  const hidden = expandLegacySections(hiddenSaved)
  const [tagSaved, setTags] = usePref(appearancePrefs.sidebarTagColors)
  const tags = readTagColors(tagSaved)
  const [dragging, setDragging] = useState<SidebarSection | null>(null)
  const order = sectionOrder(saved)
  const dragged = useRef<SidebarSection | null>(null)
  const label = (id: SidebarSection) => tx(SIDEBAR_SECTIONS.find((s) => s.id === id)!.labelKey)
  const move = (id: SidebarSection, offset: number) => setOrder(moveSection(order, id, offset))
  // The row keeps its node (keyed rows in CardSection); if moving it took focus away, it comes back to
  // the same control, so the next ⌥↑/⌥↓ keeps working (D165 revisão C, C5).
  const keepFocus = (id: SidebarSection, control: 'handle' | 'up' | 'down') => requestAnimationFrame(() => {
    const row = document.querySelector<HTMLElement>(`[data-sidebar-section="${id}"]`)
    const target = row?.querySelector<HTMLButtonElement>(`[data-order-control="${control}"]:not(:disabled)`) ?? row?.querySelector<HTMLButtonElement>('[data-order-control="handle"]')
    if (target && document.activeElement !== target) target.focus()
  })
  const onKey = (id: SidebarSection) => (e: KeyboardEvent) => {
    if (!e.altKey || (e.key !== 'ArrowUp' && e.key !== 'ArrowDown')) return
    e.preventDefault()
    move(id, e.key === 'ArrowUp' ? -1 : 1)
    keepFocus(id, 'handle')
  }

  return (
    <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
    <CardForm>
      <CardSection title={t('ajustes.barra.secoes')} footer={isMac ? t('ajustes.barra.secoesRodapeMac') : t('ajustes.barra.secoesRodape')}>
        {order.map((id, index) => (
          <div key={id} className={`sidebar-order-row${dragging === id ? ' dragging' : ''}`} data-sidebar-section={id}>
            <button
              className="sidebar-order-handle"
              data-order-control="handle"
              aria-label={t('ajustes.barra.mover', { secao: label(id) })}
              title={t('ajustes.barra.arrastar')}
              onKeyDown={onKey(id)}
              {...pointerDrag({
                start: () => { dragged.current = id; setDragging(id) },
                over: (_at, under) => {
                  const target = under?.closest<HTMLElement>('[data-sidebar-section]')?.dataset.sidebarSection as SidebarSection | undefined
                  const current = dragged.current
                  if (!current || !target || target === current) return
                  const from = order.indexOf(current)
                  const to = order.indexOf(target)
                  if (from >= 0 && to >= 0) setOrder(moveSection(order, current, to - from))
                },
                drop: () => { dragged.current = null; setDragging(null) },
                cancel: () => { dragged.current = null; setDragging(null) },
              })}
            >
              <Icon name="line.3.horizontal" size={12} />
            </button>
            <Text grow className={hidden.includes(id) ? 'sidebar-order-hidden' : undefined}>{label(id)}</Text>
            <button className="sidebar-order-move" data-order-control="up" disabled={index === 0} aria-label={t('ajustes.barra.subirSecao', { secao: label(id) })} title={t('ajustes.barra.subir')} onClick={() => { move(id, -1); keepFocus(id, 'up') }}>
              <Icon name="chevron.up" size={10} weight="semibold" />
            </button>
            <button className="sidebar-order-move" data-order-control="down" disabled={index === order.length - 1} aria-label={t('ajustes.barra.descerSecao', { secao: label(id) })} title={t('ajustes.barra.descer')} onClick={() => { move(id, 1); keepFocus(id, 'down') }}>
              <Icon name="chevron.down" size={10} weight="semibold" />
            </button>
            <Switch on={!hidden.includes(id)} label={t('ajustes.barra.mostrar', { secao: label(id) })} className="sidebar-order-switch"
              onChange={(on) => setHidden(on ? hidden.filter((h) => h !== id) : [...hidden, id])} />
          </div>
        ))}
      </CardSection>
      <CardSection title={t('ajustes.barra.cores')} footer={t('ajustes.barra.coresRodape')}>
        {TAG_SLOTS.map((slot) => (
          <div key={slot.id} className="sidebar-tag-row">
            <div className="sidebar-tag-text">
              <Text id={`sidebar-tag-${slot.id}`}>{t(slot.nameKey)}</Text>
              <Text size="caption" tone="secondary">{t(slot.detailKey)}</Text>
            </div>
            <div className="sidebar-tag-swatches" role="radiogroup" aria-labelledby={`sidebar-tag-${slot.id}`}>
              {TAG_COLORS.map((color) => (
                <button key={color.id} role="radio" aria-checked={tags[slot.id] === color.id} className={`sidebar-tag-swatch tag-${color.id}`}
                  onClick={() => setTags({ ...tags, [slot.id]: color.id })}>{t(color.nameKey)}</button>
              ))}
            </div>
          </div>
        ))}
      </CardSection>
    </CardForm>
    </div>
  )
}

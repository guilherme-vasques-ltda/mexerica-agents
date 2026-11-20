// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { useEffect, useMemo, useRef, useState } from 'react'
import { SearchField } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Text } from '../../components/Text'
import { AnchoredPanel } from '../bots/parts'
import { currentZone, matchZones, timeZoneOptions, zoneLabel, zoneOffset } from './time-zones'
import './routines.css'
import { superficieIdioma, tradutor, useIdioma } from '../../lib/fork-i18n/estado'

/** The routine's time zone: a button that opens every zone, filtered as you type. */
export function TimeZonePicker({ zone: saved, onChange }: { zone: string; onChange: (zone: string) => void }) {
  const zone = currentZone(saved)
  const anchor = useRef<HTMLButtonElement>(null)
  const [open, setOpen] = useState(false)
  const idioma = useIdioma()
  const t = tradutor(idioma)
  return (
    <>
      <button
        ref={anchor}
        className="schedule-input zone-button"
        aria-label={t('rotinas.editor.fusoEm', { fuso: zone })}
        aria-haspopup="listbox"
        aria-expanded={open}
        onClick={() => setOpen(true)}
      >
        {/* IDs de fuso e GMT são dados técnicos: LTR e isolados do texto ao redor. */}
        <Text truncate grow dir="ltr">{zoneLabel(zone)}</Text>
        <Text tone="secondary" dir="ltr">{zoneOffset(zone)}</Text>
        <Icon name="chevron.up.chevron.down" size={10} weight="semibold" color="var(--context-ink-2, var(--ink-2))" />
      </button>
      <AnchoredPanel open={open} onClose={() => setOpen(false)} anchor={anchor}>
        <ZoneList
          zone={zone}
          pick={(picked) => {
            setOpen(false)
            onChange(picked)
          }}
        />
      </AnchoredPanel>
    </>
  )
}

/** The search box over the matching zones; ↑/↓ move, Return picks. */
function ZoneList({ zone, pick }: { zone: string; pick: (zone: string) => void }) {
  const zones = useMemo(() => timeZoneOptions(), [])
  const [query, setQuery] = useState('')
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const matches = useMemo(() => matchZones(zones, query), [zones, query])
  const [highlight, setHighlight] = useState(() => Math.max(0, zones.findIndex((z) => z.id === zone)))
  const input = useRef<HTMLInputElement>(null)
  const list = useRef<HTMLDivElement>(null)

  useEffect(() => input.current?.focus(), [])
  useEffect(() => {
    list.current?.querySelector(`[data-index="${highlight}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [highlight, matches])

  return (
    // Portal no body (AnchoredPanel): declara o idioma da tela (DON-917).
    <div
      {...superficieIdioma(idioma)}
      className="zone-picker"
      onKeyDown={(e) => {
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
          e.preventDefault()
          setHighlight((h) => Math.min(Math.max(h + (e.key === 'ArrowDown' ? 1 : -1), 0), matches.length - 1))
        } else if (e.key === 'Enter' && matches[highlight]) {
          e.preventDefault()
          pick(matches[highlight].id)
        }
      }}
    >
      <SearchField
        value={query}
        prompt={t('rotinas.editor.buscarFuso')}
        inputRef={input}
        onChange={(q) => {
          setQuery(q)
          setHighlight(0)
        }}
      />
      <div ref={list} className="zone-list" role="listbox" aria-label={t('rotinas.editor.fusos')}>
        {matches.map((z, i) => (
          <button
            key={z.id}
            data-index={i}
            role="option"
            aria-selected={z.id === zone}
            className={`menu-row${i === highlight ? ' highlight' : ''}`}
            onMouseEnter={() => setHighlight(i)}
            onClick={() => pick(z.id)}
          >
            <Text truncate grow dir="ltr">{zoneLabel(z.id)}</Text>
            <Text size="caption" tone="secondary" dir="ltr">{z.offset}</Text>
            <span className={`check-slot${z.id === zone ? ' on' : ''}`}>
              <Icon name="checkmark" size={10} weight="semibold" />
            </span>
          </button>
        ))}
        {matches.length === 0 ? <Text size="callout" tone="secondary" className="zone-empty">{t('rotinas.editor.nenhumFuso', { busca: `\u2068${query}\u2069` })}</Text> : null}
      </div>
    </div>
  )
}

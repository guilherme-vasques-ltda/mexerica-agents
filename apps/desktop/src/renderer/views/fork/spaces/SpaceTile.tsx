import type { OwnerPresentation, SpaceTile as Tile } from '@shared/fork-spaces'
import { DUO_ICONS } from './space-icons'
import { drawable, isPhotoURL, tileColor } from './space-tile'
import './spaces.css'

// D169 (components/SpaceRail, SpaceIconPicker): a space's tile. Filled (`tile-*` with the icon in `on-tile`)
// or soft (a light tint of the color, the icon in it); up to two letters in the display face; or the company's
// logo on white. Icons are the design system's two-tone set (space-icons.ts): constants of this app, never
// markup from the Hub. An unknown icon or a bad logo draws an empty tile instead of guessing.

export function DuoGlyph({ id, size }: { id: string; size: number }) {
  const icon = DUO_ICONS[id]
  if (!icon) return null
  // Trusted constants from space-icons.ts (generated from the design bundle, simple shapes only).
  return (
    <svg viewBox="0 0 24 24" width={size} height={size} fill="currentColor" aria-hidden="true">
      <g opacity=".4" dangerouslySetInnerHTML={{ __html: icon.back }} />
      <g dangerouslySetInnerHTML={{ __html: icon.front }} />
    </svg>
  )
}

export function SpaceTile({ tile, size = 36, label }: { tile: Tile; size?: number; label?: string }) {
  const symbol = drawable(tile.symbol)
  const logo = symbol?.kind === 'logo'
  const classes = ['space-tile', tile.style === 'soft' && !logo ? 'is-soft' : '', symbol?.kind === 'letters' ? 'is-letters' : '', logo ? 'is-logo' : ''].filter(Boolean).join(' ')
  return (
    <span className={classes} role={label ? 'img' : undefined} aria-label={label} aria-hidden={label ? undefined : true}
      style={{ ['--tile' as string]: tileColor(tile.color), ['--size' as string]: `${size}px` }}>
      {symbol?.kind === 'icon' ? <DuoGlyph id={symbol.value} size={Math.round(size * 0.55)} />
        : symbol?.kind === 'letters' ? <span style={{ fontSize: Math.round(size * 0.42) }}>{symbol.value}</span>
          : symbol?.kind === 'logo' ? <img src={symbol.value} alt="" width={Math.round(size * 0.7)} height={Math.round(size * 0.7)} />
            : null}
    </span>
  )
}

/** Pessoal: round, with the owner's photo or initials as they chose them; no photo and no initials stays a plain person. */
export function PersonalTile({ owner, size = 36 }: { owner: OwnerPresentation | null; size?: number }) {
  return (
    <span className="space-tile is-person" aria-hidden style={{ ['--size' as string]: `${size}px` }}>
      {owner?.photo && isPhotoURL(owner.photo) ? <img src={owner.photo} alt="" width={size} height={size} />
        : owner?.initials ? <span style={{ fontSize: Math.round(size * 0.34) }}>{owner.initials}</span>
          : <DuoGlyph id="user" size={Math.round(size * 0.55)} />}
    </span>
  )
}

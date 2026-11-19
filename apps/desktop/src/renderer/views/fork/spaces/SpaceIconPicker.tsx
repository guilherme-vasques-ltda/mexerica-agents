import { useEffect, useId, useRef, useState } from 'react'
import type { OwnerPresentation, SpaceSymbol, SpaceTile as Tile } from '@shared/fork-spaces'
import { Button, SearchField, SegmentedChoice } from '../../../components/Controls'
import { Text } from '../../../components/Text'
import { DUO_ICONS, DUO_PACKS } from './space-icons'
import { clipName, DEFAULT_TILE, iconsFor, isLogoURL, letters, logoProblem, spaceName, TILE_COLORS, tileColor, validTile } from './space-tile'
import { logoRasterDataURL } from './logo-raster'
import { DuoGlyph, PersonalTile, SpaceTile } from './SpaceTile'
import './spaces.css'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { cru, ErroFrase, frase, textoDe, type Frase } from '../../../lib/fork-i18n/frase'

// D169 (docs/reference/redesenho/components/SpaceIconPicker): create or edit a space — name, color, Cheio/Suave
// and the tile's symbol (a two-tone icon by pack or search, up to two letters, or the company's logo), with
// the tile big and in the rail. It only builds the choice: `onSave` goes to the Hub (spaces wrapper, not here)
// and its error stays on screen with the draft. Pessoal never opens it: its tile is the owner's photo or initials.
// D454 (fatia 9c): no idioma escolhido; o problema fica guardado como `Frase` (D507) e o nome em dir="auto".

type Kind = SpaceSymbol['kind']
const KINDS: { id: Kind; label: Frase }[] = [{ id: 'icon', label: frase('spaces.picker.icone') }, { id: 'letters', label: frase('spaces.picker.letras') }, { id: 'logo', label: frase('spaces.picker.logo') }]

export interface SpaceDraft { name: string; tile: Tile }

export function SpaceIconPicker({ initial, owner = null, neighbors = [], onCancel, onSave, saveLabel, zone }: {
  /** The space being edited; absent creates one. Never Pessoal. */
  saveLabel?: string
  initial?: SpaceDraft | null
  /** For the rail preview only. */
  owner?: OwnerPresentation | null
  neighbors?: Tile[]
  onCancel: () => void
  onSave: (draft: SpaceDraft) => Promise<void>
  /** D543 E1: ciclo de vida do Space (ativar Berga, excluir) desenhado acima do rodapé. */
  zone?: React.ReactNode
}) {
  const start = initial && validTile(initial.tile) ? initial.tile : DEFAULT_TILE
  const [name, setName] = useState(initial?.name ?? '')
  const [color, setColor] = useState(start.color)
  const [style, setStyle] = useState(start.style)
  const [kind, setKind] = useState<Kind>(start.symbol.kind)
  const [icon, setIcon] = useState(start.symbol.kind === 'icon' ? start.symbol.value : DEFAULT_TILE.symbol.value)
  const [typed, setTyped] = useState(start.symbol.kind === 'letters' ? start.symbol.value : '')
  const [logo, setLogo] = useState(start.symbol.kind === 'logo' ? start.symbol.value : '')
  const [pack, setPack] = useState(DUO_PACKS.find((p) => p.icons.includes(icon))?.id ?? DUO_PACKS[0]!.id)
  const [query, setQuery] = useState('')
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const [problem, setProblem] = useState<Frase | null>(null)
  const [saving, setSaving] = useState(false)
  const [readingLogo, setReadingLogo] = useState(false)
  const logoReader = useRef<FileReader | null>(null)
  const stopLogoRead = () => {
    const previous = logoReader.current
    logoReader.current = null
    previous?.abort()
    setReadingLogo(false)
  }
  useEffect(() => () => {
    const previous = logoReader.current
    logoReader.current = null
    previous?.abort()
  }, [])
  const file = useRef<HTMLInputElement>(null)
  const nameId = useId()

  const symbol: SpaceSymbol = kind === 'icon' ? { kind, value: icon } : kind === 'letters' ? { kind, value: letters(typed) } : { kind, value: logo }
  const tile: Tile = { color, style, symbol }
  const cleanName = spaceName(name)
  const ready = !!cleanName && validTile(tile) && !readingLogo
  const icons = iconsFor(pack, query)

  const pickLogo = (picked: File | undefined) => {
    if (!picked || saving) return
    stopLogoRead()
    setLogo('')
    const issue = logoProblem(picked)
    if (issue) return setProblem(issue)
    const reader = new FileReader()
    logoReader.current = reader
    setReadingLogo(true)
    setProblem(null)
    // Only this choice may publish; even an already queued callback from an aborted reader is stale.
    const finish = (url: string) => {
      if (logoReader.current !== reader) return
      logoReader.current = null
      setReadingLogo(false)
      if (isLogoURL(url)) { setLogo(url); setProblem(null) } else setProblem(frase('spaces.picker.lerFalhou'))
    }
    reader.onload = () => {
      const raw = typeof reader.result === 'string' ? reader.result : ''
      // MX-79: raster vira no máx. 256 px antes do data URL; SVG segue como veio. Falha no
      // redimensionamento entrega o original — o logo continua válido, só maior.
      if (picked.type !== 'image/png') return finish(raw)
      void logoRasterDataURL(picked).then((url) => finish(url || raw), () => finish(raw))
    }
    reader.onerror = () => finish('')
    reader.onabort = () => finish('')
    try { reader.readAsDataURL(picked) } catch { finish('') }
  }

  const save = async () => {
    if (!ready || saving || logoReader.current) return
    setSaving(true)
    setProblem(null)
    try {
      await onSave({ name: cleanName, tile })
    } catch (e) {
      setProblem(e instanceof ErroFrase ? e.frase : e instanceof Error && e.message ? cru(e.message) : frase('spaces.picker.salvarFalhou'))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="space-picker idioma-superficie" {...superficieIdioma(idioma)} aria-label={tx(frase(initial ? 'spaces.picker.editar' : 'spaces.picker.novo'))}>
      <div className="space-picker-side">
        <SpaceTile tile={tile} size={88} label={tx(cleanName ? frase('spaces.picker.previa', { nome: cleanName }) : frase('spaces.picker.semNome'))} />
        <label htmlFor={nameId} className="space-picker-hidden-label">{tx(frase('spaces.picker.nome'))}</label>
        <input id={nameId} className="space-picker-name" value={name} placeholder={tx(frase('spaces.picker.nome'))} dir={name ? 'auto' : undefined} disabled={saving}
          onChange={(e) => setName(clipName(e.target.value))} />
        <Segmented label={tx(frase('spaces.picker.estilo'))} value={style} disabled={saving || kind === 'logo'} onChange={setStyle}
          options={[{ id: 'filled', label: tx(frase('spaces.picker.cheio')) }, { id: 'soft', label: tx(frase('spaces.picker.suave')) }]} />
        <div className="space-picker-rail" aria-hidden>
          <PersonalTile owner={owner} />
          <SpaceTile tile={tile} />
          {neighbors.filter(validTile).slice(0, 2).map((t, i) => <SpaceTile key={i} tile={t} />)}
        </div>
      </div>

      <div className="space-picker-main">
        <div className="space-picker-row">
          <span className="space-picker-label" id={`${nameId}-cor`}>{tx(frase('spaces.picker.cor'))}</span>
          <div className="space-picker-colors" role="group" aria-labelledby={`${nameId}-cor`}>
            {TILE_COLORS.map((c) => (
              <button key={c.id} type="button" aria-label={c.name} aria-pressed={color === c.id} disabled={saving}
                style={{ ['--c' as string]: tileColor(c.id) }} onClick={() => setColor(c.id)} />
            ))}
          </div>
        </div>
        <Segmented label={tx(frase('spaces.picker.tipoSimbolo'))} value={kind} disabled={saving} onChange={(next) => { stopLogoRead(); setKind(next); setProblem(null) }} options={KINDS.map((k) => ({ id: k.id, label: tx(k.label) }))} />

        {kind === 'icon' ? (
          <div className="space-picker-pane">
            <div className="space-picker-row">
              <div className="space-picker-packs" role="group" aria-label={tx(frase('spaces.picker.packs'))}>
                {DUO_PACKS.map((p) => (
                  <button key={p.id} type="button" aria-pressed={!query && pack === p.id} disabled={saving} onClick={() => { setPack(p.id); setQuery('') }}>{p.name}</button>
                ))}
              </div>
              <div className="space-picker-search"><SearchField value={query} onChange={setQuery} prompt={tx(frase('spaces.picker.buscar'))} clearLabel={tx(frase('spaces.picker.limpar'))} inputProps={{ 'aria-label': tx(frase('spaces.picker.buscar')) }} /></div>
            </div>
            <div className="space-picker-grid" role="group" aria-label={tx(frase('spaces.picker.icones'))}>
              {icons.map((id) => (
                <button key={id} type="button" aria-pressed={icon === id} aria-label={DUO_ICONS[id]!.name} title={DUO_ICONS[id]!.name} disabled={saving} onClick={() => setIcon(id)}>
                  <DuoGlyph id={id} size={20} />
                </button>
              ))}
              {!icons.length ? <Text as="div" size="callout" tone="secondary">{tx(frase('spaces.picker.nenhumIcone'))}</Text> : null}
            </div>
          </div>
        ) : kind === 'letters' ? (
          <label className="space-picker-letters">
            <span>{tx(frase('spaces.picker.ateDuas'))}</span>
            <input value={typed} maxLength={4} disabled={saving} onChange={(e) => setTyped(letters(e.target.value))} aria-label={tx(frase('spaces.picker.letrasTile'))} />
          </label>
        ) : (
          <div className="space-picker-pane">
            <button type="button" className="space-picker-drop" disabled={saving} onClick={() => file.current?.click()}
              onDragOver={(e) => e.preventDefault()} onDrop={(e) => { e.preventDefault(); pickLogo(e.dataTransfer.files[0]) }}>
              {logo ? <img src={logo} alt={tx(frase('spaces.picker.logoEscolhido'))} width={56} height={56} /> : null}
              <span>{tx(frase(logo ? 'spaces.picker.trocarLogo' : 'spaces.picker.arraste'))}</span>
              <Text as="span" size="caption" tone="secondary">{tx(frase('spaces.picker.logoDica'))}</Text>
            </button>
            <input ref={file} type="file" accept="image/png,image/svg+xml" hidden onChange={(e) => { pickLogo(e.target.files?.[0]); e.target.value = '' }} />
          </div>
        )}

        {zone}
        <div className="space-picker-foot">
          <Text as="div" size="caption" tone={problem ? 'danger' : 'secondary'} role={problem ? 'alert' : undefined} className="space-picker-problem">
            {problem ? tx(problem) : !cleanName ? tx(frase('spaces.picker.deNome')) : kind === 'letters' && !symbol.value ? tx(frase('spaces.picker.escrevaLetras')) : kind === 'logo' && !logo ? tx(frase('spaces.picker.escolhaLogo')) : ''}
          </Text>
          <Button kind="secondary" disabled={saving} onClick={onCancel}>{tx(frase('spaces.picker.cancelar'))}</Button>
          <Button disabled={!ready || saving} onClick={() => void save()}>{saveLabel && !saving ? saveLabel : tx(frase(saving ? 'spaces.picker.salvando' : 'spaces.picker.salvar'))}</Button>
        </div>
      </div>
    </div>
  )
}

/** The app's own segmented control, labelled as a group and inert while a save is on its way. */
function Segmented<T extends string>({ label, value, options, onChange, disabled }: { label: string; value: T; options: { id: T; label: string }[]; onChange: (v: T) => void; disabled?: boolean }) {
  return (
    <div className={`space-picker-seg${disabled ? ' is-disabled' : ''}`} role="group" aria-label={label} inert={disabled}>
      <SegmentedChoice selection={value} options={options} onChange={onChange} />
    </div>
  )
}

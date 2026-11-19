import { useRef, useState, type CSSProperties } from 'react'
import { CardForm, CardSection, ChoicePicker, SegmentedChoice, Slider, Switch, ValueRow } from '../../components/Controls'
import { Icon } from '../../components/Icon'
import { Text } from '../../components/Text'
import {
  ACCENTS, BLUR_MAX, CUSTOM_ACCEPT, CUSTOM_ERROR_KEYS, CUSTOM_ERRORS, HOUSE_WALLPAPERS, THEMES, VEIL_DEFAULT, VEIL_MAX,
  appearancePrefs, chooseCustomWallpaper, offeredIn, removeCustomWallpaper, useAppearance, wallpaperThumbURL, wallpaperURL,
} from '../../lib/appearance'
import { transparencyFor } from '../../lib/legibility'
import { areaChoice, followsGeneral, TRANSPARENCY_AREAS, TRANSPARENCY_MODES, withArea, type TransparencyArea } from '../../lib/transparency-areas'
import { DEFAULT_TEXT_SIZE, prefs, TEXT_SIZES, usePref } from '../../lib/prefs'
import { comAtalho, macDesteAparelho, webDesteAparelho } from '../../lib/atalhos'
import { APP_FONTS, fontStack } from '../../lib/appearance-choices'
import { useT, useIdioma, superficieIdioma } from '../../lib/fork-i18n/estado'
import { MarcaMexerica } from '../../components/toques-mexerica/Toques'
import './appearance.css'
import { isMac } from './parts'
import { semParametros } from './rotulo'
import { ProviderMascot } from '../usage/ProviderMascot'

// D165, fase A: Ajustes → Aparência (docs/reference/redesenho/components/AppearancePanel). Theme, accent,
// the wallpaper with blur, transparency and veil, and the text/sidebar preferences that were in Geral.

/** The wallpaper and its veil under the chat window's split; nothing without a wallpaper. */
export function WallpaperLayers() {
  const { dark, customURL } = useAppearance()
  const [id] = usePref(appearancePrefs.wallpaper)
  // `customURL` is read so a picture loaded later shows without another preference change.
  if (!wallpaperURL(id, dark) || (id === 'custom' && !customURL)) return null
  return (
    <>
      <div className="app-wallpaper" aria-hidden />
      <div className="app-veil" aria-hidden />
    </>
  )
}

export function AppearanceSettings() {
  const t = useT()
  const tx = semParametros(t)
  const idioma = useIdioma()
  const appearance = useAppearance()
  const [theme, setTheme] = usePref(appearancePrefs.theme)
  const [accent, setAccent] = usePref(appearancePrefs.accent)
  const [touches, setTouches] = usePref(appearancePrefs.mexericaTouches)
  const [font, setFont] = usePref(appearancePrefs.font)
  const [wallpaper, setWallpaper] = usePref(appearancePrefs.wallpaper)
  const [blur, setBlur] = usePref(appearancePrefs.blur)
  const [transparency, setTransparency] = usePref(appearancePrefs.transparency)
  const [transparencyMode, setTransparencyMode] = usePref(appearancePrefs.transparencyMode)
  const [ownAreas, setOwnAreas] = usePref(appearancePrefs.transparencyAreas)
  const [veil, setVeil] = usePref(appearancePrefs.veil)
  const [textSize, setTextSize] = usePref(prefs.textSize)
  const [usageIconStyle, setUsageIconStyle] = usePref(prefs.usageIconStyle)
  const [compact, setCompact] = usePref(prefs.sidebarCompact)
  const [pickError, setPickError] = useState<string | null>(null)
  const file = useRef<HTMLInputElement>(null)
  const hasWallpaper = wallpaper !== 'none' && offeredIn(wallpaper, appearance.dark) && (wallpaper !== 'custom' || appearance.customURL !== null)
  // Noite chosen in the dark theme: the light one shows no wallpaper, and says why (aparencia-01).
  const hiddenHere = !offeredIn(wallpaper, appearance.dark)
  const veilValue = veil ?? VEIL_DEFAULT[appearance.dark ? 'dark' : 'light']
  const effective = transparencyFor(appearance.glass)
  // Why a glass shows less than was picked: the Mac's contrast, or this wallpaper's legibility floor.
  const heldNote = (glass: number) => appearance.moreContrast && glass >= 0.92 && appearance.floor < 0.92
    ? t('ajustes.aparencia.limitadoContraste', { valor: transparencyFor(glass) })
    : t('ajustes.aparencia.limitadoFundo', { valor: transparencyFor(glass) })
  const transparencyNote = !hasWallpaper
    ? t('ajustes.aparencia.semFundo')
    : appearance.reducedTransparency
      ? t('ajustes.aparencia.menosTransparencia')
      : appearance.held
        ? heldNote(appearance.glass)
        : transparencyMode === 'custom' ? t('ajustes.aparencia.basePorArea') : null
  // D169: Por área. Each area shows what it uses now; one without its own value follows the general one.
  const areaSlider = (area: TransparencyArea, label: string) => {
    const value = areaChoice(area, 'custom', transparency, ownAreas)
    const glass = appearance.areas[area]
    const plate = area === 'conversation'
    const held = hasWallpaper && !plate && (glass.held || appearance.reducedTransparency)
    const reset = followsGeneral(area, transparencyMode, ownAreas)
      ? <Text tone="secondary">{t('ajustes.aparencia.segueGeral')}</Text>
      : <button className="appearance-link" onClick={() => setOwnAreas(withArea(ownAreas, area, null))}>{t('ajustes.aparencia.usarGeral')}</button>
    return (
      <Slider
        key={area}
        label={label}
        value={value}
        min={0}
        max={100}
        onChange={(v) => setOwnAreas(withArea(ownAreas, area, v))}
        shown={`${hasWallpaper && !plate ? transparencyFor(glass.glass) : value}%`}
        held={held}
        note={hasWallpaper && !plate && glass.held && !appearance.reducedTransparency ? <>{heldNote(glass.glass)} {reset}</> : reset}
        disabled={!hasWallpaper}
      />
    )
  }

  const pick = async (f: File | undefined) => {
    if (!f) return
    setPickError(null)
    try {
      await chooseCustomWallpaper(f)
    } catch (e) {
      // Our own sentence (lib/appearance.ts), never the browser's English text.
      const qual = e instanceof Error && e.message === CUSTOM_ERRORS.heic ? 'heic' : 'unreadable'
      setPickError(t(CUSTOM_ERROR_KEYS[qual]))
    }
  }

  return (
    <div className="appearance-workspace idioma-superficie" {...superficieIdioma(idioma)}>
      <div className="appearance-visualization"><AppearancePreview /></div>
      <div className="appearance-controls" role="region" aria-label={t('ajustes.aparencia.controles')} tabIndex={0}>
        <CardForm>
          <CardSection title={t('ajustes.aparencia.tema')}>
            <ValueRow label={t('ajustes.aparencia.aparencia')}>
              <SegmentedChoice selection={theme} options={THEMES.map((x) => ({ id: x.id, label: tx(x.labelKey) }))} onChange={setTheme} />
            </ValueRow>
            <div className="appearance-row">
              <Text as="div" id="appearance-accent-label">{t('ajustes.aparencia.destaque')}</Text>
              <div className="appearance-swatches" role="radiogroup" aria-labelledby="appearance-accent-label">
                {ACCENTS.map((a) => (
                  <button
                    key={a.id}
                    role="radio"
                    aria-checked={a.id === accent}
                    aria-label={tx(a.labelKey)}
                    title={tx(a.labelKey)}
                    className="appearance-swatch"
                    style={{ '--c': `var(--accent-${a.id})` } as CSSProperties}
                    onClick={() => setAccent(a.id)}
                  />
                ))}
              </div>
            </div>
            {/* Barra04 (dono, 05/11/2026): cada fonte escrita nela mesma; padrão DM Sans, embutidas no app. */}
            <div className="appearance-row appearance-fonts-row">
              <Text as="div" id="appearance-font-label">{t('ajustes.aparencia.fonte')}</Text>
              <div className="appearance-fonts" role="radiogroup" aria-labelledby="appearance-font-label">
                {APP_FONTS.map((f) => (
                  <button key={f.id} role="radio" aria-checked={f.id === font} className="appearance-font"
                    style={{ fontFamily: fontStack(f.id) ?? 'var(--font-system)' }} onClick={() => setFont(f.id)}>
                    {'nameKey' in f ? t(f.nameKey) : f.name}
                  </button>
                ))}
              </div>
            </div>
          </CardSection>
          <CardSection footer={t('aparencia.toques.descricao')}>
            <div className="mx-touch-setting">
              <MarcaMexerica label={t('marca.app.nome')} />
              <Switch on={touches} onChange={setTouches}>{t('aparencia.toques.titulo')}</Switch>
            </div>
          </CardSection>
          <CardSection
            title={t('ajustes.aparencia.fundo')}
            footer={pickError ? <Text tone="danger">{pickError}</Text> : hiddenHere ? t('ajustes.aparencia.noiteSoEscuro') : isMac ? t('ajustes.aparencia.fundoRodapeMac') : t('ajustes.aparencia.fundoRodape')}
          >
            <div className="appearance-row appearance-wallpapers" role="radiogroup" aria-label={t('ajustes.aparencia.fundo')}>
              {HOUSE_WALLPAPERS.filter((w) => offeredIn(w.id, appearance.dark)).map((w) => (
                <WallpaperThumb key={w.id} id={w.id} label={w.labelKey ? tx(w.labelKey) : w.label} language={w.labelKey ? superficieIdioma(idioma) : undefined} image={wallpaperThumbURL(w.id, appearance.dark) ?? undefined} selected={wallpaper === w.id} onPick={() => setWallpaper(w.id)} />
              ))}
              <WallpaperThumb id="none" label={t('ajustes.aparencia.nenhum')} selected={wallpaper === 'none' || hiddenHere} onPick={() => setWallpaper('none')} />
              {appearance.customURL ? (
                <WallpaperThumb id="custom" label={t('ajustes.aparencia.suaImagem')} image={appearance.customThumbURL ?? appearance.customURL} selected={wallpaper === 'custom'} onPick={() => setWallpaper('custom')} />
              ) : null}
              <button className="appearance-thumb pick" onClick={() => file.current?.click()} title={t('ajustes.aparencia.escolherImagem')}>
                <Icon name="plus" size={13} />
                <span className="appearance-thumb-label">{t('ajustes.aparencia.escolherImagem')}</span>
              </button>
              <input ref={file} type="file" accept={CUSTOM_ACCEPT} hidden onChange={(e) => { void pick(e.currentTarget.files?.[0]); e.currentTarget.value = '' }} />
            </div>
            {appearance.customURL ? (
              <div className="appearance-row appearance-inline">
                <Text tone="secondary" grow>{t('ajustes.aparencia.imagemGuardada')}</Text>
                <button className="appearance-link" onClick={() => void removeCustomWallpaper()}>{t('ajustes.aparencia.removerImagem')}</button>
              </div>
            ) : null}
            <Slider label={t('ajustes.aparencia.desfoque')} value={blur} min={0} max={BLUR_MAX} onChange={setBlur} shown={`${blur} px`} disabled={!hasWallpaper} />
            <ValueRow label={t('ajustes.aparencia.transparencia')}>
              <SegmentedChoice selection={transparencyMode} options={TRANSPARENCY_MODES.map((x) => ({ id: x.id, label: tx(x.labelKey) }))} onChange={setTransparencyMode} />
            </ValueRow>
            <Slider
              label={t('ajustes.transparencia.geral')}
              value={transparency}
              min={0}
              max={100}
              onChange={setTransparency}
              shown={`${hasWallpaper ? effective : transparency}%`}
              held={hasWallpaper && (appearance.held || appearance.reducedTransparency)}
              note={transparencyNote}
              disabled={!hasWallpaper}
            />
            {transparencyMode === 'custom' ? TRANSPARENCY_AREAS.map((a) => areaSlider(a.id, tx(a.labelKey))) : null}
            <Slider
              label={t('ajustes.aparencia.suavizar')}
              value={Math.round(veilValue * 100)}
              min={0}
              max={Math.round(VEIL_MAX * 100)}
              onChange={(v) => setVeil(v / 100)}
              shown={`${Math.round(veilValue * 100)}%`}
              note={veil !== null && hasWallpaper ? <button className="appearance-link" onClick={() => setVeil(null)}>{t('ajustes.aparencia.padraoDoTema')}</button> : null}
              disabled={!hasWallpaper}
            />
          </CardSection>
          <CardSection title={t('ajustes.aparencia.textoEBarra')}>
            <ValueRow label={t('ajustes.aparencia.tamanhoTexto')}>
              <ChoicePicker restoreFocusOnClose language={superficieIdioma(idioma)}
                selection={textSize}
                options={TEXT_SIZES.map((s) => ({ id: s, label: s === DEFAULT_TEXT_SIZE ? t('ajustes.aparencia.ptPadrao', { tamanho: s }) : t('ajustes.aparencia.pt', { tamanho: s }) }))}
                onChange={setTextSize}
              />
            </ValueRow>
            <ValueRow label={t('ajustes.aparencia.iconesUso')}>
              <ChoicePicker restoreFocusOnClose language={superficieIdioma(idioma)} selection={usageIconStyle} options={[{ id: 'character', label: t('ajustes.aparencia.personagem') }, { id: 'original', label: t('ajustes.aparencia.original') }]} onChange={setUsageIconStyle} />
            </ValueRow>
            <Switch on={compact} onChange={setCompact}>
              {comAtalho(t('ajustes.aparencia.recolherBarra'), 'barraLateral', macDesteAparelho(), webDesteAparelho())}
            </Switch>
          </CardSection>
        </CardForm>
      </div>
    </div>
  )
}

function WallpaperThumb({ id, label, image, selected, onPick, language }: { language?: { lang: string; dir: string }; id: string; label: string; image?: string; selected: boolean; onPick: () => void }) {
  return (
    <button
      {...language}
      role="radio"
      aria-checked={selected}
      aria-label={label}
      title={label}
      data-wallpaper={id}
      className={`appearance-thumb${image ? '' : ' plain'}`}
      style={image ? ({ '--thumb': `url("${image}")` } as CSSProperties) : undefined}
      onClick={onPick}
    >
      {image ? null : <span className="appearance-thumb-label">{label}</span>}
    </button>
  )
}

/** D529b: prévia sem interação, nas mesmas preferências e tokens do app. */
function AppearancePreview() {
  const t = useT()
  const { dark } = useAppearance()
  const [id] = usePref(appearancePrefs.wallpaper)
  const [compact] = usePref(prefs.sidebarCompact)
  const [usageIconStyle] = usePref(prefs.usageIconStyle)
  const plain = !wallpaperURL(id, dark)
  return (
    <div className={`appearance-preview${plain ? ' plain' : ''}${compact ? ' compact' : ''}`} aria-hidden>
      {plain ? null : <div className="appearance-preview-wallpaper" />}
      {plain ? null : <div className="appearance-preview-veil" />}
      <div className="appearance-preview-sidebar">
        <span className="appearance-preview-line strong" />
        <span className="appearance-preview-line selected" />
        <span className="appearance-preview-line" />
        <span className="appearance-preview-line" />
        <div className="appearance-preview-usage">
          {['codex', 'claude'].map((id) => (
            <ProviderMascot key={id} provider={{ id, name: id, windows: [], source: '', updatedAt: 0 }} size={18} style={usageIconStyle} />
          ))}
        </div>
      </div>
      <div className="appearance-preview-chat">
        <div className="appearance-preview-bubble">{t('ajustes.aparencia.previaRecebida')}</div>
        <div className="appearance-preview-bubble mine">{t('ajustes.aparencia.previaEnviada')}</div>
        <div className="appearance-preview-composer">
          <span>{t('ajustes.aparencia.previaMensagem')}</span>
          <span className="appearance-preview-send" />
        </div>
      </div>
    </div>
  )
}

import { useEffect, useState } from 'react'
import type { Space } from '@shared/fork-spaces'
import type { BotEditor } from '@shared/fork-bot-editor'
import { CORES } from '../../../lib/mexeriquinhas/index.ts'
import { Button } from '../../../components/Controls'
import { Text } from '../../../components/Text'
import { readSpaceBotEditor, saveSpaceBotEditor, type SpacesOrigin } from '../../../store/fork-spaces'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { cru, ErroFrase, frase, textoDe, type Frase } from '../../../lib/fork-i18n/frase'
import { spaceLifecycleOk } from '../../../lib/fork-spaces-rail'
import { BERGA_CORES, BERGA_FIGURES, bergaAppearance, type BergaCor, type BergaFigure } from './berga-figures'
import { FIGURE_NAME, FigurePreview } from './SpacePrincipal'
import '../../../lib/mexeriquinhas/mexeriquinhas.css'
import './spaces.css'

// D543 E1 (tela 3): depois de ativado, o dono troca só a figura e a cor do Berga
// deste Space — e um principal legado com outro nome pode ser renomeado para "Berga".
// A gravação passa pela intenção do editor de bot (CAS por settingsVersion); o Hub
// recusa qualquer outro campo do principal (space_principal_fixed).

const CORE_NAME = Object.fromEntries(BERGA_CORES.map((c) => [c, frase(`spaces.principal.cor.${c}`)])) as Record<BergaCor, Frase>

function currentPick(editor: BotEditor | null): { figure: BergaFigure; cor: BergaCor } {
  const mexeriquinha = editor?.profile.mexeriquinha
  const aparencia = mexeriquinha && 'aparencia' in mexeriquinha ? mexeriquinha.aparencia : null
  const figure = aparencia ? BERGA_FIGURES.find((f) => f.olhos === aparencia.olhos && f.roupa === aparencia.roupa && f.acessorio === aparencia.acessorio && f.objeto === aparencia.objeto) ?? BERGA_FIGURES[0]! : BERGA_FIGURES[0]!
  const cor = aparencia && (BERGA_CORES as readonly string[]).includes(aparencia.cor) ? aparencia.cor as BergaCor : 'laranja'
  return { figure, cor }
}

export function SpacePrincipalLook({ origin, space, onBack, onClose }: { origin: SpacesOrigin; space: Space; onBack: () => void; onClose: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const botId = space.principalBotId ?? ''
  const [editor, setEditor] = useState<BotEditor | null>(null)
  const [state, setState] = useState<'loading' | 'ready' | 'error'>('loading')
  const [figure, setFigure] = useState<BergaFigure>(BERGA_FIGURES[0]!)
  const [cor, setCor] = useState<BergaCor>('laranja')
  const [dirty, setDirty] = useState(false)
  const [saving, setSaving] = useState(false)
  const [problem, setProblem] = useState<Frase | null>(null)
  const supported = spaceLifecycleOk(origin.store.hello)
  useEffect(() => {
    if (!supported || !botId || origin.store.connectedClient !== origin.client || origin.store.hello?.hostId !== origin.hostId || origin.store.principalEpoch !== origin.epoch) { setState('error'); return }
    let live = true
    void readSpaceBotEditor(origin, space.id, botId).then((answer) => {
      if (!live) return
      if (origin.store.connectedClient !== origin.client || origin.store.hello?.hostId !== origin.hostId || origin.store.principalEpoch !== origin.epoch) { setState('error'); return }
      setEditor(answer)
      const pick = currentPick(answer)
      setFigure(pick.figure)
      setCor(pick.cor)
      setState('ready')
    }, () => { if (live) setState('error') })
    return () => { live = false }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [origin, supported, botId])
  const problemOf = (error: unknown) => error instanceof ErroFrase ? error.frase : error instanceof Error && error.message ? cru(error.message) : frase('spaces.principal.falhou')
  const rename = async () => {
    if (!editor || saving) return
    setSaving(true)
    setProblem(null)
    try {
      const answer = await saveSpaceBotEditor(origin, space.id, botId, editor.settingsVersion, { name: 'Berga' })
      setEditor(answer)
    } catch (error) { setProblem(problemOf(error)) } finally { setSaving(false) }
  }
  const save = async () => {
    if (!editor || saving || !dirty) return
    setSaving(true)
    setProblem(null)
    try {
      await saveSpaceBotEditor(origin, space.id, botId, editor.settingsVersion, { mexeriquinha: bergaAppearance(figure, cor) })
      onClose()
    } catch (error) { setProblem(problemOf(error)) } finally { setSaving(false) }
  }
  const unavailable = !supported ? frase('spaces.principal.atualize')
    : state === 'loading' ? frase('spaces.birth.carregando')
      : state === 'error' || !editor ? frase('spaces.birth.consultaFalhou') : null
  return <section className="space-birth idioma-superficie" aria-label={t('spaces.sheet.aparencia')} {...superficieIdioma(idioma)}>
    <div className="space-birth-heading"><div><h2>{t('spaces.sheet.aparencia')}</h2><p dir="auto">{space.name}</p></div></div>
    {editor && editor.profile.name.trim() !== 'Berga' ? (
      <div className="space-zone-row">
        <Text as="div" size="callout" tone="secondary">{t('spaces.sheet.nomeAntigo', { nome: editor.profile.name })}</Text>
        <Button kind="secondary" disabled={saving} onClick={() => void rename()}>{t('spaces.sheet.renomear')}</Button>
      </div>
    ) : null}
    {unavailable ? <p role="status">{tx(unavailable)}</p> : (
      <>
        <div className="space-berga-figuras" role="group" aria-label={t('spaces.principal.figura')}>
          <span className="space-picker-label">{t('spaces.principal.figura')}</span>
          {BERGA_FIGURES.map((f) => (
            <button key={f.id} type="button" className="space-berga-figura" aria-pressed={figure.id === f.id} disabled={saving} onClick={() => { setFigure(f); setDirty(true) }}>
              <FigurePreview figure={f} cor={cor} size={40} />
              <span>{tx(FIGURE_NAME[f.id])}</span>
            </button>
          ))}
        </div>
        <div className="space-berga-cores" role="group" aria-label={t('spaces.principal.cor')}>
          <span className="space-picker-label">{t('spaces.principal.cor')}</span>
          {BERGA_CORES.map((c) => {
            const base = CORES.find((k) => k.id === c)?.base ?? '#888'
            return <button key={c} type="button" aria-label={tx(CORE_NAME[c])} aria-pressed={cor === c} disabled={saving}
              style={{ ['--c' as string]: base }} onClick={() => { setCor(c); setDirty(true) }} />
          })}
        </div>
      </>)}
    {problem ? <p role="alert">{tx(problem)}</p> : null}
    <footer>
      <Button kind="secondary" disabled={saving} onClick={onBack}>{t('spaces.birth.voltar')}</Button>
      <Button kind="secondary" disabled={saving} onClick={onClose}>{t('spaces.birth.cancelar')}</Button>
      <Button disabled={saving || !dirty || !!unavailable} onClick={() => void save()}>{t(saving ? 'spaces.principal.ativando' : 'spaces.sheet.salvarAparencia')}</Button>
    </footer>
  </section>
}

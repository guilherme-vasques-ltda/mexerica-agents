import { useEffect, useId, useState } from 'react'
import type { Space } from '@shared/fork-spaces'
import type { Engine, ManagedAccount } from '@shared/fork-engine-accounts'
import { birthAccounts, birthDefault } from '@shared/fork-space-birth'
import { defaultNewBotBackend } from '@shared/new-bot-defaults'
import { Button } from '../../../components/Controls'
import { settingsScreen } from '../ajustes/tela'
import { engineSetup } from '../../../store/fork-engine-setup'
import { activateSpacePrincipal, principalIntent, type SpacesOrigin } from '../../../store/fork-spaces'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { cru, ErroFrase, frase, textoDe, type Frase } from '../../../lib/fork-i18n/frase'
import { spaceLifecycleOk } from '../../../lib/fork-spaces-rail'
import { renderSimples, CORES } from '../../../lib/mexeriquinhas/index.ts'
import { BERGA_CORES, BERGA_FIGURES, bergaAppearance, type BergaCor, type BergaFigure } from './berga-figures'
import '../../../lib/mexeriquinhas/mexeriquinhas.css'
import './spaces.css'

// D543 E1: ativa o Berga num Space legado. Nome e persona são fixos no Hub; aqui só
// motor, conta, figura e cor. Uma nonce por montagem: repetir o envio é idempotente.

export const FIGURE_NAME: Record<BergaFigure['id'], Frase> = {
  simples: frase('spaces.principal.figura.simples'),
  elegante: frase('spaces.principal.figura.elegante'),
  despojado: frase('spaces.principal.figura.despojado'),
  brabo: frase('spaces.principal.figura.brabo'),
  praieiro: frase('spaces.principal.figura.praieiro'),
  curioso: frase('spaces.principal.figura.curioso'),
}
const CORE_NAME = Object.fromEntries(BERGA_CORES.map((c) => [c, frase(`spaces.principal.cor.${c}`)])) as Record<BergaCor, Frase>

export function FigurePreview({ figure, cor, size }: { figure: BergaFigure; cor: BergaCor; size: number }) {
  const id = `berga-${useId().replace(/[^A-Za-z0-9_-]/g, '')}`
  const aparencia = bergaAppearance(figure, cor).aparencia
  return <span className="space-berga-preview" aria-hidden="true"
    dangerouslySetInnerHTML={{ __html: renderSimples(aparencia, { size, id, label: '' }) }} />
}

export function SpacePrincipal({ origin, space, onBack, onClose }: { origin: SpacesOrigin; space: Space; onBack: () => void; onClose: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const [intent] = useState(() => principalIntent(origin, space.id))
  const [nonce] = useState(() => crypto.randomUUID())
  const [backend, setBackend] = useState<Engine>(() => defaultNewBotBackend(origin.store.hello?.backends ?? []) === 'claude' ? 'claude' : 'codex')
  const [accounts, setAccounts] = useState<ManagedAccount[]>([])
  const [state, setState] = useState<'loading' | 'ready' | 'error'>('loading')
  const [accountId, setAccountId] = useState<string | null>(null)
  const [confirmed, setConfirmed] = useState(false)
  const [figure, setFigure] = useState<BergaFigure>(BERGA_FIGURES[0]!)
  const [cor, setCor] = useState<BergaCor>('laranja')
  const [saving, setSaving] = useState(false)
  const [problem, setProblem] = useState<Frase | null>(null)
  const supported = spaceLifecycleOk(origin.store.hello)
  useEffect(() => {
    if (!supported || origin.store.connectedClient !== origin.client || origin.store.hello?.hostId !== origin.hostId || origin.store.principalEpoch !== origin.epoch) { setState('error'); return }
    let live = true
    void engineSetup.accounts(origin.client).then((result) => {
      if (!live) return
      if (origin.store.connectedClient !== origin.client || origin.store.hello?.hostId !== origin.hostId || origin.store.principalEpoch !== origin.epoch) { setState('error'); return }
      setAccounts(result.accounts)
      setState('ready')
    }, () => { if (live) setState('error') })
    return () => { live = false }
  }, [origin, supported])
  const engineName = (id: Engine) => origin.store.hello?.backends.find((b) => b.id === id)?.name ?? id
  const options = birthAccounts(accounts, backend)
  const selected = accountId ?? birthDefault(accounts, backend)
  const account = options.find((a) => a.id === selected)
  const ready = supported && state === 'ready' && !!account && confirmed
  const save = async () => {
    if (!ready || saving) return
    setSaving(true)
    setProblem(null)
    try {
      await activateSpacePrincipal(origin, space.id, { clientNonce: nonce, backend, engineAccountId: selected, appearance: bergaAppearance(figure, cor) }, intent)
      onClose()
    } catch (error) {
      setProblem(error instanceof ErroFrase ? error.frase : error instanceof Error && error.message ? cru(error.message) : frase('spaces.principal.falhou'))
    } finally { setSaving(false) }
  }
  const unavailable = !supported ? frase('spaces.principal.atualize')
    : state === 'loading' ? frase('spaces.birth.carregando')
      : state === 'error' ? frase('spaces.birth.consultaFalhou')
        : !options.length ? frase('spaces.principal.semConta') : null
  return <section className="space-birth idioma-superficie" aria-label={t('spaces.principal.titulo')} {...superficieIdioma(idioma)}>
    <div className="space-birth-heading"><div><h2>{t('spaces.principal.titulo')}</h2><p dir="auto">{space.name}</p></div></div>
    <p>{t('spaces.principal.separado')}</p>
    <fieldset disabled={saving || intent.fixed}>
      <label>{t('spaces.birth.motor')}<select value={backend} onChange={(e) => { setBackend(e.target.value as Engine); setAccountId(null); setConfirmed(false) }}>{(['codex', 'claude'] as const).map((id) => <option key={id} value={id}>{engineName(id)}</option>)}</select></label>
      <label>{t('spaces.birth.conta')}<select value={selected} onChange={(e) => { setAccountId(e.target.value); setConfirmed(false) }} disabled={state !== 'ready'}>
        <option value="">{t('spaces.birth.escolha')}</option>
        {options.map((a) => <option key={a.id} value={a.id}>{a.origin === 'legacy' ? t('spaces.birth.padrao', { conta: a.label }) : a.label}</option>)}
      </select></label>
      <label className="space-birth-confirm"><input type="checkbox" checked={confirmed} disabled={!account} onChange={(e) => setConfirmed(e.target.checked)} />{t('spaces.birth.confirmo')}</label>
    </fieldset>
    <div className="space-berga-figuras" role="group" aria-label={t('spaces.principal.figura')}>
      <span className="space-picker-label">{t('spaces.principal.figura')}</span>
      {BERGA_FIGURES.map((f) => (
        <button key={f.id} type="button" className="space-berga-figura" aria-pressed={figure.id === f.id} disabled={saving || intent.fixed} onClick={() => setFigure(f)}>
          <FigurePreview figure={f} cor={cor} size={40} />
          <span>{tx(FIGURE_NAME[f.id])}</span>
        </button>
      ))}
    </div>
    <div className="space-berga-cores" role="group" aria-label={t('spaces.principal.cor')}>
      <span className="space-picker-label">{t('spaces.principal.cor')}</span>
      {BERGA_CORES.map((c) => {
        const base = CORES.find((k) => k.id === c)?.base ?? '#888'
        return <button key={c} type="button" aria-label={tx(CORE_NAME[c])} aria-pressed={cor === c} disabled={saving || intent.fixed}
          style={{ ['--c' as string]: base }} onClick={() => setCor(c)} />
      })}
    </div>
    {unavailable ? <p role="status">{tx(unavailable)}</p> : null}
    {supported && state !== 'loading' && state !== 'error' && !options.length ? <Button kind="secondary" onClick={() => settingsScreen.show('ai-accounts', 'contas')}>{t('conversa.avisos.falha.acao.abrirContas')}</Button> : null}
    {problem ? <p role="alert">{tx(problem)}</p> : null}
    <footer><Button kind="secondary" disabled={saving} onClick={onBack}>{t('spaces.birth.voltar')}</Button><Button kind="secondary" disabled={saving} onClick={onClose}>{t('spaces.birth.cancelar')}</Button><Button disabled={saving || !ready} onClick={() => void save()}>{t(saving ? 'spaces.principal.ativando' : 'spaces.principal.ativar')}</Button></footer>
  </section>
}

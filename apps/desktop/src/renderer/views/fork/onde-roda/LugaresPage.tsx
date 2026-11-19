import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from 'react'
import { decodeManifest, type Intent, type Place } from '../../../../shared/fork-workplaces'
import { Button, CardForm, CardSection, Spinner } from '../../../components/Controls'
import { Dialog } from '../../../components/Overlay'
import { useApp } from '../../../store/context'
import type { BotStore } from '../../../store/bot-store'
import { Workplaces } from '../../../store/fork-workplaces'
import { CadastrarLugar } from './CadastrarLugar'
import { ChavesPublicas, GuiaPreparo } from './GuiaPreparo'
import { EditorLugar } from './EditorLugar'
import { VerificarLugar } from './VerificarLugar'
import { createPlaceDraft, reconcilePlaceDraft } from './formulario'
import { chaveRascunho, chaveSelecionado } from './chaves'
import { useEstadoDaPagina } from '../../settings/estado-pagina'
import { placeFocusScope, trackStepFocus, type FocusScope, type StepFocus } from './foco'
import './lugares.css'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'

// Nome de protocolo e formato técnico: iguais em todo idioma.
const SSH_ROTULO = 'SSH' // textos-ok: nome do protocolo, não traduzível
const CHAVE_EXEMPLO = 'ssh-ed25519 …' // textos-ok: formato técnico da linha de known_hosts

export function LugaresPage() {
  const app = useApp()
  const stores = app.computers.flatMap((c) => app.store(c.id) ?? [])
  const t = useT()
  const idioma = useIdioma()
  // DON-917: a página declara o idioma e a direção (a moldura de Ajustes só cobre o cabeçalho), sem criar caixa.
  return <div className="idioma-superficie" style={{ display: 'contents' }} {...superficieIdioma(idioma)}><CardForm>
    <p>{t('lugares.intro')}</p>
    {!stores.length && <p>{t('lugares.conecte')}</p>}
    {stores.map((source) => <HubPlaces key={source.computer.id} source={source} />)}
  </CardForm></div>
}
function HubPlaces({ source }: { source: BotStore }) {
  const [model, setModel] = useState<Workplaces | null>(null)
  useEffect(() => {
    const next = new Workplaces(source); setModel(next)
    return () => next.dispose()
  }, [source])
  const t = useT()
  // Cada Hub tem o seu estado de tela (rascunhos, seleção): a chave leva o computador dele (estado-pagina.ts).
  return <CardSection title={t('lugares.hub')}>{model && <BoundPlaces model={model} hubId={source.computer.id} />}</CardSection>
}
const FALTA_ACESSO: readonly string[] = ['soDireto', 'semRecurso']
function BoundPlaces({ model, hubId }: { model: Workplaces; hubId: string }) {
  useSyncExternalStore(model.subscribe, model.getVersion, model.getVersion)
  useEffect(() => { void model.load() }, [model, model.generation])
  const t = useT()
  return <div className="lugares">
    {model.error && !(!model.available && FALTA_ACESSO.includes(model.error)) && <ErrorNotice text={t(`lugares.erro.${model.error}`)} />}
    {!model.available ? <>
      {/* Falta de acesso ou de recurso não é falha do usuário: explicação calma, sem alerta vermelho. */}
      <p role="status">{t(model.error === 'semRecurso' ? 'lugares.erro.semRecurso' : 'lugares.soDireto')}</p>
      <Button kind="secondary" disabled={model.busy} onClick={() => void model.load()}>{t('lugares.conferirConexao')}</Button>
    </> : <PlacesContent key={model.generation} model={model} hubId={hubId} />}
    {model.busy && <span role="status"><Spinner /> {t('lugares.conferindo')}</span>}
  </div>
}
function ErrorNotice({ text }: { text: string }) {
  return <p className="lugares-erro" role="alert">{text}</p>
}
function PlacesContent({ model, hubId }: { model: Workplaces; hubId: string }) {
  const [selected, setSelected] = useEstadoDaPagina<string | null>(chaveSelecionado(hubId), null)
  const place = model.places.find((p) => p.id === selected)
  const t = useT()
  return <div className="lugares-layout">
    <nav className="lugares-lista" aria-label={t('lugares.seusLugares')}>
      <div className="lugares-padrao"><strong>{t('lugares.nesteServidor')}</strong><span>{t('lugares.padrao')}</span></div>
      {model.places.map((p) => <button key={p.id} className={p.id === selected ? 'selected' : ''} aria-current={p.id === selected ? 'true' : undefined} onClick={() => setSelected(p.id)}><bdi>{p.name}</bdi><span>{t('lugares.boatSemAjudantes')}</span></button>)}
      <p>{SSH_ROTULO} <span>{t('lugares.depois')}</span></p>
      <Button kind="secondary" disabled={model.busy} onClick={() => setSelected(null)}>{t('lugares.cadastrarBoat')}</Button>
      <Button kind="secondary" disabled={model.busy} onClick={() => void model.load()}>{t('lugares.atualizarLista')}</Button>
    </nav>
    <div className="lugares-conteudo">{place ? <PlaceForm key={place.id} model={model} place={place} hubId={hubId} /> : <CadastrarLugar model={model} hubId={hubId} />}</div>
  </div>
}
function PlaceForm({ model, place, hubId }: { model: Workplaces; place: Place; hubId: string }) {
  const t = useT()
  const idioma = useIdioma()
  // O rascunho do lugar fica no pai: a página volta com o que estava sendo editado (estado-pagina.ts).
  const [draft, setDraft] = useEstadoDaPagina(chaveRascunho(hubId, place.id), () => createPlaceDraft(place))
  const { name, pin, manifest, host, port, known, confirmation } = draft
  const setField = (key: 'name' | 'pin' | 'manifest' | 'host' | 'port' | 'known', value: string) => setDraft((previous) => ({ ...previous, [key]: value }))
  const setName = (value: string) => setField('name', value)
  const setPin = (value: string) => setField('pin', value)
  const setManifest = (value: string) => setField('manifest', value)
  const setHost = (value: string) => setField('host', value)
  const setPort = (value: string) => setField('port', value)
  const setKnown = (value: string) => setField('known', value)
  const setConfirmation = (intent: Intent | null) => setDraft((previous) => ({ ...previous, confirmation: intent ? { intent, target: { ...place } } : null }))
  const [publicOpen, setPublicOpen] = useState(false)
  const publicButton = useRef<HTMLButtonElement>(null)
  const [formError, setFormError] = useState(false)
  const pinStatus = useRef<HTMLParagraphElement>(null)
  const connectionStatus = useRef<HTMLParagraphElement>(null)
  const focusTracker = useRef<StepFocus | null>(null)
  const [pendingFocus, setPendingFocus] = useState<{ tracker: StepFocus; expected: FocusScope; step: 'pin' | 'connection' } | null>(null)
  useEffect(() => () => { focusTracker.current?.cancel() }, [])
  useLayoutEffect(() => {
    if (!pendingFocus) return
    pendingFocus.tracker.restore(pendingFocus.step === 'pin' ? pinStatus.current : connectionStatus.current, pendingFocus.expected)
    if (focusTracker.current === pendingFocus.tracker) focusTracker.current = null
    setPendingFocus(null)
  }, [pendingFocus])
  const material = model.material?.placeId === place.id ? model.material : null
  useEffect(() => {
    setDraft((previous) => reconcilePlaceDraft(previous, place, material))
    setFormError(false)
  }, [place.id, place.hubId, place.name, place.revision, place.admissionRevision, place.connectionRevision, material])
  const run = (intent: Intent, target = place) => {
    setFormError(false)
    if (intent.type === 'prepare_connection') setPublicOpen(true)
    const step = intent.type === 'pin_runner' ? 'pin' : intent.type === 'refix' ? 'connection' : null
    const origin = placeFocusScope(target, model.generation)
    const tracker = step ? trackStepFocus(document, origin, () => {
      const current = model.available && model.places.find((item) => item.id === target.id)
      return current ? placeFocusScope(current, model.generation) : null
    }) : null
    if (tracker) { focusTracker.current?.cancel(); focusTracker.current = tracker }
    void model.mutate(intent, target).then((reply) => {
      if (tracker?.active() && step && reply?.place) {
        setPendingFocus({ tracker, step, expected: placeFocusScope(reply.place, origin.generation) })
      } else {
        tracker?.cancel()
        if (focusTracker.current === tracker) focusTracker.current = null
      }
    })
  }
  const pinRunner = () => {
    try {
      const intent: Intent = { type: 'pin_runner', pin: pin.trim(), manifest: decodeManifest(JSON.parse(manifest)), rotate: place.pinned }
      if (place.pinned) setConfirmation(intent); else run(intent)
    } catch { setFormError(true) }
  }
  return <>
    <header><h3><bdi>{place.name}</bdi></h3><p>{t('lugares.boat')} · <code dir="ltr">{place.resourceId}</code> · {t('lugares.naoDisponivelAjudantes')}</p></header>
    {formError && <ErrorNotice text={t('lugares.erroForm')} />}
    <div className="lugares-passos">
      <section className="lugares-passo">
        <h3>{t('lugares.passo1')}</h3>
        <p>{t('lugares.cadastroSalvo')}</p>
        <EditorLugar title={t('lugares.renomearCadastro')}>
          <form className="lugares-form" onSubmit={(e) => { e.preventDefault(); run({ type: 'rename', name: name.trim() }) }}>
            <label>{t('lugares.nome')}<input required dir="auto" maxLength={120} value={name} onChange={(e) => setName(e.target.value)} /></label>
            <button type="submit" className="styled-button secondary" disabled={model.busy || name.trim() === place.name}>{t('lugares.renomear')}</button>
          </form>
        </EditorLugar>
      </section>
      <section className="lugares-passo"><h3>{t('lugares.passo2')}</h3>
        <p>{place.prepared ? t('lugares.chavesGeradas') : t('lugares.gereChaves')}</p>
        <button ref={publicButton} className="styled-button secondary" disabled={model.busy} onClick={() => run({ type: 'prepare_connection', rotate: false })}>{place.prepared ? t('lugares.mostrarChaves') : t('lugares.gerarChaves')}</button>
        {publicOpen && material && <div className="lugares-publicas-abertas">
          <ChavesPublicas material={material} />
          <Button kind="secondary" onClick={() => { setPublicOpen(false); publicButton.current?.focus({ preventScroll: true }) }}>{t('lugares.ocultarChaves')}</Button>
        </div>}
      </section>
      <GuiaPreparo model={model} />
      <section className="lugares-passo lugares-fixar"><h3>{t('lugares.passo4')}</h3>
        <div className="lugares-fixacoes">
          <div>
            <h4>{t('marca.lugares.programa')}</h4>
            <p ref={pinStatus} tabIndex={-1} role="status">{place.pinned ? t('lugares.identidadeFixada') : t('lugares.identidadeNaoFixada')}</p>
            <EditorLugar title={place.pinned ? t('lugares.revisarIdentidade') : t('lugares.informarIdentidade')}>
              <form className="lugares-form" onSubmit={(e) => { e.preventDefault(); pinRunner() }}>
                <p>{t('lugares.identidadeAjuda')}</p>
                <label>{t('lugares.identificacaoPublica')}<input required dir="ltr" value={pin} pattern="[A-Za-z0-9_-]{43}" maxLength={43} onChange={(e) => setPin(e.target.value)} autoComplete="off" spellCheck={false} /></label>
                <label>{t('lugares.manifesto')}<textarea required dir="ltr" value={manifest} onChange={(e) => setManifest(e.target.value)} autoComplete="off" spellCheck={false} maxLength={8192} rows={4} /></label>
                <button type="submit" className="styled-button secondary" disabled={model.busy || !place.prepared}>{place.pinned ? t('lugares.trocarIdentidade') : t('lugares.fixarIdentidade')}</button>
              </form>
            </EditorLugar>
          </div>
          <div>
            <h4>{t('lugares.conexaoMaquina')}</h4>
            <p ref={connectionStatus} tabIndex={-1} role="status">{place.connected ? t('lugares.conexaoFixada') : t('lugares.conexaoNaoFixada')}</p>
            <EditorLugar title={place.connected ? t('lugares.editarConexao') : t('lugares.informarConexao')}>
              <form className="lugares-form" onSubmit={(e) => { e.preventDefault(); run({ type: 'refix', package: { sandbox_id: place.resourceId, host: host.trim(), port: Number(port), known_hosts_line: known } }) }}>
                <p>{t('lugares.conexaoAjuda')}</p>
                <label>{t('lugares.idEsperado')}<input readOnly dir="ltr" value={place.resourceId} /></label>
                <label>{t('lugares.endereco')}<input required dir="ltr" value={host} onChange={(e) => setHost(e.target.value)} autoComplete="off" spellCheck={false} maxLength={253} /></label>
                <label>{t('lugares.porta')}<input required dir="ltr" type="number" min={1} max={65535} value={port} onChange={(e) => setPort(e.target.value)} /></label>
                <label>{t('lugares.chaveServidor')}<input required dir="ltr" value={known} onChange={(e) => setKnown(e.target.value)} autoComplete="off" spellCheck={false} maxLength={300} placeholder={`${place.resourceId} ${CHAVE_EXEMPLO}`} /></label>
                <p>{t('lugares.confiraId')}</p>
                <button type="submit" className="styled-button secondary" disabled={model.busy || !place.pinned}>{place.connected ? t('lugares.refixar') : t('lugares.fixarConexao')}</button>
              </form>
            </EditorLugar>
          </div>
        </div>
      </section>
      <VerificarLugar model={model} place={place} />
    </div>
    <section className="lugares-manutencao" aria-label={t('lugares.quandoPrecisar')}>
      <Button kind="secondary" disabled={model.busy || !place.prepared} onClick={() => setConfirmation({ type: 'prepare_connection', rotate: true })}>{t('lugares.gerarNovas')}</Button>
      <Button kind="secondary" disabled={model.busy} onClick={() => setConfirmation({ type: 'remove' })}>{t('lugares.removerCadastro')}</Button>
    </section>
    <aside className="lugares-aviso">{t('lugares.aviso')}</aside>
    <Dialog open={confirmation !== null} title={confirmation?.intent.type === 'remove' ? t('lugares.removerTitulo') : t('lugares.trocarTitulo')}
      message={confirmation?.intent.type === 'remove' ? t('lugares.removerMensagem') : t('lugares.trocarMensagem')}
      cancel={t('lugares.cancelar')} language={superficieIdioma(idioma)}
      onClose={() => setConfirmation(null)} actions={[{ title: t('lugares.confirmar'), action: () => { if (confirmation) run(confirmation.intent, confirmation.target); setConfirmation(null) } }]} />
  </>
}

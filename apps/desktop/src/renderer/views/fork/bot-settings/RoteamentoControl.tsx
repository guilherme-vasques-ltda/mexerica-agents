import { useState, useSyncExternalStore } from 'react'
import type { RoutingReserve, RoutingView } from '@shared/fork-roteamento'
import { ROUTING_PERSONAL } from '@shared/fork-roteamento'
import { Button, CardSection } from '../../../components/Controls'
import { Dialog } from '../../../components/Overlay'
import type { RoutingStore } from '../../../store/fork-roteamento'
import { superficieIdioma, tradutor, useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import './roteamento.css'

const optionLabel = (t: Tradutor, id: string) => ['reasoning_effort', 'effort', 'thinking'].includes(id) ? t('bots.ajustes.rota.raciocinio') : id
const fixedMessage = (t: Tradutor, view: RoutingView) => {
  switch (view.fixedCheck.reason) {
    case 'catalog_only': return t('bots.ajustes.rota.fixo.catalogo')
    case 'legacy_preserved': return t('bots.ajustes.rota.fixo.legado')
    case 'scope_unproven': return t('bots.ajustes.rota.fixo.escopo')
    default: return t('bots.ajustes.rota.fixo.falhou')
  }
}
export function RoteamentoControl({ model, disabled }: { model: RoutingStore; disabled: boolean }) {
  const state = useSyncExternalStore(model.subscribe, model.getSnapshot)
  const [reloading, setReloading] = useState(false)
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const { view, policy } = state
  if (!view || !policy) return <CardSection><div className="idioma-superficie" {...superficieIdioma(idioma)}><h3>{t('bots.ajustes.rota.titulo')}</h3><p role="status">{state.error ?? t('bots.ajustes.rota.carregando')}</p><Button kind="secondary" disabled={disabled || state.busy} onClick={() => { void model.read() }}>{t('bots.ajustes.rota.tentarNovamente')}</Button></div></CardSection>
  const edit = (patch: Partial<typeof policy>) => model.edit({ ...policy, ...patch })
  const blocked = disabled || state.busy
  const personal = view.spaceId === ROUTING_PERSONAL
  return <CardSection><div className="roteamento-control idioma-superficie" {...superficieIdioma(idioma)}>
    <h3>{t('bots.ajustes.rota.titulo')}</h3>
    <p>{t('bots.ajustes.rota.intro')}</p>
    <fieldset disabled={blocked}>
      <legend>{t('bots.ajustes.rota.preferencia')}</legend>
      <label><input type="radio" name="routing-mode" checked={policy.mode === 'fixed'} onChange={() => edit({ mode: 'fixed' })} /> {t('bots.ajustes.rota.fixo')}</label>
      <label><input type="radio" name="routing-mode" checked={policy.mode === 'auto'} onChange={() => edit({ mode: 'auto' })} /> {t('bots.ajustes.rota.auto')}</label>
    </fieldset>
    <p className="bot-settings-warning" role="note">{t('bots.ajustes.rota.autoIndisponivel')}</p>
    <label>{t('bots.ajustes.rota.guiaGeral')}<textarea value={state.generalGuide} maxLength={2000} disabled={blocked} rows={3} onChange={(e) => model.edit(policy, e.target.value)} /></label>
    <label>{t('bots.ajustes.rota.guiaBot')}<textarea value={policy.guide} maxLength={2000} disabled={blocked} rows={3} onChange={(e) => edit({ guide: e.target.value })} /></label>
    <p>{t('bots.ajustes.rota.guiaDica')}</p>
    <h4>{t('bots.ajustes.rota.reservas')}</h4>
    <p>{t('bots.ajustes.rota.reservasDica')}</p>
    {!personal ? <p role="note">{t('bots.ajustes.rota.semCadastro')}</p> : null}
    <ol>{policy.reserves.map((reserve, index) => <li key={`${reserve.accountId}:${index}`}>
      <span>{view.accounts.find((a) => a.id === reserve.accountId)?.label ?? t('bots.ajustes.rota.contaIndisponivel')} · {reserve.selection.backend} · {reserve.selection.model}{Object.entries(reserve.selection.modelOptions).map(([id, value]) => ` · ${optionLabel(t, id)}: ${value}`).join('')}</span>
      <Button kind="secondary" disabled={blocked || index === 0} onClick={() => { const next = [...policy.reserves]; [next[index - 1], next[index]] = [next[index]!, next[index - 1]!]; edit({ reserves: next }) }}>{t('bots.ajustes.rota.subir', { n: index + 1 })}</Button>
      <Button kind="secondary" disabled={blocked || index === policy.reserves.length - 1} onClick={() => { const next = [...policy.reserves]; [next[index], next[index + 1]] = [next[index + 1]!, next[index]!]; edit({ reserves: next }) }}>{t('bots.ajustes.rota.descer', { n: index + 1 })}</Button>
      <Button kind="secondary" disabled={blocked} onClick={() => edit({ reserves: policy.reserves.filter((_, i) => i !== index) })}>{t('bots.ajustes.rota.remover', { n: index + 1 })}</Button>
    </li>)}</ol>
    {!policy.reserves.length ? <p>{t('bots.ajustes.rota.semReservas')}</p> : null}
    {personal ? <Reserva key={view.policy.revision} view={view} disabled={blocked || policy.reserves.length >= 8} add={(reserve) => edit({ reserves: [...policy.reserves, reserve] })} /> : null}
    <p role="status">{disabled || !state.fixedReady ? t('bots.ajustes.rota.aguardeFixo') : state.fixedCurrent ? fixedMessage(t, view) : t('bots.ajustes.rota.fixoMudou')}</p>
    <div className="roteamento-actions">
      <Button kind="secondary" disabled={blocked || !state.fixedReady} onClick={() => { void model.read(true) }}>{t('bots.ajustes.rota.conferir')}</Button>
      <Button disabled={blocked || !state.dirty} onClick={() => { void model.save() }}>{t('bots.ajustes.rota.salvar')}</Button>
      <Button kind="secondary" disabled={blocked} onClick={() => setReloading(true)}>{t('bots.ajustes.rota.recarregar')}</Button>
    </div>
    <p role={state.error ? 'alert' : 'status'}>{state.error ?? state.message ?? (state.busy ? t('bots.ajustes.rota.aguarde') : state.dirty ? t('bots.ajustes.rota.naoSalvas') : t('bots.ajustes.rota.guardadas'))}</p>
    <p>{t('bots.ajustes.rota.hubLe')}</p>
    <Dialog open={reloading} title={t('bots.ajustes.rota.recarregarTitulo')} message={t('bots.ajustes.rota.recarregarMensagem')} cancel={t('bots.ajustes.rota.continuar')} language={superficieIdioma(idioma)} onClose={() => setReloading(false)} actions={[{ title: t('bots.ajustes.rota.recarregarAcao'), action: () => { setReloading(false); void model.read(false, true) } }]} />
  </div></CardSection>
}
function Reserva({ view, disabled, add }: { view: RoutingView; disabled: boolean; add: (reserve: RoutingReserve) => void }) {
  const [id, setId] = useState('')
  const [model, setModel] = useState('')
  const [options, setOptions] = useState<Record<string, string>>({})
  const t = useT()
  const account = view.accounts.find((a) => a.id === id)
  const chosen = account?.models.find((m) => m.id === model)
  return <fieldset disabled={disabled} className="roteamento-reserva"><legend>{t('bots.ajustes.rota.adicionarReserva')}</legend>
    <label>{t('bots.ajustes.rota.conta')}<select value={id} onChange={(e) => { setId(e.target.value); setModel(''); setOptions({}) }}><option value="">{t('bots.ajustes.rota.escolherConta')}</option>{view.accounts.filter((a) => a.state === 'connected').map((a) => <option key={a.id} value={a.id}>{a.label}</option>)}</select></label>
    <label>{t('bots.editor.modelo.rotulo')}<select value={model} onChange={(e) => { setModel(e.target.value); setOptions({}) }}><option value="">{t('bots.ajustes.rota.escolherModelo')}</option>{account?.models.map((m) => <option key={m.id} value={m.id}>{m.id}</option>)}</select></label>
    {chosen?.options.map((o) => <label key={o.id}>{optionLabel(t, o.id)}<select value={options[o.id] ?? ''} onChange={(e) => { const next = { ...options }; if (e.target.value) next[o.id] = e.target.value; else delete next[o.id]; setOptions(next) }}><option value="">{t('bots.editor.modelo.padrao')}</option>{o.values.map((v) => <option key={v} value={v}>{v}</option>)}</select></label>)}
    <Button kind="secondary" disabled={!account || !chosen || disabled} onClick={() => { if (account && chosen) { add({ accountId: account.id, accountRevision: account.revision, selection: { backend: account.backend, model, modelOptions: { ...options } } }); setId(''); setModel(''); setOptions({}) } }}>{t('bots.ajustes.rota.adicionarLista')}</Button>
  </fieldset>
}

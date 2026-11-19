import { useEffect, useState } from 'react'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { ChaveTexto } from '../../../lib/fork-i18n'
import type { Engine, ManagedAccount } from '@shared/fork-engine-accounts'
import { birthAccounts, birthDefault, SpaceBirthIntent } from '@shared/fork-space-birth'
import { defaultNewBotBackend } from '@shared/new-bot-defaults'
import { Button } from '../../../components/Controls'
import { settingsScreen } from '../ajustes/tela'
import { engineSetup } from '../../../store/fork-engine-setup'
import type { SpacesOrigin } from '../../../store/fork-spaces'
import type { SpaceDraft } from './SpaceIconPicker'
import { SpaceTile } from './SpaceTile'

type BirthText = Exclude<Extract<ChaveTexto, `spaces.birth.${string}`>, 'spaces.birth.padrao'>

/** D543: a folha conserva a origem e a intenção; não lê perfil nem preferências do principal pessoal. */
export function SpaceBirth({ origin, draft, onBack, onClose }: { origin: SpacesOrigin; draft: SpaceDraft; onBack: () => void; onClose: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  const [intent] = useState(() => new SpaceBirthIntent(origin.store, origin.hostId, origin.client, origin.epoch, origin.snapshot.personalSpaceId, origin.snapshot.revision, crypto.randomUUID()))
  // D543 E1: o nome do principal é fixo ("Berga") no Hub; a folha só mostra, não edita.
  const name = 'Berga'
  const [backend, setBackend] = useState<Engine>(() => defaultNewBotBackend(origin.store.hello?.backends ?? []) === 'claude' ? 'claude' : 'codex')
  const [accounts, setAccounts] = useState<ManagedAccount[]>([])
  const [state, setState] = useState<'loading' | 'ready' | 'error'>('loading')
  const [accountId, setAccountId] = useState<string | null>(null)
  const [confirmed, setConfirmed] = useState(false)
  const [saving, setSaving] = useState(false)
  const [fixed, setFixed] = useState(false)
  const [problem, setProblem] = useState<BirthText | null>(null)
  const supported = origin.store.hello?.spaceBirthContract === 1
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
  const clean = name.trim()
  const validName = !!clean && [...clean].length <= 40 && !/[\u0000-\u001f\u007f]/u.test(clean)
  const ready = supported && state === 'ready' && validName && !!account && confirmed
  const save = async () => {
    if ((!ready && !fixed) || saving) return
    setSaving(true)
    setProblem(null)
    try {
      const request = intent.send({ ...draft, principal: { name: clean, backend, engineAccountId: selected } })
      setFixed(intent.fixed)
      const snapshot = await request
      // A intenção já conferiu o link no retorno. Uma lista mais nova do stream não deve regredir.
      origin.store.acceptSpaces(snapshot, origin)
      onClose()
    } catch (error) {
      const code = error instanceof Error ? error.message : ''
      const key: BirthText = /space_birth_context_changed/.test(code) ? 'spaces.birth.conexaoMudou'
        : /space_birth_unsupported/.test(code) ? 'spaces.birth.atualize'
          : /space_birth_inflight/.test(code) ? 'spaces.birth.emAndamento'
            : /space_birth_revision_changed|revision_conflict/.test(code) ? 'spaces.birth.listaMudou'
              : /not_allowed|forbidden|remote_setup_unavailable/.test(code) ? 'spaces.birth.permissao'
                : /account|engine_selection/.test(code) ? 'spaces.birth.contaIndisponivel'
                  : /invalid_field/.test(code) ? 'spaces.birth.nomeInvalido' : 'spaces.birth.incerto'
      setProblem(key)
    } finally { setSaving(false) }
  }
  const unavailable: BirthText | null = !supported ? 'spaces.birth.atualize'
    : state === 'loading' ? 'spaces.birth.carregando'
      : state === 'error' ? 'spaces.birth.consultaFalhou'
        : !options.length ? 'spaces.birth.semConta' : null
  return <section className="space-birth idioma-superficie" aria-label={t('spaces.birth.titulo')} {...superficieIdioma(idioma)}>
    <div className="space-birth-heading"> <SpaceTile tile={draft.tile} size={48} /><div><h2>{t('spaces.birth.titulo')}</h2><p dir="auto">{draft.name}</p></div></div>
    <p>{t('spaces.birth.separado')}</p>
    <fieldset disabled={saving || fixed}>
      <label>{t('spaces.birth.nome')}<span dir="auto">{name}</span></label>
      <label>{t('spaces.birth.motor')}<select value={backend} onChange={(e) => { setBackend(e.target.value as Engine); setAccountId(null); setConfirmed(false) }}>{(['codex', 'claude'] as const).map((id) => <option key={id} value={id}>{engineName(id)}</option>)}</select></label>
      <label>{t('spaces.birth.conta')}<select value={selected} onChange={(e) => { setAccountId(e.target.value); setConfirmed(false) }} disabled={state !== 'ready'}>
        <option value="">{t('spaces.birth.escolha')}</option>
        {options.map((a) => <option key={a.id} value={a.id}>{a.origin === 'legacy' ? t('spaces.birth.padrao', { conta: a.label }) : a.label}</option>)}
      </select></label>
      <label className="space-birth-confirm"><input type="checkbox" checked={confirmed} disabled={!account} onChange={(e) => setConfirmed(e.target.checked)} />{t('spaces.birth.confirmo')}</label>
    </fieldset>
    {unavailable ? <p role="status">{t(unavailable)}</p> : null}
    {unavailable === 'spaces.birth.semConta' ? <Button kind="secondary" onClick={() => settingsScreen.show('ai-accounts', 'contas')}>{t('conversa.avisos.falha.acao.abrirContas')}</Button> : null}
    {!validName ? <p role="status">{t('spaces.birth.nomeDica')}</p> : null}
    {problem ? <p role="alert">{t(problem)}</p> : null}
    <footer><Button kind="secondary" disabled={saving || fixed} onClick={onBack}>{t('spaces.birth.voltar')}</Button><Button kind="secondary" disabled={saving} onClick={onClose}>{t('spaces.birth.cancelar')}</Button><Button disabled={saving || (!fixed && !ready)} onClick={() => void save()}>{t(saving ? 'spaces.birth.criando' : fixed ? 'spaces.birth.tentar' : 'spaces.birth.criar')}</Button></footer>
  </section>
}

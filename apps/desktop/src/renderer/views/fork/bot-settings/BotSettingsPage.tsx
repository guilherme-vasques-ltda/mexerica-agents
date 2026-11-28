import { useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState, useSyncExternalStore, type Ref } from 'react'
import { draftOf, isGroup, type BotDraft } from '@shared/models'
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import { Button, IconButton } from '../../../components/Controls'
import { Dialog, Sheet, useEscape } from '../../../components/Overlay'
import { MemoryCard } from '../../bots/MemoryView'
import { BotTemplateView } from '../../bots/BotTemplateView'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import { useModel } from '../../../lib/observable'
import { useStore } from '../../../store/context'
import { IdentitySection } from './IdentitySection'
import { DonnaPreferencias } from './DonnaPreferencias'
import { BotVoiceSection } from './BotVoiceSection'
import { botSettingsSignature, createBotSettingsStore } from './bot-settings-store'
import { captureSkillChoice, saveBotSettingsIntent, type BotSettingsIntent } from '../../../store/fork-bot-settings-intents'
import { errorText, type InstalledSkill } from '../../bots/drafts'
import { CurrentBehaviourSettings, CurrentEngineSettings, CurrentNotificationSettings, CurrentToolsSettings, type SettingsControl, type SettingsErrors } from './CurrentSettingsSections'
import type { BotSettingsHandle, BotSettingsSection } from './bot-settings-entry'
import './bot-settings.css'
import { abaAlterada, abaDaIntencao, abaPeloTeclado, abasComErro } from './abas'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { comNos, no } from '../../settings/rotulo'
import { mensagemDoErro, textoDaMensagem } from './mensagem'
import { SecaoWhatsapp } from '../whatsapp/SecaoWhatsapp'
import { useWhatsapp } from '../whatsapp/partes'
import { pluginLigado, secaoValida, secoesVisiveis } from '../whatsapp/secao'
import { ComportamentoDaOpen } from './fork-open-comportamento'
import { AvisoDoFluxo } from './fork-open-fluxo'
import { MotorDaOpen } from './fork-open-motor'

const allSections = [
  ['identity', 'botSettings.abas.identity'], ['engine', 'botSettings.abas.engine'], ['behaviour', 'botSettings.abas.behaviour'], ['tools', 'botSettings.abas.tools'], ['whatsapp', 'botSettings.abas.whatsapp'], ['memory', 'botSettings.abas.memory'],
  ['voice', 'botSettings.abas.voice'], ['notifications', 'botSettings.abas.notifications'], ['routines', 'botSettings.abas.routines'], ['export', 'botSettings.abas.export'], ['danger', 'botSettings.abas.danger'],
] as const
type Section = typeof allSections[number][0]

export function BotSettingsPage({ botId, initialSection, onClose, onConversation, ref }: { botId: string; initialSection?: BotSettingsSection; onClose: () => void; onConversation: () => void; ref?: Ref<BotSettingsHandle> }) {
  const t = useT()
  const open = window.codync.appScheme === 'mexerica-open'
  const store = useModel(useStore())
  const principal = useModel(store.principal)
  const model = useMemo(() => createBotSettingsStore(store), [store])
  const state = useSyncExternalStore(model.subscribe, model.getSnapshot)
  const [section, setSectionState] = useState<Section>(initialSection ?? 'identity')
  // D350: o item e a seção WhatsApp só com o plugin ligado em Ajustes → Plugins (e somem ao desligar).
  const whatsapp = useWhatsapp(store, !open)
  const whatsappKnown = whatsapp.leitura !== null || whatsapp.erroLeitura !== null
  const sections = open ? allSections : secoesVisiveis(allSections, pluginLigado(whatsapp.leitura))
  const setSection = (next: Section) => setSectionState(secaoValida(next, sections))
  useEffect(() => { if (secaoValida(section, sections) !== section) setSectionState(sections[0]![0]) }, [section, sections.length])
  const [leaving, setLeaving] = useState<(() => void) | null>(null)
  const [reloading, setReloading] = useState(false)
  // Barra05: Memória e Compartilhar saíram do painel do bot e ficam aqui.
  const [sharing, setSharing] = useState<BotDraft | null>(null)
  const content = useRef<HTMLDivElement>(null)
  const intentBusy = useRef(false)
  const editorSavingSection = useRef<Section>('identity')
  const [savingSection, setSavingSection] = useState<Section | null>(null)
  const intentSaving = savingSection !== null
  const [intentErrors, setIntentErrors] = useState<SettingsErrors>({})
  const [pendingControls, setPendingControls] = useState<Partial<Record<SettingsControl, boolean>>>({})
  const draftChanged = useCallback((control: SettingsControl, pending: boolean) => {
    setPendingControls((current) => current[control] === pending ? current : { ...current, [control]: pending })
  }, [])
  const online = store.connection.kind === 'online' && !store.mismatch
  const bot = store.bots.get(botId)
  const signature = botSettingsSignature(bot, principal.id)
  useEffect(() => { model.setOnline(online); void model.open(botId); return () => model.pause() }, [model, botId])
  useEffect(() => { model.setOnline(online) }, [model, online])
  useEffect(() => { void model.refresh() }, [model, signature])
  useEffect(() => {
    setIntentErrors({})
    let saving = model.getSnapshot().saving
    return model.subscribe(() => {
      const current = model.getSnapshot()
      if (current.saving && !saving) editorSavingSection.current = abaAlterada('identity', current.draft, current.editor?.profile ?? null) ? 'identity' : abaAlterada('behaviour', current.draft, current.editor?.profile ?? null) ? 'behaviour' : 'engine'
      saving = current.saving
    })
  }, [model, botId])

  const leave = async (action: () => void) => {
    if (!await model.flush()) { setLeaving(() => action); return }
    action()
  }
  useImperativeHandle(ref, () => ({ leave: (action) => { void leave(action) } }))
  useEscape(true, () => { void leave(onClose) }, content)

  // Primeiro confirma a identidade; os controles existentes enviam só sua própria intenção.
  const clearIntentError = (control: SettingsControl) => setIntentErrors((errors) => {
    if (!errors[control]) return errors
    const { [control]: _, ...rest } = errors
    return rest
  })
  const feedback = { errors: intentErrors, clearError: clearIntentError, draftChanged }
  const saveIntent = async (control: SettingsControl, action: () => Promise<unknown>) => {
    clearIntentError(control)
    if (intentBusy.current || !online || !state.editor?.permissions.canEdit) return
    intentBusy.current = true
    setSavingSection(abaDaIntencao(control))
    try {
      if (!await model.flush()) { setIntentErrors((errors) => ({ ...errors, [control]: { chave: 'botSettings.estado.identidadeAntes' } })); return }
      await action()
      await model.refresh()
    } catch (error) { setIntentErrors((errors) => ({ ...errors, [control]: mensagemDoErro(error, errorText) })) }
    finally { intentBusy.current = false; setSavingSection(null) }
  }
  const save = (control: SettingsControl, patch: BotSettingsIntent) => { void saveIntent(control, () => saveBotSettingsIntent(store, botId, patch)) }
  // c1: versão, instalação e origem presas ao gesto AQUI, antes do flush de identidade em saveIntent.
  const saveSkill = (skill: InstalledSkill, enabled: boolean) => {
    if (intentBusy.current) return
    const current = store.bots.get(botId)
    const choice = current ? captureSkillChoice(store, current, skill, enabled) : { refused: { chave: 'botSettings.estado.naoEncontrado' } as const }
    if ('refused' in choice) { setIntentErrors((errors) => ({ ...errors, skills: choice.refused })); return }
    void saveIntent('skills', choice.send)
  }

  const navigate = (next: Section) => {
    setSection(next)
    // A troca começa no topo do painel; nunca procura outra seção por rolagem.
    if (content.current) content.current.scrollTop = 0
  }
  // Opened on a section (the shield opens Comportamento, D169): go there once the sections exist.
  const openedOn = useRef(false)
  useEffect(() => {
    // Aberto na seção WhatsApp: espera a leitura do plugin; desligado, fica na primeira.
    if (openedOn.current || !initialSection || !state.editor || (initialSection === 'whatsapp' && !whatsappKnown)) return
    openedOn.current = true
    navigate(secaoValida(initialSection, sections))
  }, [state.editor, whatsappKnown])
  const name = state.draft?.name ?? bot?.name ?? t('botSettings.pagina.bot')
  const editorDirty = abaAlterada(section, state.draft, state.editor?.profile ?? null)
  const dirty = editorDirty || Object.entries(pendingControls).some(([control, pending]) => pending && abaDaIntencao(control as SettingsControl) === section)
  const hasIntentErrors = Object.keys(intentErrors).some((control) => abaDaIntencao(control as SettingsControl) === section)
  const errorSections = abasComErro(state.error !== null || state.conflict, editorSavingSection.current, state.draft, state.editor?.profile ?? null, intentErrors)
  // R1 do #572: o estado guarda a chave; o texto sai aqui, no idioma atual.
  const sectionError = (!state.editor || editorDirty || section === editorSavingSection.current) && state.error ? textoDaMensagem(t, { chave: state.error }) : null
  const status = !online ? t('botSettings.estado.offline') : sectionError ?? (hasIntentErrors ? t('botSettings.estado.recusada') : state.loading && !state.editor ? t('botSettings.estado.carregando') : (state.saving && (editorDirty || section === editorSavingSection.current)) || savingSection === section ? t('botSettings.estado.salvando') : dirty ? t('botSettings.estado.pendente') : errorSections.size ? t('botSettings.estado.outraAba') : state.editor && ['identity', 'engine', 'behaviour', 'tools', 'notifications'].includes(section) ? t('botSettings.estado.salvo') : '')
  // Auditoria D249, F4: a função do bot no lugar do texto técnico; a principal vem da identidade (D121).
  const subtitle = principal.id === botId ? t('botSettings.pagina.principal') : (state.draft?.title ?? bot?.title)?.trim() || ''
  const workspaceIssue = open ? null : state.editor?.workspaceIssue ?? bot?.workspaceIssue
  const disabled = !online || !state.editor?.permissions.canEdit || intentSaving
  // D366/D368: o Hub recusa; a tela só reflete. Motor, ferramentas e preferências: só o dono, por aqui.
  const fixed = principal.id === botId
  // D366/D368: textos do Berga fixo já pela base de Idiomas (botSettings.donnaFixa.*).
  const idioma = superficieIdioma(useIdioma())

  return <div className="bot-settings-page">
    <header className="bot-settings-header drag" {...idioma}>
      <div className="bot-settings-heading no-drag">
        <DonnabotAvatar config={normalizarAvatarDoBot(avatarDoBot(state.draft ?? bot), name, state.draft?.description)} size={52} label={name} animated={false} />
        <div><h1>{comNos(t('botSettings.pagina.titulo', { nome: no('nome') }), { nome: <bdi dir="auto">{name}</bdi> })}</h1>{subtitle ? <p>{subtitle}</p> : null}</div>
      </div>
      <div className="bot-settings-actions no-drag">
        <Button onClick={() => { void leave(onConversation) }}>{t('botSettings.pagina.conversa')}</Button>
        <IconButton title={t('botSettings.pagina.fechar')} icon="xmark" onClick={() => { void leave(onClose) }} />
      </div>
    </header>
    {!open ? <div className="bot-settings-chips" {...idioma} aria-label={t('botSettings.pagina.resumo')}>
      {bot ? <>
        <button title={t('botSettings.pagina.modelo', { modelo: bot.model || store.backendName(bot.backend) })} onClick={() => navigate('engine')}>{bot.model || store.backendName(bot.backend)}</button>
        <button title={t('botSettings.pagina.autonomia', { modo: bot.permission === 'auto' ? t('botSettings.pagina.liberar') : t('botSettings.pagina.pedir') })} onClick={() => navigate('behaviour')}>{bot.permission === 'auto' ? t('botSettings.pagina.liberar') : t('botSettings.pagina.pedir')}</button>
        <button title={t('botSettings.pagina.skills', { n: bot.skills.length })} onClick={() => navigate('tools')}>{t('botSettings.pagina.skills', { n: bot.skills.length })}</button>
      </> : null}
    </div> : null}
    <div className={`bot-settings-status ${errorSections.size || !online ? 'attention' : ''}`} role="status" aria-live="polite" {...idioma}>
      <span>{status}</span>
      {sectionError && !state.conflict || state.paused && online && editorDirty ? <button onClick={() => { void model.retry() }}>{t('botSettings.pagina.retry')}</button> : null}
      {state.conflict && editorDirty ? <>
        <button onClick={() => setReloading(true)}>{t('botSettings.pagina.reload')}</button>
        <button onClick={() => { requestAnimationFrame(() => content.current?.querySelector<HTMLElement>('[role=tabpanel]:not([hidden]) input:not(:disabled), [role=tabpanel]:not([hidden]) textarea:not(:disabled)')?.focus({ preventScroll: true })) }}>{t('botSettings.pagina.editar')}</button>
      </> : null}
    </div>
    {(section === 'identity' || section === 'engine') && state.editor?.application === 'apply_pending' ? <p className="bot-settings-warning">{t('botSettings.pagina.aplicacao')}</p> : null}
    {workspaceIssue ? <p className="bot-settings-warning">{workspaceIssue} <button onClick={() => navigate('behaviour')}>{t('botSettings.pagina.pasta')}</button></p> : null}
    {fixed && !open ? <p className="bot-settings-warning" {...idioma}>{t('botSettings.donnaFixa.aviso')}</p> : null}
    {state.editor && !state.editor.permissions.canEdit ? <p className="bot-settings-warning">{t('botSettings.pagina.semPermissao')}</p> : null}
    <div className="bot-settings-layout">
      <nav className="bot-settings-nav" {...idioma} role="tablist" aria-orientation="vertical" aria-label={t('botSettings.abas.rotulo')}>
        {sections.map(([id, label]) => <button key={id} type="button" role="tab" id={`bot-settings-tab-${id}`} aria-controls={`bot-settings-panel-${id}`} aria-selected={id === section} tabIndex={id === section ? 0 : -1}
          onClick={() => navigate(id)} onKeyDown={(e) => {
            const next = abaPeloTeclado(sections.map(([key]) => key), id, e.key)
            if (!next) return
            e.preventDefault()
            navigate(next)
            document.getElementById(`bot-settings-tab-${next}`)?.focus({ preventScroll: true })
          }}>{t(label)}{errorSections.has(id) ? <span className="bot-settings-tab-error" role="img" aria-label={t('botSettings.abas.erro', { aba: t(label) })} /> : null}</button>)}
      </nav>
      <label className="bot-settings-mobile" {...idioma}>{t('botSettings.abas.rotulo')}<select value={section} onChange={(e) => navigate(e.target.value as Section)}>
        {sections.map(([id, label]) => <option key={id} value={id}>{errorSections.has(id) ? t('botSettings.abas.comErro', { aba: t(label) }) : t(label)}</option>)}
      </select></label>
      <div className="bot-settings-content" ref={content}>
        {bot && isGroup(bot) ? <p>{t('botSettings.pagina.canal')}</p> : <>
          <div role="tabpanel" id="bot-settings-panel-identity" aria-labelledby="bot-settings-tab-identity" hidden={section !== 'identity'} tabIndex={0}><IdentitySection state={state} model={model} blocked={intentSaving} fixed={fixed}
            preferences={fixed && bot && !open ? <DonnaPreferencias bot={bot} save={save} feedback={feedback} disabled={disabled} /> : null} /></div>
          {sections.slice(1).map(([id, label]) => <section key={id} role="tabpanel" id={`bot-settings-panel-${id}`} aria-labelledby={`bot-settings-tab-${id}`} hidden={section !== id} tabIndex={0} className="bot-settings-placeholder">
            <h2>{t(label)}</h2>
            {bot && open && id === 'engine' ? <MotorDaOpen bot={bot} disabled={disabled} />
              : bot && open && id === 'behaviour' ? <fieldset disabled={disabled} className="bot-settings-controls"><ComportamentoDaOpen bot={bot} permission={(permission) => { void saveIntent('permission', () => store.setPermission(botId, permission)) }} onFerramentas={() => setSection('tools')} /></fieldset>
              : bot && open && id === 'tools' ? <><AvisoDoFluxo fluxo="conectores" /><fieldset className="bot-settings-controls"><CurrentToolsSettings bot={bot} save={save} saveSkill={saveSkill} feedback={feedback} disabled={disabled} /></fieldset></>
              : open && id === 'whatsapp' ? <AvisoDoFluxo fluxo="whatsapp" />
              : open && id === 'voice' ? <AvisoDoFluxo fluxo="voz" />
              : bot && ['engine', 'behaviour', 'tools', 'notifications'].includes(id) ? <fieldset disabled={id === 'tools' ? false : disabled} className="bot-settings-controls">
              {id === 'engine' ? <CurrentEngineSettings bot={bot} save={save} feedback={feedback} accounts={state.editor ? { editor: state.editor, draft: state.draft, model, disabled } : undefined} /> : null}
              {id === 'behaviour' ? <CurrentBehaviourSettings bot={bot} save={save} feedback={feedback} busy={disabled} navegador={{ editor: state.editor, draft: state.draft, model }} permission={(permission) => { void saveIntent('permission', () => store.setPermission(botId, permission)) }} onFerramentas={() => setSection('tools')} /> : null}
              {id === 'tools' ? <CurrentToolsSettings bot={bot} save={save} saveSkill={saveSkill} feedback={feedback} disabled={disabled} /> : null}
              {id === 'notifications' ? <CurrentNotificationSettings bot={bot} save={save} feedback={feedback} /> : null}
            </fieldset> : id === 'whatsapp' ? <SecaoWhatsapp store={store} botId={botId} nome={name} disabled={disabled} />
              : id === 'voice' ? section === id ? <BotVoiceSection store={store} /> : null
              : fixed && id === 'danger' ? <p {...idioma}>{t('botSettings.donnaFixa.perigo')}</p>
              : bot && id === 'memory' ? <>{open ? <p {...idioma}>{t('botSettings.open.memoria.aviso')}</p> : null}<MemoryCard botId={botId} /></>
              : bot && id === 'export' ? <Button onClick={() => setSharing(draftOf(bot))}>{t('botSettings.pagina.compartilhar')}</Button>
              : <p>{t('botSettings.pagina.futura')}</p>}
          </section>)}
        </>}
      </div>
    </div>
    <Dialog language={idioma} open={leaving !== null} title={t('botSettings.estado.pendente')} message={t('botSettings.pagina.sairAjuda')}
      cancel={t('botSettings.pagina.editar')} onClose={() => setLeaving(null)} actions={[{ title: t('botSettings.pagina.descartar'), destructive: true, action: () => { const action = leaving; void model.discard().then(() => action?.()) } }]} />
    <Sheet language={idioma} open={sharing !== null} onClose={() => setSharing(null)}>
      {sharing ? <BotTemplateView draft={sharing} /> : null}
    </Sheet>
    <Dialog language={idioma} open={reloading} title={t('botSettings.pagina.recarregarTitulo')} message={t('botSettings.pagina.recarregarAjuda')}
      cancel={t('botSettings.pagina.editar')} onClose={() => setReloading(false)} actions={[{ title: t('botSettings.pagina.reload'), action: () => { void model.discard() } }]} />
  </div>
}

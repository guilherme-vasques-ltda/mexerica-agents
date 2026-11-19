import { useEffect, useState } from 'react'
import { fastModeRefusal, fastModeShown } from '@shared/fast-mode'
import type { Bot } from '@shared/models'
import type { BotEditor, BotEditorProfile } from '@shared/fork-bot-editor'
import { CardSection, ChoicePicker, DropdownMenu, Pill, ValueRow } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { Dialog, Sheet } from '../../../components/Overlay'
import { Text } from '../../../components/Text'
import { useStore } from '../../../store/context'
import { engineSettingsIntent, type BotSettingsIntent } from '../../../store/fork-bot-settings-intents'
import { lastPathComponent, type InstalledSkill } from '../../bots/drafts'
import { SwitchRow } from '../../bots/parts'
import { FolderPicker } from '../../bots/BotEditorView'
import type { BotSettingsStore } from './bot-settings-store'
import { ContaDoMotor } from './ContaDoMotor'
import { MotorIndisponiveis } from './MotorIndisponiveis'
import { FerramentasEmAbas } from '../bot-ferramentas/FerramentasEmAbas'
import { AvisoDoMotor } from '../aviso-motor/AvisoDoMotor'
import { avisoDoMotor } from '../aviso-motor/aviso-motor'
import { UsarNaPrincipal } from '../motor-principal/UsarNaPrincipal'
import { accountsEditable, applyEnginePatch, selectedAccount } from './engine-account'
import { chaveDoSeletor, gravacoesDaEscolha, provedoresDoEditor } from './seletor-editor'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { textoDaMensagem, type Mensagem } from './mensagem'
import { carregarModelos, provedoresDoHub, useCatalogoMudou } from '../seletor-modelo/catalogo'
import type { Escolha } from '../seletor-modelo/regras'
import { SeletorModelo } from '../seletor-modelo/SeletorModelo'
import { TelaPorComputador } from '../computador/TelaPorComputador'
import { UsarOComputador } from '../computador/UsarOComputador'
import { podeMudarComputador } from '../computador/estado'
import { NavegadorDoBot } from './NavegadorDoBot'
import { NuncaFazerBot } from '../nunca-fazer/NuncaFazer'

export type SettingsControl = keyof BotSettingsIntent | 'permission' | 'skills'
/** Erro de cada controle como chave do catálogo (ou texto pronto do Hub); traduzido ao mostrar (R1 do #572). */
export type SettingsErrors = Partial<Record<SettingsControl, Mensagem>>
export type SaveSettings = (control: SettingsControl, patch: BotSettingsIntent) => void
/** c1: uma célula de skill, capturada no gesto (antes de qualquer flush). */
export type SaveSkill = (skill: InstalledSkill, enabled: boolean) => void
export interface SettingsFeedback {
  errors: SettingsErrors
  clearError: (control: SettingsControl) => void
  draftChanged?: (control: SettingsControl, pending: boolean) => void
}
function ControlError({ feedback, control }: { feedback: SettingsFeedback; control: SettingsControl }) {
  const t = useT()
  const erro = feedback.errors[control]
  return erro ? <p className="bot-settings-control-error" role="alert">{textoDaMensagem(t, erro)}</p> : null
}

/** D164: conta do motor pelo editor versionado; ausente em Hub antigo. */
export interface EngineAccountsControl {
  editor: BotEditor
  draft: BotEditorProfile | null
  model: BotSettingsStore
  disabled: boolean
}

export function CurrentEngineSettings({ bot, save, feedback, accounts }: { bot: Bot; save: SaveSettings; feedback: SettingsFeedback; accounts?: EngineAccountsControl }) {
  const store = useStore()
  const [backend, setBackend] = useState(bot.backend)
  const [command, setCommand] = useState(bot.command ?? '')
  const [pendingMotor, setPendingMotor] = useState<string | null>(null)
  const [changingMotor, setChangingMotor] = useState(false)
  useEffect(() => setCommand(bot.command ?? ''), [bot.id, bot.command])
  useEffect(() => { feedback.draftChanged?.('command', command !== (bot.command ?? '')) }, [command, bot.command, feedback.draftChanged])
  const editor = accounts && accountsEditable(accounts.editor) ? accounts.editor : null
  const savedBackend = editor?.profile.backend ?? bot.backend
  useEffect(() => setBackend(savedBackend), [bot.id, savedBackend])
  const withAccount = !!editor && selectedAccount(accounts?.draft ?? editor.profile) !== null
  // O seletor da C9 no modo padrão do bot: só o motor salvo; com conta, os modelos que ela comprovou.
  const t = useT()
  const idioma = useIdioma()
  useCatalogoMudou()
  const perfil = accounts?.draft ?? editor?.profile ?? null
  const conta = withAccount && editor ? editor.engineAccounts.find((a) => a.id === selectedAccount(perfil)) ?? null : null
  // Com conta sem lista comprovada (ou fora deste Hub), o editor mostra a nota da conta (ContaDoMotor), sem seletor.
  const listaDaConta = !withAccount || (conta?.models.length ?? 0) > 0
  const contaSemPosse = withAccount && !store.canManageEngineAccounts
  const provedoresSeletor = provedoresDoEditor(provedoresDoHub(store), backend, conta)
  const semSeletor = provedoresSeletor.length === 0
  const velocidadeNoSeletor = !semSeletor && listaDaConta && provedoresSeletor[0]?.fastMode === true
  const valorSeletor: Escolha = {
    accountId: withAccount ? selectedAccount(perfil) : null, backend,
    model: (withAccount ? perfil?.model : bot.model) ?? null,
    modelOptions: (withAccount ? perfil?.modelOptions : bot.modelOptions) ?? {},
    fastMode: bot.fastMode === true,
  }
  const escolherModelo = (nova: Escolha) => {
    for (const g of gravacoesDaEscolha(valorSeletor, nova, withAccount)) {
      if (g.via === 'editor') {
        const latest = accounts?.model.getSnapshot().editor
        if (latest && accounts) applyEnginePatch(accounts.model, latest, g.patch)
      } else {
        feedback.clearError(g.control)
        save(g.control, g.patch)
      }
    }
  }
  const changeMotor = async (target: string) => {
    if (!editor || !accounts || changingMotor || accounts.disabled) return
    // A pessoa confirmou: a conta sai junto com o motor (o modelo volta ao padrão).
    setChangingMotor(true)
    try {
      if (await accounts.model.saveMotor(target === 'custom' ? null : target)) setBackend(target)
    } finally { setChangingMotor(false) }
  }
  return <><CardSection>
    <ValueRow label={t('botSettings.motor.rotulo')}>
      <ChoicePicker language={superficieIdioma(idioma)} selection={backend} disabled={changingMotor || accounts?.disabled} options={[
        ...(store.hello?.backends ?? []).map((b) => ({ id: b.id, label: b.available ? b.name : t('botSettings.motor.naoInstalado', { nome: b.name }) })),
        { id: 'custom', label: t('botSettings.motor.comando') },
      ]} onChange={(backend) => {
        if (withAccount && backend !== savedBackend) { setPendingMotor(backend); return }
        if (editor && accounts && !bot.command && backend !== 'custom' && backend !== savedBackend) { void changeMotor(backend); return }
        feedback.clearError('backend')
        feedback.clearError('command')
        setBackend(backend)
        if (backend === bot.backend) setCommand(bot.command ?? '')
        const patch = engineSettingsIntent(backend, null, bot.backend)
        if (patch) save('backend', patch)
      }} />
    </ValueRow>
    {avisoDoMotor(backend) ? <AvisoDoMotor backend={backend} /> : null}
    {feedback.errors.backend ? <ControlError feedback={feedback} control="backend" /> : null}
    {editor && accounts && backend === savedBackend ? <ContaDoMotor bot={bot} editor={editor} draft={accounts.draft} model={accounts.model} disabled={accounts.disabled || changingMotor} /> : null}
    {backend === 'custom' ? <>
      <input className="field-box" aria-label={t('botSettings.motor.comandoCampo')} value={command} onChange={(e) => { feedback.clearError('command'); setCommand(e.target.value) }}
        onBlur={() => {
          if (bot.backend === 'custom' && command === (bot.command ?? '')) return
          const patch = engineSettingsIntent('custom', command, bot.backend)
          if (patch) save('command', patch)
        }} />
      <ControlError feedback={feedback} control="command" />
    </> : null}
    <fieldset className="bot-settings-controls" disabled={backend !== bot.backend}>
      {/* Revisão Sol do #459: o modelo pelo seletor único da C9 (favoritos, ⌘1–⌘3, sem o "Default"); grava como antes. */}
      {semSeletor ? (backend === 'custom' ? null : <p className="bot-settings-control-note">{t('botSettings.motor.semSeletor')}</p>)
        : listaDaConta ? <fieldset className="bot-settings-seletor" disabled={contaSemPosse}>
          <SeletorModelo key={chaveDoSeletor(backend)} modo="padraoDoBot" provedores={provedoresSeletor} valor={valorSeletor} carregar={(b) => carregarModelos(store, b)} onEscolher={escolherModelo} />
        </fieldset> : null}
      <ControlError feedback={feedback} control="model" />
      <ControlError feedback={feedback} control="modelOptions" />
      {!velocidadeNoSeletor && (bot.fastMode || fastModeShown(bot)) ? <SwitchRow title={t('botSettings.motor.rapido')} on={bot.fastMode ?? false}
        detail={fastModeRefusal(t, bot) ?? t('botSettings.motor.rapidoAjuda')} onChange={(fastMode) => save('fastMode', { fastMode })} /> : null}
      <ControlError feedback={feedback} control="fastMode" />
    </fieldset>
    {/* DON-865 fatia 4 (D372): o padrão vale para conversas novas; a principal só muda pelo botão (C9). */}
    <UsarNaPrincipal bot={bot} accountId={selectedAccount(editor?.profile) ?? bot.forkEditor?.engineAccountId ?? null} />
  </CardSection>
  <MotorIndisponiveis />
  <Dialog open={pendingMotor !== null} title={t('botSettings.motor.trocarTitulo')} message={t('botSettings.motor.trocarAjuda', { nome: bot.name })}
    cancel={t('botSettings.motor.cancelar')} language={superficieIdioma(idioma)} onClose={() => setPendingMotor(null)} actions={[{ title: t('botSettings.motor.trocar'), action: () => { if (pendingMotor) changeMotor(pendingMotor) } }]} />
  </>
}

export function CurrentBehaviourSettings({ bot, permission, save, feedback, busy = false, navegador, onFerramentas }: { bot: Bot; permission: (value: string) => void; save: SaveSettings; feedback: SettingsFeedback; busy?: boolean; navegador: { editor: BotEditor | null; draft: BotEditorProfile | null; model: BotSettingsStore }; onFerramentas: () => void }) {
  const store = useStore()
  const t = useT()
  const idioma = useIdioma()
  const idiomaApp = useIdioma()
  const [folder, setFolder] = useState(false)
  // R1 do #572: a seção é traduzida; declara o idioma e a direção (sem isso herdaria o limite pt-BR/LTR do body).
  return <div className="idioma-superficie" {...superficieIdioma(idiomaApp)} style={{ display: 'contents' }}>
    <CardSection title={t('bots.editor.autonomia')}>
      <ValueRow label={t('bots.ajustes.quantoPergunta')} detail={t(bot.permission === 'auto' ? 'bots.ajustes.autonomiaLiberar' : 'bots.ajustes.autonomiaPedir')}>
        <ChoicePicker language={superficieIdioma(idiomaApp)} selection={bot.permission} options={[{ id: 'ask', label: t('bots.editor.pedir') }, { id: 'auto', label: t('bots.editor.liberar') }]} onChange={permission} />
      </ValueRow>
      <ControlError feedback={feedback} control="permission" />
    </CardSection>
    {/* MX-111 (fase 1): Nunca fazer logo depois da autonomia; só orientação. */}
    <NuncaFazerBot store={store} bot={bot} onFerramentas={onFerramentas} />
    <CardSection title={t('bots.ajustes.pasta')}>
      <ValueRow label={t('bots.ajustes.pastaHub')} detail={t('bots.ajustes.pastaDetalhe')}>
        <DropdownMenu language={superficieIdioma(idiomaApp)} title={bot.cwd || t('bots.ajustes.pastaPropriaBot')} items={() => [
          { title: t('bots.ajustes.pastaPropriaBot'), selected: !bot.cwd, action: () => save('cwd', { cwd: '' }) },
          { title: t('bots.ajustes.outraPasta'), selected: !!bot.cwd, action: () => { feedback.clearError('cwd'); setFolder(true) } },
        ]}>
          <Pill outlined className="workspace-pill"><Text truncate className="workspace-pill-label">{bot.cwd ? lastPathComponent(bot.cwd) : t('bots.ajustes.pastaPropria')}</Text><Icon name="chevron.down" size={10} /></Pill>
        </DropdownMenu>
      </ValueRow>
      <ControlError feedback={feedback} control="cwd" />
    </CardSection>
    <CardSection title={t('bots.editor.computador')}>
      <div id="bot-usa-computador"><UsarOComputador screen={store.screen} bot={bot} online={podeMudarComputador(store)} ocupado={busy} erro={feedback.errors.computer ? textoDaMensagem(t, feedback.errors.computer) : null}
        onChange={(computer) => save('computer', { computer })} /></div>
      <TelaPorComputador bot={bot} disabled={busy || !podeMudarComputador(store)} />
    </CardSection>
    <NavegadorDoBot bot={bot} screen={store.screen} editor={navegador.editor} draft={navegador.draft} model={navegador.model} disabled={busy} />
    <Sheet language={superficieIdioma(idioma)} open={folder} onClose={() => setFolder(false)} width={460} height={520}>
      <FolderPicker key={String(folder)} start={bot.workspaceIssue ? store.hello?.home ?? null : bot.cwd || store.hello?.home || null} onPick={(cwd) => { setFolder(false); save('cwd', { cwd }) }} />
    </Sheet>
  </div>
}

/** MX-84: a seção em abas (Conectores, Logins, Skills) mora em ../bot-ferramentas; aqui fica só a entrada. */
export function CurrentToolsSettings(props: { bot: Bot; save: SaveSettings; saveSkill: SaveSkill; feedback: SettingsFeedback; /** Offline, sem permissão ou salvando: só os interruptores desligam; abas, busca e páginas seguem. */ disabled?: boolean }) {
  return <FerramentasEmAbas {...props} />
}

export function CurrentNotificationSettings({ bot, save, feedback }: { bot: Bot; save: SaveSettings; feedback: SettingsFeedback }) {
  const t = useT()
  const idioma = useIdioma()
  return <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}><CardSection><SwitchRow title={t('bots.editor.notificacoes')} detail={t('bots.ajustes.notificacoesDetalhe')} on={bot.notify ?? true} onChange={(notify) => save('notify', { notify })} />
    <ControlError feedback={feedback} control="notify" /></CardSection></div>
}

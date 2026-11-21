import { useState } from 'react'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { BotEditor, BotEditorPatch, BotEditorProfile } from '../../../../shared/fork-bot-editor.ts'
import type { ManagedAccount } from '../../../../shared/fork-engine-accounts.ts'
import type { Bot } from '../../../../shared/models.ts'
import { CardForm, ChoicePicker, ValueRow } from '../../../components/Controls'
import { ModalHeader, SheetPage } from '../../../components/Overlay'
import { VStack } from '../../../components/Stack'
import { useStore } from '../../../store/context'
import { Text } from '../../../components/Text'
import { FormSheet } from '../../marketplace/pieces'
import { ContasDoHub } from '../contas-ia/ContasDoHub'
import { LinhaDaConta } from '../contas-ia/FalhaDeConta'
import { engineDaSessao } from '../seletor-modelo/regras'
import { useModel } from '../../../lib/observable'
import { motorComConta } from '../../../../shared/fork-criar-bot-destino'
import '../contas-ia/contas-ia.css'
import type { BotSettingsStore } from './bot-settings-store'
import { accountOptions, accountPatch, applyEnginePatch, modelPatch, selectedAccount, cobrancaDaConta } from './engine-account'

/** Conta do motor e, com conta escolhida, o modelo que essa conta comprovou. */
export function ContaDoMotor({ bot, editor, draft, model, disabled }: {
  bot: Bot
  editor: BotEditor
  draft: BotEditorProfile | null
  model: BotSettingsStore
  disabled: boolean
}) {
  const t = useT()
  const idioma = superficieIdioma(useIdioma())
  const store = useStore()
  const [managing, setManaging] = useState(false)
  const sessions = useModel(store.sessions)
  // Capacidade própria respondida pelo Hub, sem inferir pela rota.
  const remote = !store.canManageEngineAccounts
  const profile = draft ?? editor.profile
  const backend = profile.backend ?? bot.backend
  if (!motorComConta(backend)) return null
  const selected = selectedAccount(profile)
  const account = selected ? editor.engineAccounts.find((a) => a.id === selected) ?? null : null
  const billing = account ? cobrancaDaConta(account) : null
  const runtime = editor.engineRuntime
  const apply = (patch: BotEditorPatch | null) => {
    const latest = model.getSnapshot().editor
    if (patch && latest) applyEnginePatch(model, latest, patch)
  }
  const warning = runtime && runtime.state !== 'connected'
    ? t('botSettings.conta.aviso', { estado: t(`botSettings.conta.estado.${runtime.state}`) })
    : null

  // Cabeçalho: no Pessoal (Berga) vale a conta da conversa; nos demais, a escolha do perfil (com o rascunho).
  const engine = engineDaSessao(sessions.of(bot.id)?.find((s) => s.principal))
  return (
    <VStack gap={10} className="conta-motor" {...idioma}>
      <LinhaDaConta accountId={engine ? engine.accountId ?? null : selected} backend={engine?.backend ?? backend} botId={bot.id} contasDoEditor={editor.engineAccounts} onClick={() => setManaging(true)} />
      <ValueRow label={t('botSettings.conta.rotulo')} detail={account ? t('botSettings.conta.detalhe', { nome: account.label, cobranca: billing ? t(`botSettings.conta.cobranca.${billing}`) : '' }) : t('botSettings.conta.semEscolha')}>
        <ChoicePicker
          language={idioma}
          selection={selected ?? ''}
          disabled={disabled || remote}
          options={accountOptions(t, editor, backend, selected)}
          onChange={(id) => apply(accountPatch(selected, id || null))}
        />
      </ValueRow>
      {selected ? <ModeloDaConta account={account} profile={profile} disabled={disabled || remote} onPatch={apply} /> : null}
      {remote ? <p className="conta-motor-aviso" role="note">{t('botSettings.conta.semAutorizacao')}</p> : null}
      {warning ? <p className="conta-motor-aviso" role="status">{warning}</p> : null}
      <div className="conta-motor-acoes">
        <button className="plain-link small" disabled={disabled} onClick={() => setManaging(true)}>{t('botSettings.conta.gerenciar')}</button>
      </div>
      <FormSheet language={idioma} open={managing} onClose={() => { setManaging(false); void model.refresh() }} width={600} height={620} fixed>
        {managing ? (
          <SheetPage>
            <ModalHeader closeTitle={t('botSettings.fechar')} icon="key" title={t('contas.pagina')} onClose={() => { setManaging(false); void model.refresh() }} />
            <div className="contas-panel-body">
              <CardForm>
                <ContasDoHub
                  forBot={{ name: bot.name, engine: backend, use: (id) => apply(accountPatch(selectedAccount(model.getSnapshot().draft ?? model.getSnapshot().editor?.profile), id)) }}
                  onChanged={() => void model.refresh()}
                />
              </CardForm>
            </div>
          </SheetPage>
        ) : null}
      </FormSheet>
    </VStack>
  )
}

/** Sem lista comprovada, fica o padrão da conta (nunca completar pela marca); com lista, quem escolhe é o seletor da C9. */
function ModeloDaConta({ account, profile, disabled, onPatch }: {
  account: ManagedAccount | null
  profile: BotEditorProfile
  disabled: boolean
  onPatch: (patch: BotEditorPatch) => void
}) {
  const t = useT()
  const current = profile.model ?? null
  const models = account?.models ?? []
  if (!models.length) {
    return (
      <ValueRow label={t('botSettings.conta.modelo')} detail={t('botSettings.conta.semModelos')}>
        {current ? (
          <button className="plain-link small" disabled={disabled} onClick={() => onPatch(modelPatch(null))}>{t('botSettings.conta.trocarPadrao', { modelo: current })}</button>
        ) : <Text tone="secondary">{t('botSettings.conta.padrao')}</Text>}
      </ValueRow>
    )
  }
  // Com lista comprovada, o modelo e as opções vêm do seletor da C9, na seção Motor (CurrentEngineSettings).
  return null
}

import type { EngineAccount } from '../../../../shared/fork-engine-accounts.ts'
import type { BotEditor, BotEditorPatch, BotEditorProfile } from '../../../../shared/fork-bot-editor.ts'
import { motorComConta } from '../../../../shared/fork-criar-bot-destino.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'

// D164: conta e modelo do bot pelo editor versionado (patch parcial com `expectedVersion`).

/** O Hub deste editor aceita escolher a conta do motor (Hub antigo não manda o campo editável). */
export const accountsEditable = (editor: BotEditor | null): editor is BotEditor =>
  !!editor && editor.capabilities.editableFields.includes('forkEditor')

/** A conta escolhida no perfil; `null` = padrão do Hub (comportamento antigo). */
export const selectedAccount = (profile: Pick<BotEditorProfile, 'forkEditor'> | null | undefined) =>
  profile?.forkEditor?.engineAccountId ?? null

export interface AccountOption {
  id: string
  label: string
}

/** R1 do #572: o estado da conta no idioma do app (mesma regra de `accountView`, sem o texto pt-BR de contas-ia). */
function estadoDaConta(t: Tradutor, account: { origin?: string; state: string; billing?: string | null }): string {
  switch (account.state) {
    case 'connected': return t('bots.ajustes.conta.estado.conectada')
    case 'unverified': return t(account.origin === 'managed' && account.billing === 'unverified' ? 'bots.ajustes.conta.estado.semLogin' : 'bots.ajustes.conta.estado.naoConfirmada')
    case 'unavailable': return t('bots.ajustes.conta.estado.indisponivel')
    default: return t('bots.ajustes.conta.estado.desconectada')
  }
}

/** `''` = padrão do Hub; depois as contas gerenciadas do mesmo motor, com o estado de cada uma. */
export function accountOptions(t: Tradutor, editor: BotEditor, backend: string, selected: string | null): AccountOption[] {
  if (!motorComConta(backend)) return []
  const legacy = editor.engineAccounts.find((a) => a.origin === 'legacy' && a.backend === backend)
  const options: AccountOption[] = [{ id: '', label: legacy ? t('bots.ajustes.conta.padraoCom', { estado: estadoDaConta(t, legacy) }) : t('bots.ajustes.conta.padrao') }]
  for (const account of editor.engineAccounts) {
    if (account.origin !== 'managed' || account.backend !== backend) continue
    options.push({ id: account.id, label: t('bots.ajustes.conta.comEstado', { conta: account.label, estado: estadoDaConta(t, account) }) })
  }
  // A escolha salva continua visível mesmo se a conta sumiu da lista: nunca trocar sem a pessoa.
  if (selected && !options.some((o) => o.id === selected)) options.push({ id: selected, label: t('bots.ajustes.conta.indisponivel') })
  return options
}

/** Trocar a conta: só `forkEditor`. `null` só pela escolha explícita do padrão do Hub. */
export function accountPatch(current: string | null, next: string | null): BotEditorPatch | null {
  return (current ?? null) === (next ?? null) ? null : { forkEditor: { engineAccountId: next } }
}

/** Trocar o motor de um bot com conta (depois de confirmar): a conta sai e o modelo volta ao padrão. */
export const motorPatch = (backend: string): BotEditorPatch => ({ backend, model: null, modelOptions: {}, forkEditor: { engineAccountId: null } })

/** Modelo da conta: só o modelo e as opções dele. */
export const modelPatch = (model: string | null): BotEditorPatch => ({ model, modelOptions: {} })

/** Opção de um modelo da conta; vazio volta ao padrão dela. */
export function modelOptionPatch(current: Record<string, string> | undefined, option: string, value: string): BotEditorPatch {
  const { [option]: _, ...rest } = current ?? {}
  return { modelOptions: value ? { ...rest, [option]: value } : rest }
}

export interface EngineEditorModel {
  change<K extends keyof BotEditorPatch>(field: K, value: BotEditorPatch[K], immediate?: boolean): void
}

/**
 * Coloca os campos no rascunho do editor e salva de uma vez: o store manda só o que mudou, com
 * `expectedVersion`. Campo que o Hub não declarou editável recusa o patch inteiro.
 */
export function applyEnginePatch(model: EngineEditorModel, editor: BotEditor, patch: BotEditorPatch): boolean {
  const fields = Object.keys(patch) as (keyof BotEditorPatch)[]
  if (!fields.length || fields.some((field) => !editor.capabilities.editableFields.includes(field))) return false
  fields.forEach((field, i) => model.change(field, patch[field], i === fields.length - 1))
  return true
}

/** API por chave continua identificável sem conexão; assinatura só se confirma conectada. */
export function cobrancaDaConta(account: Pick<EngineAccount, 'billing' | 'state'>): EngineAccount['billing'] | 'porChave' | null {
  if (account.billing === 'paid_api') return account.state === 'connected' ? 'paid_api' : 'porChave'
  return account.state === 'connected' ? account.billing : null
}

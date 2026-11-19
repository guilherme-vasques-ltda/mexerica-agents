import type { Bot, BotDraft } from '../../shared/models'
import { captureSkillSelection, captureSkillSelectionOrigin, sendSkillSelection, SkillSelectionOutcomeUnknown, SkillSelectionRefusal,
  type SkillSelectionItem, type SkillSelectionRefusalCode } from '../../shared/fork-skill-selection.ts'
import type { BotStore } from './bot-store'
import type { ChaveTexto } from '../lib/fork-i18n/index.ts'
import { ErroComChave, type Mensagem } from '../views/fork/bot-settings/mensagem.ts'

/** Controles existentes: cada intenção exclui identidade, autoria e gestão. Skills têm intenção própria (c1). */
export type BotSettingsIntent = Partial<Pick<BotDraft, 'backend' | 'command' | 'model' | 'modelOptions' | 'fastMode' | 'cwd' | 'connectors' | 'logins' | 'notify' | 'computer' | 'ownerPreferences'>>

/** Motor salvo não gera patch; custom só chega ao Hub com comando preenchido. */
export function engineSettingsIntent(backend: string, command: string | null = null, savedBackend?: string): BotSettingsIntent | null {
  if (backend === 'custom') {
    if (!command?.trim()) return null
    return { backend, command, ...(savedBackend === backend ? {} : { model: null, modelOptions: {} }) }
  }
  if (backend === savedBackend) return null
  return { backend, model: null, modelOptions: {}, command: null }
}

/** O evento bot do Hub atualiza o mirror; esta intenção não mantém uma segunda cópia. */
export async function saveBotSettingsIntent(store: BotStore, id: string, patch: BotSettingsIntent) {
  return (await store.ready()).call('updateBot', { id, ...patch })
}

// Fatia 3 (D454) e R1 do #572: cada recusa vira a CHAVE do catálogo (bots.ajustes.skill.*); a tela traduz ao mostrar.
const skillRefusals: Record<SkillSelectionRefusalCode, ChaveTexto> = {
  skill_selection_unsupported: 'bots.ajustes.skill.semProtecao',
  skill_selection_conflict: 'bots.ajustes.skill.conflito',
  skill_installation_stale: 'bots.ajustes.skill.reinstalada',
  skill_installation_changed: 'bots.ajustes.skill.mudou',
  skill_installation_unavailable: 'bots.ajustes.skill.indisponivel',
  skill_selection_origin_changed: 'bots.ajustes.skill.origem',
  skill_selection_not_ready: 'bots.ajustes.skill.desconectado',
  skills_personal_only: 'bots.ajustes.skill.pessoal',
}

/** A chave da recusa ou do resultado incerto (nenhum dos dois reenvia a escolha sozinho); outro erro: null. */
export function skillSelectionKey(error: unknown): ChaveTexto | null {
  if (error instanceof SkillSelectionRefusal) return skillRefusals[error.code]
  if (error instanceof SkillSelectionOutcomeUnknown) return 'bots.ajustes.skill.incerto'
  return null
}

/** A mensagem da recusa: a chave do catálogo, ou o texto pronto de um erro que não é de seleção. */
export function skillSelectionMessage(error: unknown): Mensagem {
  const chave = skillSelectionKey(error)
  return chave ? { chave } : { texto: error instanceof Error ? error.message : String(error) }
}

/**
 * c1: captura síncrona no gesto (versão do bot, instalação vista e origem), ANTES de
 * qualquer flush/await. Devolve o envio já preso a essa captura, ou a recusa local.
 */
export function captureSkillChoice(store: BotStore, bot: Pick<Bot, 'id' | 'skillSelection'>, skill: SkillSelectionItem, enabled: boolean):
  { send: () => Promise<unknown> } | { refused: Mensagem } {
  try {
    const origin = captureSkillSelectionOrigin(store)
    const request = captureSkillSelection(origin, bot, skill, enabled)
    return { send: async () => {
      try { return await sendSkillSelection(origin, request) } catch (error) { const chave = skillSelectionKey(error); throw chave ? new ErroComChave(chave, { cause: error }) : error }
    } }
  } catch (error) { return { refused: skillSelectionMessage(error) } }
}

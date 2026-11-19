import type { BrowserPreference } from './fork-browser.ts'
// D110 / 17.1: contrato parcial. Modelo, autonomia e Donna usam intenções próprias.
import type { ManagedAccount } from './fork-engine-accounts'
import type { AvatarConfig, MexeriquinhaConfig } from './fork-avatar-config'

// O transporte envia "código: mensagem"; a mensagem nunca contém erros crus do Hub.
export type BotEditorErrorCode =
  | 'bot_not_found'
  | 'bot_deleted'
  | 'settings_conflict'
  | 'not_allowed'
  | 'invalid_field'
  | 'folder_unavailable'
  | 'model_unavailable'
  | 'command_not_allowed'
  | 'save_failed'

export interface BotEditorProfile {
  browserPreference?: BrowserPreference
  name: string
  description: string
  avatarConfig: AvatarConfig | null
  mexeriquinha?: MexeriquinhaConfig
  /** D133: a função do bot; ausente em Hub antigo. */
  title?: string
  backend?: string
  model?: string | null
  modelOptions?: Record<string, string>
  fastMode?: boolean
  forkEditor?: { engineAccountId: string | null }
}

export type BotEditorPatch = Partial<Omit<BotEditorProfile, 'avatarConfig'> & { avatarConfig: AvatarConfig }>

export interface BotEditorSave {
  botId: string
  expectedVersion: string
  patch: BotEditorPatch
}

export interface BotEditor {
  botId: string
  settingsVersion: string
  profile: BotEditorProfile
  capabilities: {
    editableFields: (keyof BotEditorPatch)[]
    unavailableSections: string[]
  }
  permissions: { canEdit: boolean; canDelete: boolean }
  costInfo: null
  speedStatus: null
  engineAccounts: ManagedAccount[]
  engineRuntime: { engineAccountId: string; state: ManagedAccount['state']; revision?: number } | null
  application: 'next_turn' | 'apply_pending'
  workspaceIssue: string | null
  message: string
}

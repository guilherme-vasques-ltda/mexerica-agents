import { fieldEqual } from './bot-settings-store.ts'
import type { BotEditorProfile } from '../../../../shared/fork-bot-editor.ts'
import type { BotSettingsSection } from './bot-settings-entry.tsx'
import type { SettingsControl } from './CurrentSettingsSections.tsx'

/** A lista já vem filtrada pelas capacidades (inclusive o plugin WhatsApp). */
export function abaPeloTeclado<T>(abas: readonly T[], atual: T, tecla: string): T | null {
  const index = abas.indexOf(atual)
  if (index < 0 || !abas.length) return null
  if (tecla === 'Home') return abas[0]!
  if (tecla === 'End') return abas[abas.length - 1]!
  if (tecla === 'ArrowDown') return abas[(index + 1) % abas.length]!
  if (tecla === 'ArrowUp') return abas[(index + abas.length - 1) % abas.length]!
  return null
}

/** Tablist horizontal: cima/baixo continuam disponíveis para rolar a página. */
export function abaHorizontalPeloTeclado<T>(abas: readonly T[], atual: T, tecla: string, direcao: 'ltr' | 'rtl'): T | null {
  if (tecla === 'Home' || tecla === 'End') return abaPeloTeclado(abas, atual, tecla)
  if (tecla !== 'ArrowLeft' && tecla !== 'ArrowRight') return null
  const avanca = (tecla === 'ArrowRight') === (direcao === 'ltr')
  return abaPeloTeclado(abas, atual, avanca ? 'ArrowDown' : 'ArrowUp')
}

export function abaDaIntencao(control: SettingsControl): BotSettingsSection {
  if (control === 'ownerPreferences') return 'identity'
  if (control === 'permission' || control === 'cwd' || control === 'computer' || control === 'localCommandsEnabled') return 'behaviour'
  if (control === 'connectors' || control === 'logins' || control === 'skills') return 'tools'
  if (control === 'notify') return 'notifications'
  return 'engine'
}

const campos = {
  identity: ['name', 'title', 'description', 'avatarConfig'],
  engine: ['backend', 'model', 'modelOptions', 'fastMode', 'forkEditor'],
  behaviour: ['browserPreference'],
} as const

/** O editor mantém um rascunho único; cada aba só anuncia seus próprios campos. */
export function abaAlterada(aba: BotSettingsSection, draft: BotEditorProfile | null, saved: BotEditorProfile | null): boolean {
  if (!draft || !saved) return false
  const camposDaAba = aba === 'identity' ? campos.identity : aba === 'engine' ? campos.engine : aba === 'behaviour' ? campos.behaviour : null
  if (!camposDaAba) return false
  return camposDaAba.some((campo) => !fieldEqual(campo, draft[campo], saved[campo]))
}

/** Erros sobrevivem à navegação; uma gravação do editor pode abranger dois grupos de campos. */
export function abasComErro(editorFalhou: boolean, origem: BotSettingsSection, draft: BotEditorProfile | null, saved: BotEditorProfile | null, intencoes: Partial<Record<SettingsControl, unknown>>): Set<BotSettingsSection> {
  const abas = new Set<BotSettingsSection>()
  if (editorFalhou) {
    for (const aba of ['identity', 'engine', 'behaviour'] as const) if (abaAlterada(aba, draft, saved)) abas.add(aba)
    if (!abas.size) abas.add(origem)
  }
  for (const control of Object.keys(intencoes) as SettingsControl[]) abas.add(abaDaIntencao(control))
  return abas
}

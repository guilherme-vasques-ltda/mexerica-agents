import { createContext, useContext } from 'react'
import type { BotStore } from '../../../store/bot-store'
import { Button } from '../../../components/Controls'
import { useT } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'

/** `section`: open on that section of the page (the shield opens Comportamento, D169). */
export type BotSettingsSection = 'identity' | 'engine' | 'behaviour' | 'tools' | 'whatsapp' | 'memory' | 'voice' | 'notifications' | 'routines' | 'export' | 'danger'
export const OpenBotSettings = createContext<((store: BotStore, botId: string, section?: BotSettingsSection) => void) | null>(null)
export const useOpenBotSettings = () => useContext(OpenBotSettings)
export interface BotSettingsHandle { leave(action: () => void): void }

export function BotSettingsPanel({ botId }: { botId: string; part?: 'often' | 'rarely' }) {
  const store = useStore()
  const open = useOpenBotSettings()
  const t = useT()
  // Auditoria D249, F5: secundário; a aba Configurações leva ao mesmo lugar e o destaque fica com o Parar.
  return <Button kind="secondary" onClick={() => open?.(store, botId)}>{t('bots.ajustes.configuracoesBot')}</Button>
}

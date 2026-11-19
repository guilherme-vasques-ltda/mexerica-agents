// DON-907 C1: as escolhas de Ajustes → Conversa, deste aparelho (localStorage). Valor guardado inválido cai no padrão.
import { pref, usePref } from '../../../lib/prefs'
import { isMac } from '../../settings/parts'
import { isSendMode, sendShortcuts, type SendMode } from '../../thread/composer-keyboard'

const isBoolean = (value: unknown): value is boolean => typeof value === 'boolean'

export const conversaPrefs = {
  /** Atalho de envio; o padrão é o comportamento de sempre (Enter envia). */
  sendMode: pref<SendMode>('composerSendMode', 'enter', isSendMode),
  /** As skills do bot no menu `/`; desligado, só os comandos próprios (`/fast`). */
  slashSkills: pref<boolean>('slashMenuSkills', true, isBoolean),
  /** DON-906 C2: o medidor de contexto no rodapé do composer; desligado, nem pede ao Hub. */
  contextMeter: pref<boolean>('contextMeter', true, isBoolean),
}

/** O que o compositor lê de Ajustes → Conversa (o único gancho no Composer.tsx herdado). */
export function useConversaComposer() {
  const [sendMode] = usePref(conversaPrefs.sendMode)
  const [slashSkills] = usePref(conversaPrefs.slashSkills)
  return { sendMode, slashSkills, shortcuts: sendShortcuts(sendMode, isMac) }
}

import { useEffect, useId, useState } from 'react'
import type { Bot } from '@shared/models'
import { formatarNumero } from '../../../lib/fork-i18n/index'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { textoDaMensagem } from './mensagem'
import type { SaveSettings, SettingsFeedback } from './CurrentSettingsSections'

/** Mesmo limite das instruções no Hub (`donna_fixa::PREFERENCIAS_LIMITE`). */
const LIMITE = 4000

/** D368: instruções extras do dono; entram depois das instruções base, que continuam fixas. */
export function DonnaPreferencias({ bot, save, feedback, disabled }: { bot: Bot; save: SaveSettings; feedback: SettingsFeedback; disabled: boolean }) {
  const id = useId()
  const t = useT()
  const idioma = useIdioma()
  const saved = bot.ownerPreferences ?? ''
  const [text, setText] = useState(saved)
  useEffect(() => setText(saved), [bot.id, saved])
  useEffect(() => { feedback.draftChanged?.('ownerPreferences', text !== saved) }, [text, saved, feedback.draftChanged])
  const flush = () => { if (text !== saved) save('ownerPreferences', { ownerPreferences: text }) }
  return <div className="bot-settings-card">
    <label htmlFor={`${id}-prefs`} {...superficieIdioma(idioma)}>{t('botSettings.donnaFixa.preferencias')}</label>
    <p id={`${id}-help`} className="bot-settings-help" {...superficieIdioma(idioma)}>{t('botSettings.donnaFixa.preferenciasAjuda', { limite: formatarNumero(idioma, LIMITE) })}</p>
    {/* Texto do dono: direção pelo próprio conteúdo. */}
    <textarea id={`${id}-prefs`} className="field-box" dir="auto" rows={4} value={text} maxLength={LIMITE} disabled={disabled} aria-describedby={`${id}-help`}
      onChange={(e) => { feedback.clearError('ownerPreferences'); setText(e.target.value) }} onBlur={flush} />
    {feedback.errors.ownerPreferences ? <p className="bot-settings-control-error" role="alert">{textoDaMensagem(t, feedback.errors.ownerPreferences)}</p> : null}
  </div>
}

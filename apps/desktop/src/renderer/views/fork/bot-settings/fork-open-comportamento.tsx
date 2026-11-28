import type { Bot } from '@shared/models'
import { CardSection, ChoicePicker, ValueRow } from '../../../components/Controls'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'
import { NuncaFazerBot } from '../nunca-fazer/NuncaFazer'
import { SwitchRow } from '../../bots/parts'
import { AvisoDoFluxo } from './fork-open-fluxo'
import { textoDaMensagem, type Mensagem } from './mensagem'

/** Comportamento na Open: autonomia e "nunca fazer" rodam no Hub; pasta, computador e navegador mostram o fluxo. */
export function ComportamentoDaOpen({ bot, permission, localCommands, commandError, onFerramentas }: { bot: Bot; permission: (value: string) => void; localCommands: (enabled: boolean) => void; commandError?: Mensagem; onFerramentas: () => void }) {
  const store = useStore()
  const t = useT()
  const idioma = superficieIdioma(useIdioma())
  return <div className="idioma-superficie" {...idioma} style={{ display: 'contents' }}>
    <CardSection title={t('bots.editor.autonomia')}>
      <ValueRow label={t('bots.ajustes.quantoPergunta')} detail={t(bot.permission === 'auto' ? 'bots.ajustes.autonomiaLiberar' : 'bots.ajustes.autonomiaPedir')}>
        <ChoicePicker language={idioma} selection={bot.permission} options={[{ id: 'ask', label: t('bots.editor.pedir') }, { id: 'auto', label: t('bots.editor.liberar') }]} onChange={permission} />
      </ValueRow>
    </CardSection>
    {bot.localCommandsEnabled !== undefined ? <CardSection>
      <SwitchRow title={t('botSettings.comandosLocais.titulo')} detail={t('botSettings.comandosLocais.aviso')} on={bot.localCommandsEnabled} onChange={localCommands} />
      {commandError ? <p className="bot-settings-control-error" role="alert">{textoDaMensagem(t, commandError)}</p> : null}
    </CardSection> : null}
    <NuncaFazerBot store={store} bot={bot} onFerramentas={onFerramentas} />
    {!bot.localCommandsEnabled ? <AvisoDoFluxo fluxo="pasta" /> : null}
    <AvisoDoFluxo fluxo="computador" />
    <AvisoDoFluxo fluxo="navegador" />
  </div>
}

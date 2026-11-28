import type { Bot } from '@shared/models'
import { CardSection, ChoicePicker, ValueRow } from '../../../components/Controls'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { useStore } from '../../../store/context'
import { NuncaFazerBot } from '../nunca-fazer/NuncaFazer'
import { AvisoDoFluxo } from './fork-open-fluxo'

/** Comportamento na Open: autonomia e "nunca fazer" rodam no Hub; pasta, computador e navegador mostram o fluxo. */
export function ComportamentoDaOpen({ bot, permission, onFerramentas }: { bot: Bot; permission: (value: string) => void; onFerramentas: () => void }) {
  const store = useStore()
  const t = useT()
  const idioma = superficieIdioma(useIdioma())
  return <div className="idioma-superficie" {...idioma} style={{ display: 'contents' }}>
    <CardSection title={t('bots.editor.autonomia')}>
      <ValueRow label={t('bots.ajustes.quantoPergunta')} detail={t(bot.permission === 'auto' ? 'bots.ajustes.autonomiaLiberar' : 'bots.ajustes.autonomiaPedir')}>
        <ChoicePicker language={idioma} selection={bot.permission} options={[{ id: 'ask', label: t('bots.editor.pedir') }, { id: 'auto', label: t('bots.editor.liberar') }]} onChange={permission} />
      </ValueRow>
    </CardSection>
    <NuncaFazerBot store={store} bot={bot} onFerramentas={onFerramentas} />
    <AvisoDoFluxo fluxo="pasta" />
    <AvisoDoFluxo fluxo="computador" />
    <AvisoDoFluxo fluxo="navegador" />
  </div>
}

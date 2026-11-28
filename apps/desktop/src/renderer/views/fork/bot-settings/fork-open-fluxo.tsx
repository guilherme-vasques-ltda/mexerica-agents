import { CardSection } from '../../../components/Controls'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'

export type FluxoDaOpen = 'pasta' | 'computador' | 'navegador' | 'conectores' | 'whatsapp' | 'voz'

/** Na Open o controle que depende de shell, computador, login, Cofre ou voz aparece com o aviso do fluxo, em vez de sumir. */
export function AvisoDoFluxo({ fluxo }: { fluxo: FluxoDaOpen }) {
  const t = useT()
  const idioma = useIdioma()
  return <div className="idioma-superficie" {...superficieIdioma(idioma)} style={{ display: 'contents' }}>
    <CardSection><p className="bot-settings-control-note">{t(`botSettings.open.fluxo.${fluxo}`)}</p></CardSection>
  </div>
}

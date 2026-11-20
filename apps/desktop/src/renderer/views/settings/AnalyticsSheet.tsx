// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { CardForm, CardSection } from '../../components/Controls'
import { ModalHeader, SheetPage } from '../../components/Overlay'
import { Text } from '../../components/Text'
import { useT } from '../../lib/fork-i18n/estado'
import { Superficie } from './rotulo'

/** Descrição da coleta de analytics, que permanece desativada no fork. */
export function AnalyticsSheet() {
  const t = useT()
  return (
    <SheetPage>
      <Superficie>
      <ModalHeader closeTitle={t('ajustes.fechar')} icon="chart.bar" title={t('ajustes.privacidade.folhaTitulo')} />
      <CardForm>
        <CardSection>
          <Text size="footnote" tone="secondary">{t('ajustes.privacidade.desativada')}</Text>
        </CardSection>
      </CardForm>
      </Superficie>
    </SheetPage>
  )
}

import { CardSection } from '../../../components/Controls'
import { useT } from '../../../lib/fork-i18n/estado'

/** D440 (P49=B, substitui a D407): as três funções ainda inexistentes aparecem apagadas, com selo e uma frase.
 *  Sem controle, foco ou gravação: não lê nem grava as preferências antigas guardadas no Hub. */
export const FUNCOES_INDISPONIVEIS = ['auto', 'reservas', 'guias'] as const

export function MotorIndisponiveis() {
  const t = useT()
  return <CardSection title={t('botSettings.indisponivel.titulo')} footer={t('botSettings.indisponivel.nota')}>
    {FUNCOES_INDISPONIVEIS.map((id) => <div key={id} className="bot-settings-futura" role="group" aria-disabled="true" aria-labelledby={`motor-futura-${id}`} data-funcao={id}>
      <span className="bot-settings-futura-nome" id={`motor-futura-${id}`}>{t(`botSettings.indisponivel.${id}`)}<span className="bot-settings-futura-selo">{t('botSettings.indisponivel.selo')}</span></span>
      <span className="bot-settings-futura-linha">{t(`botSettings.indisponivel.${id}Linha`)}</span>
    </div>)}
  </CardSection>
}

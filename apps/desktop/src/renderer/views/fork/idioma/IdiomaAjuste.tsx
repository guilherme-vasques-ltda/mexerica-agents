import { CardSection, ChoiceList } from '../../../components/Controls'
import { idiomaPref, superficieIdioma, tradutor, useIdioma } from '../../../lib/fork-i18n/estado'
import { ORDEM_RODA } from './passo'
import './escritas.css'
import { AvisosIdioma } from './AvisosIdioma'

/** DON-903: Ajustes → Geral → Idioma. A seção inteira segue o idioma escolhido; a troca vale na hora. */
export function IdiomaAjuste() {
  const idioma = useIdioma()
  const t = tradutor(idioma)
  return (
    <div className="idioma-superficie" {...superficieIdioma(idioma)}>
    <CardSection title={t('idioma.ajuste.secao')} footer={idioma === 'pt-BR' ? null : <AvisosIdioma idioma={idioma} className="idioma-ajuste-avisos" />}>
      <ChoiceList
        label={t('idioma.ajuste.rotulo')}
        selection={idioma}
        options={ORDEM_RODA.map((id) => ({ id, label: tradutor(id)('idioma.nome') }))}
        onChange={(id) => idiomaPref.set(id)}
      />
    </CardSection>
    </div>
  )
}

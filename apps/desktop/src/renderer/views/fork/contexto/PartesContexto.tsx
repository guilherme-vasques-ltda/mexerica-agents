// O que ocupa o espaço da conversa, parte a parte, dentro de Ver detalhes. O motor informa só o total usado;
// cada parte diz de onde vem o número (estimado pela Mexerica ou não informado) e nunca mostra estimativa como medida.
import type { AppLanguage } from '../../../../shared/fork-idioma.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado'
import { formatarNumero } from '../../../lib/fork-i18n/index.ts'
import { tokensCurtos, type LeituraContexto } from './estado'

export function PartesContexto({ leitura, idioma, t }: { leitura: LeituraContexto; idioma: AppLanguage; t: Tradutor }) {
  const { partes, used } = leitura
  return (
    <section className="ctx-partes" aria-label={t('contexto.partes.titulo')}>
      <h4>{t('contexto.partes.titulo')}</h4>
      {partes ? (
        <>
          <ul>
            <li>
              <span className="ctx-parte-nome">{t('contexto.partes.total')}</span>
              <span className="ctx-parte-valor">{t('contexto.partes.tokensExatos', { n: formatarNumero(idioma, used) })}</span>
              <span className="ctx-parte-origem">{t('contexto.partes.origem.informado')}</span>
            </li>
            {partes.itens.map((parte) => (
              <li key={parte.id} className={parte.tokens === null ? 'is-vazio' : undefined}>
                <span className="ctx-parte-nome">{t(`contexto.partes.parte.${parte.id}`)}</span>
                <span className="ctx-parte-valor">{parte.tokens === null ? t('contexto.partes.semNumero') : t('contexto.partes.tokensEstimados', { n: tokensCurtos(idioma, parte.tokens) })}</span>
                <span className="ctx-parte-origem">{t(parte.origem === 'estimado' ? 'contexto.partes.origem.estimado' : parte.id === 'ferramentas' ? 'contexto.partes.origem.naoInformado' : 'contexto.partes.origem.aindaNao')}</span>
                {parte.tokens !== null && used > 0 ? <span className="ctx-parte-barra" aria-hidden="true"><i style={{ inlineSize: `${Math.min(100, Math.round((parte.tokens / used) * 100))}%` }} /></span> : null}
              </li>
            ))}
          </ul>
          <p className="ctx-nota-glossario">{t('contexto.partes.nota', { n: String(partes.caracteresPorToken) })}</p>
        </>
      ) : <p className="ctx-nota-glossario">{t('contexto.partes.antigo')}</p>}
    </section>
  )
}

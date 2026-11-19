import { formatarData } from '../../../lib/fork-i18n/index.ts'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { semParametros } from '../../settings/rotulo'
import type { NonSavedHistory as History } from './non-saved'
import { nonSavedReason } from './non-saved'
import './memory-auto.css'

/** Histórico somente leitura; disposição passada não representa fila nem autoriza ações. */
export function NonSavedHistory({ history }: { history: History }) {
  const t = useT()
  const idioma = useIdioma()
  return <section aria-label={t('memoria.auto.naoSalvei.rotulo')} className="memory-non-saved">
    <p className="memory-muted">{t('memoria.auto.naoSalvei.lead')}</p>
    {history.state !== 'ready' ? <p className="memory-muted" role="status">{history.state === 'loading'
      ? t('memoria.auto.naoSalvei.consultando')
      : history.state === 'unavailable'
        ? t('memoria.auto.naoSalvei.indisponivel')
        : t('memoria.auto.naoSalvei.naoConfirmado')}</p>
      : history.rows.length === 0 ? <p className="memory-muted">{t('memoria.auto.naoSalvei.vazio')}</p>
        : <ul className="memory-content memory-non-saved-list">
          {history.rows.map((row) => <li key={row.id} className="memory-content">
            <strong>{semParametros(t)(nonSavedReason[row.reason])}</strong>
            <small>{row.status === 'deferred' ? t('memoria.auto.naoSalvei.adiado') : t('memoria.auto.naoSalvei.naoSalvo')} · {row.at > 0
              ? <time dir="ltr" className="bidi-isolado" dateTime={new Date(row.at).toISOString()}>{formatarData(idioma, row.at, { day: '2-digit', month: '2-digit', year: 'numeric', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' })}</time>
              : t('memoria.auto.versao.semData')}</small>
          </li>)}
        </ul>}
  </section>
}

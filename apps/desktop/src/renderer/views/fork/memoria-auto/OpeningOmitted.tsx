import { formatarData } from '../../../lib/fork-i18n/index.ts'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { semParametros } from '../../settings/rotulo'
import type { MemoryView } from '../../../store/memory-store'
import type { OpeningOmittedHistory as History } from './opening-omitted'
import { openingOmittedReason } from './opening-omitted'
import './memory-auto.css'

/** O que ficou de fora da última abertura do bot e por quê; somente leitura. */
export function OpeningOmitted({ history, view }: { history: History; view: MemoryView | null }) {
  const t = useT()
  const idioma = useIdioma()
  const textOf = (layer: 'yours' | 'bot', entry: number) =>
    (layer === 'yours' ? view?.yours : view?.bot)?.entries.find((e) => e.id === entry)?.text
  return <section aria-label={t('memoria.painel.foraAbertura.rotulo')} className="memory-non-saved">
    <p className="memory-muted">{t('memoria.painel.foraAbertura.lead')}</p>
    {history.state !== 'ready' ? <p className="memory-muted" role="status">{history.state === 'loading'
      ? t('memoria.painel.foraAbertura.consultando')
      : history.state === 'unavailable'
        ? t('memoria.painel.foraAbertura.indisponivel')
        : t('memoria.painel.foraAbertura.naoConfirmado')}</p>
      : history.rows.length === 0 ? <p className="memory-muted">{t('memoria.painel.foraAbertura.vazio')}</p>
        : <ul className="memory-content memory-non-saved-list">
          {history.rows.map((row) => {
            const text = textOf(row.layer, row.entry)
            return <li key={`${row.layer}:${row.entry}`} className="memory-content">
              <strong>{semParametros(t)(openingOmittedReason[row.reason])}</strong>
              {text ? <span className="line-clamp memory-clamp" title={text}><bdi dir="auto">{text}</bdi></span> : null}
              <small>{t(row.layer === 'yours' ? 'memoria.painel.aba.voce' : 'memoria.painel.aba.bot')}{row.at > 0
                ? <> · <time dir="ltr" className="bidi-isolado" dateTime={new Date(row.at).toISOString()}>{formatarData(idioma, row.at, { day: '2-digit', month: '2-digit', year: 'numeric', hour: '2-digit', minute: '2-digit', hourCycle: 'h23' })}</time></>
                : null}</small>
            </li>
          })}
        </ul>}
  </section>
}

// MEX-1391 (F6 fatia B, regra (o).2): seção "Trocas de conta" do Mission Board. Só desenha `accountSwitches` do snapshot:
// de qual conta para qual, por quê e quando. É informação, não decisão: sem botão que mude algo.
import type { TowerSnapshot } from '@shared/fork-torre'
import { ago } from '@shared/fork-torre-pedidos'
import { isolado } from '@shared/bidi'
import { haQuanto } from './formato'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { whenText } from '../contas-ia/regras'
import { Icon } from '../../../components/Icon'
import './trocas-de-conta.css'

export function TrocasDeConta({ snapshot }: { snapshot: TowerSnapshot }) {
  const t = useT()
  const idioma = useIdioma()
  const TITLE = t('torre.trocas.titulo')
  const observed = snapshot.accountSwitches
  if (observed.availability !== 'available') {
    return (
      <section className="torre-group trocas" aria-label={TITLE}>
        <h2 className="torre-group-title">{TITLE} <span>{t('torre.pedidos.seteDias')}</span></h2>
        <p className="trocas-empty"><span className="torre-pill is-plain">{t('torre.estado.unknown')}</span> {t('torre.trocas.naoInformou')}</p>
      </section>
    )
  }
  const bots = new Map(snapshot.bots.map((b) => [b.botId, b.name]))
  const { items, omitted } = observed.value
  return (
    <section className="torre-group trocas" aria-label={TITLE}>
      <h2 className="torre-group-title">{TITLE} <span>{(items.length + omitted).toLocaleString(idioma)} · {t('torre.pedidos.seteDias')}</span></h2>
      {items.length === 0 ? (
        <p className="trocas-empty">{t('torre.trocas.nenhuma')}</p>
      ) : (
        <div className="trocas-list">
          {items.map((item) => (
            <div key={item.id} className="trocas-row" data-troca={item.id}>
              <span className="trocas-contas">
                <b dir="auto">{isolado(item.fromLabel)}</b>
                <Icon name="arrow.right" size={11} />
                <b dir="auto">{isolado(item.toLabel)}</b>
              </span>
              <span className="trocas-bot" dir="auto">{isolado(bots.get(item.botId) ?? '—')}</span>
              <span className={`torre-pill trocas-motivo-${item.reason}`}><i aria-hidden />{t(`torre.trocas.motivo.${item.reason}`)}</span>
              <span className="trocas-when">
                {haQuanto(ago(item.at, snapshot.collectedAt), idioma)}
                {item.backAt ? ` · ${t('torre.trocas.volta', { quando: whenText(item.backAt, idioma, snapshot.collectedAt) })}` : ''}
              </span>
            </div>
          ))}
          {omitted ? <p className="trocas-more">{t.plural('torre.trocas.maisAntigas', omitted)}</p> : null}
        </div>
      )}
      <p className="torre-foot"><Icon name="info.circle" size={13} /> {t('torre.trocas.rodape')}</p>
    </section>
  )
}

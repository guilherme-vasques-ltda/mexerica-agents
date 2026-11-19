// Times R3a / MB-2: seção "Pedidos entre bots" no fim do Mission Board.
// Só desenha `interBotRequests` do snapshot. O único botão é Abrir, que reaproveita o destino já validado do cartão de
// quem pediu (`bot.open` → `goTo`). Não retoma, não repete, não responde e não mostra texto de pedido ou resposta.
import { memo } from 'react'
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import type { TowerBot, TowerSnapshot } from '@shared/fork-torre'
import { ago, interBotLabel } from '@shared/fork-torre-pedidos'
import { haQuanto } from './formato'
import { isolado } from '@shared/bidi'
import { localeDe } from '../../../lib/fork-i18n/index.ts'
import { useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { Icon } from '../../../components/Icon'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import './pedidos-entre-bots.css'


const Face = memo(function Face({ bot }: { bot: TowerBot }) {
  const config = normalizarAvatarDoBot(avatarDoBot(bot), bot.name)
  return <DonnabotAvatar config={config} size={24} animated={false} label={bot.name} />
})

export function PedidosEntreBots({ snapshot, personal, goTo }: {
  snapshot: TowerSnapshot
  personal: boolean
  goTo: (conversationId: string, botId: string) => (() => void) | null
}) {
  const t = useT()
  const idioma = useIdioma()
  const TITLE = t('torre.pedidos.titulo')
  const observed = snapshot.interBotRequests
  const head = (count: number | null, note: string) => (
    <h2 className="torre-group-title">{TITLE} <span>{count === null ? '' : `${count.toLocaleString(localeDe(idioma))} · `}{note}</span></h2>
  )
  if (!personal) {
    return (
      <section className="torre-group pedidos" aria-label={TITLE}>
        {head(null, t('torre.pedidos.soPessoalNota'))}
        <p className="pedidos-empty">{t('torre.pedidos.soPessoal')}</p>
      </section>
    )
  }
  if (observed.availability !== 'available') {
    return (
      <section className="torre-group pedidos" aria-label={TITLE}>
        {head(null, t('torre.pedidos.seteDias'))}
        <p className="pedidos-empty"><span className="torre-pill is-plain">{t('torre.estado.unknown')}</span> {t('torre.pedidos.naoInformou')}</p>
      </section>
    )
  }
  const byId = new Map(snapshot.bots.map((b) => [b.botId, b]))
  const rows = observed.value.items.flatMap((item) => {
    const from = byId.get(item.requesterBotId)
    const to = byId.get(item.targetBotId)
    const what = interBotLabel(item)
    return from && to && what ? [{ item, from, to, what }] : []
  })
  const total = rows.length + observed.value.omitted
  return (
    <section className="torre-group pedidos" aria-label={TITLE}>
      {head(total, t('torre.pedidos.naoTerminaram'))}
      {total === 0 ? (
        <p className="pedidos-empty">{t('torre.pedidos.nenhum')}</p>
      ) : (
        <div className="pedidos-list">
          {rows.map(({ item, from, to, what }) => {
            const open = from.open ? goTo(from.open.conversation.conversationId, from.botId) : null
            return (
              <div key={item.key} className="pedidos-row" data-pedido={item.key}>
                <span className="pedidos-faces" aria-hidden><Face bot={from} /><Icon name="arrow.right" size={11} /><Face bot={to} /></span>
                <span className="pedidos-who">{t('torre.pedidos.pediuA', { de: isolado(from.name), para: isolado(to.name) })}</span>
                <span className={`torre-pill pedidos-tone-${what.tone}`}><i aria-hidden />{t(`torre.pedidos.rotulo.${what.rotulo}`)}</span>
                <span className="pedidos-when">{t(`torre.pedidos.modo.${item.mode}`)} · {haQuanto(ago(item.endedAt, snapshot.collectedAt), idioma)}</span>
                <button className="torre-button" disabled={!open} title={open ? t('torre.pedidos.abrirDe', { nome: isolado(from.name) }) : t('torre.pedidos.semConversaDe', { nome: isolado(from.name) })}
                  onClick={() => open?.()}>{t('torre.abrir')}</button>
              </div>
            )
          })}
          {observed.value.omitted ? (
            <p className="pedidos-more">{t.plural('torre.pedidos.maisAntigos', observed.value.omitted)}</p>
          ) : null}
        </div>
      )}
      <p className="torre-foot"><Icon name="info.circle" size={13} /> {t('torre.pedidos.rodape')}</p>
    </section>
  )
}

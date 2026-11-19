// MX-478: permissões pedidas por um turno delegado, em Pontas soltas (Mission Board).
// Só desenha `delegatedPermissions` do snapshot. O único botão é Abrir: leva à conversa de QUEM RECEBEU o pedido, onde
// está o cartão de permissão original (a resposta continua lá e em "Esperam você"). Não aprova, não nega, não retoma
// e não mostra texto de pedido, comando ou argumento. Sem o campo (Hub anterior) ou sem linha confirmada pela
// própria fotografia, a seção não aparece.
import { memo } from 'react'
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import type { TowerBot, TowerSnapshot } from '@shared/fork-torre'
import { ago } from '@shared/fork-torre-pedidos'
import { delegatedRows } from '@shared/fork-torre-permissoes'
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

export function PermissoesDelegadas({ snapshot, live, goTo }: {
  snapshot: TowerSnapshot
  /** Sem conexão a fotografia é de antes: o cartão pode já ter sido respondido, então nada é listado (como em `pending`). */
  live: boolean
  goTo: (conversationId: string, botId: string) => (() => void) | null
}) {
  const t = useT()
  const idioma = useIdioma()
  const observed = snapshot.delegatedPermissions
  if (!live || observed.availability !== 'available') return null
  const rows = delegatedRows(observed.value.items, snapshot.bots)
  // `omitted` sozinho não é confirmado pela fotografia: sem nenhuma linha confirmada, a seção não aparece.
  if (rows.length === 0) return null
  const total = rows.length + observed.value.omitted
  const TITLE = t('torre.permDeleg.titulo')
  return (
    <section className="torre-group pedidos" aria-label={TITLE}>
      <h2 className="torre-group-title">{TITLE} <span>{total.toLocaleString(localeDe(idioma))} · {t('torre.permDeleg.nota')}</span></h2>
      <div className="pedidos-list">
        {rows.map(({ item, from, to }) => {
          const open = goTo(item.conversation.conversationId, to.botId)
          return (
            <div key={item.key} className="pedidos-row" data-permissao={item.key}>
              <span className="pedidos-faces" aria-hidden><Face bot={from} /><Icon name="arrow.right" size={11} /><Face bot={to} /></span>
              <span className="pedidos-who">{t('torre.pedidos.pediuA', { de: isolado(from.name), para: isolado(to.name) })}</span>
              <span className="torre-pill pedidos-tone-uncertain"><i aria-hidden />{t('torre.permDeleg.pilula')}</span>
              <span className="pedidos-when">{t(`torre.pedidos.modo.${item.mode}`)} · {haQuanto(ago(item.requestedAt, snapshot.collectedAt), idioma)}</span>
              <button className="torre-button" disabled={!open} title={open ? t('torre.pedidos.abrirDe', { nome: isolado(to.name) }) : t('torre.pedidos.semConversaDe', { nome: isolado(to.name) })}
                onClick={() => open?.()}>{t('torre.abrir')}</button>
            </div>
          )
        })}
        {observed.value.omitted ? (
          <p className="pedidos-more">{t.plural('torre.permDeleg.maisAntigos', observed.value.omitted)}</p>
        ) : null}
      </div>
      <p className="torre-foot"><Icon name="info.circle" size={13} /> {t('torre.permDeleg.rodape')}</p>
    </section>
  )
}

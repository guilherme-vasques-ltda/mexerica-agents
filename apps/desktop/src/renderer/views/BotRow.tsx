import { spaceBotLabel } from '../lib/fork-spaces-rail'
import { memo } from 'react'
import { useIdioma, useT, type Tradutor } from '../lib/fork-i18n/estado'
import { activityOriginal, activityText } from '../lib/fork-atividade'
import type { AppLanguage } from '@shared/fork-idioma'
import { folderName, isGroup, isUnavailable, isWorking, needsInput, UNAVAILABLE_TEXT, type Bot } from '@shared/models'
import { onlyQuestionsPending } from '@shared/fork-owner-question'
import { BotAvatar } from '../components/Avatar'
import { Icon } from '../components/Icon'
import { waitingLabel } from '@shared/subscription'
import { messagePreview } from '../lib/message-preview'
import { rowTime } from '../lib/sidebar-layout'
import type { BotStore } from '../store/bot-store'
import { ptBackend, ptFolder } from '../lib/textos-pt'

/** Barra05: o tamanho do rosto por altura na árvore (linha de 48, 44 e 40 px); Agentes e a sala usam o do topo. */
export const FACE_BY_LEVEL = [36, 32, 28] as const
export const faceSize = (level: number) => FACE_BY_LEVEL[Math.min(Math.max(level, 1), 3) - 1]!

/** A etiqueta ao lado do nome: o texto e o nível da cor (`l1`…`l3` nas equipes, `l0` em Agentes). */
export interface RowTag { text: string; tone: 'l0' | 'l1' | 'l2' | 'l3' }

/**
 * Barra05 (docs/reference/redesenho/components/TeamGroup): a linha de um bot é uma linha de conversa em duas linhas.
 * Rosto com o estado no próprio quadro (sem pontinhos nem "+N"); na primeira linha o nome e a etiqueta em pílula, com a
 * hora da última mensagem na ponta (na cor de destaque com não lidas, sem contador); na segunda, a última mensagem.
 * `level`: a altura na árvore (1 no topo de Equipes e em Agentes). Um canal fica numa linha só, com `#`.
 */
export const BotRow = memo(function BotRow({ bot, store, compact, usingComputer, level = 1, tag = null, tone = 'l0', lead = false }: {
  bot: Bot
  store: BotStore
  compact: boolean
  usingComputer: boolean
  /** Canais ignoram (kept for the ghost and other callers). */
  members?: Bot[]
  level?: number
  /** A etiqueta e o nível da cor; textos soltos para o `memo` valer (um objeto novo a cada render o derrubava). */
  tag?: string | null
  tone?: RowTag['tone']
  /** O chefe, a linha da equipe: nome em 500. */
  lead?: boolean
}) {
  const t = useT()
  const idioma = useIdioma()
  const unread = bot.unread > 0
  if (isGroup(bot)) {
    return (
      <span className={`bot-row is-channel${unread ? ' is-unread' : ''}`}>
        <span className="bot-row-hash" aria-hidden><Icon name="number" size={15} /></span>
        {compact ? null : <>
          <span className="bot-row-name" dir="auto">{bot.name.toLocaleLowerCase('pt-BR')}</span>
          {/* Dono, 05/11/2026: canal com não lida mostra a hora na cor de destaque, igual aos bots; sem não lida, nada. */}
          {unread ? <span className="bot-row-time">{rowTime(bot.lastAt)}</span> : null}
        </>}
      </span>
    )
  }
  const size = faceSize(level)
  return (
    <span className={`bot-row is-two l${Math.min(Math.max(level, 1), 3)}${lead ? ' is-lead' : ''}${unread ? ' is-unread' : ''}`}>
      <span className="bot-row-face"><BotAvatar bot={bot} size={size} state /></span>
      {compact ? null : <>
        <span className="bot-row-line">
          <span className="bot-row-name" dir="auto">{spaceBotLabel(bot, store.spaces)}</span>
          {tag ? <span className={`bot-row-tag tag-${tone}`} title={tag}>{tag}</span> : null}
        </span>
        <span className="bot-row-end">
          {usingComputer ? <Icon name="cursorarrow.motionlines" size={11} color="var(--accent)" title={t('navegacao.barra.usandoComputador')} /> : null}
          {bot.status === 'error' ? <Icon name="exclamationmark.triangle" size={11} color="var(--danger)" title={t('navegacao.barra.principal.erro')} /> : null}
          {/* runtime-audiencia-01: sem runtime aqui, sem hora (lastAt não é tempo de trabalho). */}
          {isUnavailable(bot) ? <span className="state-unknown" role="img" aria-label={UNAVAILABLE_TEXT} title={UNAVAILABLE_TEXT} /> : <span className="bot-row-time">{rowTime(bot.lastAt)}</span>}
        </span>
        <span className="bot-row-message" dir="auto" title={isWorking(bot) ? activityOriginal(bot) || undefined : undefined}>{rowDescription(t, idioma, bot, store)}</span>
      </>}
    </span>
  )
})

/** The second line: the last message, or what the bot is doing or asking for. Also the row's VoiceOver description. */
export function rowDescription(t: Tradutor, idioma: AppLanguage, bot: Bot, store: BotStore): string {
  if (isUnavailable(bot)) return UNAVAILABLE_TEXT
  if (bot.status === 'waiting') return waitingLabel(t, idioma, bot)
  if (onlyQuestionsPending(bot)) return t('navegacao.barra.pergunta')
  // Uma autorização só: o pedido sai do estado; o texto do Hub ("Needs approval: …") não é lido (MX-147).
  if (needsInput(bot)) return t('navegacao.barra.aprovacao')
  if (isWorking(bot)) return activityText(t, idioma, bot) || t('navegacao.barra.trabalhando')
  if (bot.status === 'error') return messagePreview(bot.lastMessage ?? t('navegacao.janela.erro'))
  return messagePreview(bot.lastMessage ?? (isGroup(bot) ? t.plural('navegacao.barra.bots', bot.members.length) : `${ptBackend(store.backendName(bot.backend))} · ${ptFolder(folderName(bot))}`))
}

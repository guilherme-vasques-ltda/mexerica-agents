import { useId, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import type { Bot } from '@shared/models'
import { onlyQuestionsPending } from '@shared/fork-owner-question'
import { BotAvatar } from '../../../components/Avatar'
import { useEstadoMexeriquinha } from '../../../fork/avatars/Avatar'
import { rowDescription } from '../../BotRow'
import { Icon } from '../../../components/Icon'
import { useEscape } from '../../../components/Overlay'
import { useModel } from '../../../lib/observable'
import { linhaDoPedido, pedidoAberto, principalEmpty, principalStatus } from '../../../lib/principal'
import { sameRef } from '../../../store/app-model'
import type { BotStore } from '../../../store/bot-store'
import { useApp } from '../../../store/context'
import { donnaTexts as texts } from './texts'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { openPrincipal } from './open-principal'
import { activityText } from '../../../lib/fork-atividade'
import './donna.css'

export function DonnaCard({ store, compact, many, edit, newSession, createSession, startSession, onSessions, onActivate, listEmpty = false }: {
  store: BotStore
  onActivate?: () => void
  /** Passo 12: dois cliques no Berga abrem o painel direito em Sessões (gancho do executor B). */
  onSessions?: (bot: Bot) => void
  compact: boolean
  many: boolean
  edit: (bot: Bot) => void
  newSession: (bot: Bot) => void
  /** UI-01: o Hub cria uma extra para este bot (ChatWindow `canStartSession`) e o pedido de criar. */
  createSession: (bot: Bot, store: BotStore) => boolean
  startSession: (bot: Bot) => void
  /** Auditoria D249, F7: whole list empty; the conversation side says it, so no "Defina a bot principal". */
  listEmpty?: boolean
}) {
  useModel(store)
  const identity = useModel(store.principal)
  const bot = identity.id ? store.bots.get(identity.id) : undefined
  const online = store.connection.kind === 'online' && !store.mismatch
  const [menu, setMenu] = useState(false)
  const menuId = useId()
  const menuButton = useRef<HTMLButtonElement>(null)
  const closeMenu = () => { setMenu(false); menuButton.current?.focus() }
  const menuLayer = useRef<HTMLDivElement>(null)
  const t = useT()
  const idioma = useIdioma()
  useEscape(menu, closeMenu, menuLayer)

  // D366/D397: o Berga é fixo; o app não escolhe nem troca a principal (o Hub recusa).
  if (!bot) {
    return <div className={`donna-empty idioma-superficie ${compact ? 'compact' : ''}`} {...superficieIdioma(idioma)} role="region" aria-label={many ? t('navegacao.barra.donna.principalEm', { hub: store.hostName }) : t('navegacao.janela.botsPrincipal')}>
      {many && !compact ? <span className="donna-computer">{store.hostName}</span> : null}
      {onActivate ? <button disabled={!online || store.hello?.spaceLifecycleContract !== 1} onClick={onActivate}>{t('spaces.sheet.ativar')}</button> : null}
      {!compact && !listEmpty && !onActivate ? <p role="status">{principalEmpty(t, identity, online, !!store.mismatch)}</p> : null}
      {compact ? <span title={principalEmpty(t, identity, online, !!store.mismatch)} aria-label={principalEmpty(t, identity, online, !!store.mismatch)}><Icon name="person.crop.circle" size={28} /></span> : null}
      {!compact && online && identity.phase === 'error' ? <button onClick={() => identity.refresh()}>{t(texts.retry)}</button> : null}
    </div>
  }

  return <DonnaCentral bot={bot} store={store} compact={compact} many={many} online={online} menu={menu} menuButton={menuButton} menuLayer={menuLayer} menuId={menuId} openMenu={() => setMenu((value) => !value)} closeMenu={closeMenu} edit={edit} newSession={newSession} createSession={createSession} startSession={startSession} onSessions={onSessions} />
}

/**
 * Barra05 (dono, 05/11/2026; SidebarHeader, "Donna, central"): uma linha de 60px na largura da barra. Rosto oficial de
 * 48px à esquerda, com os quatro estados no rosto e, trabalhando, um halo que respira; ao lado, Donna com o nome do
 * space, o estado com um ponto e a linha de apoio (novidades na cor de destaque, a atividade, ou a última mensagem).
 * Sem estrela, sem número e sem selinhos. Aberta, fundo `selected` sem anel.
 */
function DonnaCentral({ bot, store, compact, many, online, menu, menuButton, menuLayer, menuId, openMenu, closeMenu, edit, newSession, createSession, startSession, onSessions }: {
  onSessions?: (bot: Bot) => void
  bot: Bot
  store: BotStore
  compact: boolean
  many: boolean
  online: boolean
  menu: boolean
  menuButton: React.RefObject<HTMLButtonElement | null>
  menuLayer: React.RefObject<HTMLDivElement | null>
  menuId: string
  openMenu: () => void
  closeMenu: () => void
  edit: (bot: Bot) => void
  newSession: (bot: Bot) => void
  /** UI-01: o Hub cria uma extra para este bot (ChatWindow `canStartSession`) e o pedido de criar. */
  createSession: (bot: Bot, store: BotStore) => boolean
  startSession: (bot: Bot) => void
}) {
  const app = useApp()
  const t = useT()
  const idioma = useIdioma()
  const superficie = superficieIdioma(idioma)
  const ref = { computerId: store.computer.id, hubId: store.hello?.hostId, botId: bot.id, ...(store.spaceId ? { spaceId: store.spaceId } : {}) }
  const pending = store.allEntries(bot.id).some((entry) => entry.kind === 'permission' && entry.data.status === 'pending')
  const status = principalStatus(t, idioma, bot, online, pending, !!store.mismatch)
  const real = useEstadoMexeriquinha(bot)
  const state = real === 'dormindo' ? 'sleep' : real === 'trabalhando' ? 'work' : real === 'precisa-resposta' ? 'listen' : 'idle'
  // Sem conexão, indisponível ou com erro, a linha do estado diz isso (o texto de sempre); senão, os quatro estados.
  const plain = online && !store.mismatch && status.tone === 'normal'
  // R1 do #575: o estado sai do catálogo ao desenhar (o principal, masculino, D480).
  // Uma autorização só: pedindo você, uma linha só e clara ("Precisa da sua aprovação"), sem repetir embaixo.
  // O pedido vem do estado (status e cartões), nunca do texto da atividade.
  const pedido = linhaDoPedido(status, online, !!store.mismatch, pedidoAberto(bot, pending, onlyQuestionsPending(bot)))
  const ask = pedido ? t(pedido === 'pergunta' ? 'navegacao.barra.pergunta' : 'navegacao.barra.aprovacao') : null
  const stateText = ask ?? (plain ? t(`navegacao.barra.donna.estado.${state}`) : status.text)
  const light = !plain ? (status.tone === 'unknown' ? 'unknown' : status.tone === 'danger' ? 'error' : 'offline') : state
  const space = many ? store.hostName : store.spaces?.spaces.find((s) => s.id === (store.spaceId ?? store.spaces?.personalSpaceId))?.name || t('navegacao.barra.donna.pessoal')
  const news = bot.unread > 0 ? t.plural('navegacao.barra.donna.novidades', bot.unread) : null
  // A atividade traduzida pelo código só vale trabalhando; pedindo você, o pedido vem do estado (MX-147, #712).
  const support = news ?? (ask ? null : status.secondary || (state === 'work' && activityText(t, idioma, bot) ? activityText(t, idioma, bot) : rowDescription(t, idioma, bot, store)))
  const selected = sameRef(ref, app.selection)
  const label = [bot.name, t('navegacao.barra.donna.principalSelo'), many ? store.hostName : null, stateText.toLocaleLowerCase(), news ? t.plural('navegacao.barra.donna.novidadesCurto', bot.unread) : null, !news ? support : null].filter(Boolean).join(', ')
  return <section {...superficie} className={`donna-card is-central idioma-superficie ${compact ? 'compact' : ''} ${selected ? 'selected' : ''}${state === 'work' && plain ? ' is-working' : ''}`} aria-label={many ? t('navegacao.barra.donna.botPrincipalEm', { hub: store.hostName }) : t('navegacao.janela.botsPrincipal')}>
    <button className="donna-open" aria-label={label} aria-current={selected || undefined} title={label}
      onClick={() => openPrincipal(app, ref)}
      onDoubleClick={onSessions ? () => onSessions(bot) : undefined}
      onKeyDown={(event) => { if (event.key === 'Enter' && event.ctrlKey && onSessions) { event.preventDefault(); onSessions(bot) } }}
      onContextMenu={(event) => { event.preventDefault(); openMenu() }}>
      <span className="donna-avatar"><BotAvatar bot={bot} size={compact ? 36 : 48} state /></span>
      {!compact ? <span className="donna-text">
        <span className="donna-name"><bdi dir="auto">{bot.name}</bdi><span className="donna-space">{space}</span></span>
        <span className="donna-status" data-state={light} role="status">{stateText}</span>
        {support ? <span className={`donna-support${news ? ' is-new' : ''}`} dir={news ? undefined : 'auto'}>{support}</span> : null}
      </span> : null}
    </button>
    <button ref={menuButton} className="donna-menu-button" aria-label={many ? t('navegacao.barra.donna.acoesEm', { bot: bot.name, hub: store.hostName }) : t('navegacao.barra.donna.acoes', { bot: bot.name })} title={t('navegacao.barra.donna.acoes', { bot: bot.name })}
      aria-haspopup="menu" aria-expanded={menu} aria-controls={menu ? menuId : undefined} onClick={openMenu}>
      <Icon name="ellipsis" size={compact ? 12 : 16} />
    </button>
    {menu ? createPortal(<>
      <div className="donna-menu-dismiss" onClick={closeMenu} />
      <div ref={menuLayer} className="donna-menu menu-panel idioma-superficie" {...superficie} role="menu" id={menuId} aria-label={t('navegacao.barra.donna.acoes', { bot: bot.name })}
        style={{ top: Math.min(menuButton.current?.getBoundingClientRect().bottom ?? 80, window.innerHeight - 250), left: Math.min(menuButton.current?.getBoundingClientRect().left ?? 10, window.innerWidth - 272) }}
        onKeyDown={(event) => {
          const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')]
          if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
            event.preventDefault()
            const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
            buttons[(index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length]?.focus()
          } else if (event.key === 'Tab') closeMenu()
        }}>
        <button className="menu-row" role="menuitem" disabled title={t(texts.unavailable)}>{t(texts.tasks)}<span className="donna-menu-detail">{t(texts.unavailable)}</span></button>
        <button className="menu-row" role="menuitem" autoFocus disabled={!online} onClick={() => { closeMenu(); store.markAllRead(bot.id) }}>{t(texts.read)}</button>
        <button className="menu-row" role="menuitem" disabled={!online} onClick={() => { closeMenu(); edit(bot) }}>{t(texts.edit)}</button>
        {createSession(bot, store) ? <button className="menu-row" role="menuitem" disabled={!online} onClick={() => { closeMenu(); startSession(bot) }}>{t(texts.session)}</button> : null}
        <button className="menu-row" role="menuitem" disabled={!online} onClick={() => { closeMenu(); newSession(bot) }}>{t(texts.restart)}</button>
      </div>
    </>, document.body) : null}
  </section>
}

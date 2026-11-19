import { spaceAllows, type SpaceCapability } from '../../../lib/fork-space-capabilities'
import { interactive } from '../../../lib/fork-modal-stack'
import { useEncerrar } from '../conversa/arquivar/Encerrada'
import { sessionMenuKey } from '../sessoes/menu'
import { useEffect, useRef, useState, type Ref } from 'react'
import { isGroup, type Bot, type Entry } from '@shared/models'
import { BotAvatar } from '../../../components/Avatar'
import { Icon } from '../../../components/Icon'
import { useModel } from '../../../lib/observable'
import { usePref } from '../../../lib/prefs'
import { rowTime } from '../../../lib/sidebar-layout'
import { useStore } from '../../../store/context'
import { useScreenViewed } from '../../../store/analytics'
import type { SidePanelTab } from '../../../store/fork-sessions'
import { RoutinesView } from '../../routines/RoutinesView'
import { HoldButton } from '../../thread/cards/HoldButton'
import { sortedOptions } from '../../thread/PermissionCard'
import { useOpenBotSettings } from '../bot-settings/bot-settings-entry'
import { canRemoveMember, channelDraft, leaderOf } from '../canal/canal'
import { Fold } from '../painel-direito/Fold'
import { panelPrefs } from '../painel-direito/prefs'
import { LDot } from '../sessoes/LDot'
import { BOT_STATE_LABEL, botState, nowSummary, SESSION_LABEL, sessionOf, sessionTitle, type HubSession, type SessionState } from '../sessoes/estado'
import type { ChatSessions } from '../sessoes/useChatSessions'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import { activityText } from '../../../lib/fork-atividade'
import { comNos, no } from '../../settings/rotulo'
import type { ChaveTexto } from '../../../lib/fork-i18n'
import { genuiChoiceOf } from '../genui/choice-entry'
import { messagePreview } from '../../../lib/message-preview'
import { sessionDetail, type DetailState, type SessionDetail, type SessionNeed } from '../sessoes/detalhe'
import { abaQueSumiu, currentTab, tabKeyTarget, visibleTabs, type PanelTabDef } from './abas'
import { AbaWhatsapp, useCaixa, type WaControle } from '../whatsapp/MiniWhatsapp'
import { mostraAba } from '../whatsapp/caixa'
import type { BotWhatsapp } from '../whatsapp/regras'
import '../../bots/bots.css'
import './bot-details-panel.css'

export type DetailsTab = SidePanelTab

type NewSession = { run: () => void; busy: boolean }

interface PanelProps {
  botId: string
  /** O painel está à vista (solto aberto ou fixo). */
  active: boolean
  routineId: string | null
  tab: DetailsTab
  setTab: (tab: DetailsTab) => void
  sessions: ChatSessions
  openSession: string | null
  onOpenSession: (id: string | null) => void
  /** UI-01: "Nova sessão" (ThreadView `useNewSession`); null quando o Hub não cria ou a conversa não tem extras. */
  onNewSession: NewSession | null
  pinned: boolean
  onTogglePin: () => void
  editGroup: () => void
  onCall?: (() => void) | null
  /** WhatsApp R5c: a conversa aberta e o Expandir, guardados na conversa do bot (ThreadView). */
  whatsapp: WaControle
}

/**
 * Barra05 (passo 15): a mesa no painel direito. Bot: cabeçalho (engrenagem = Configurações do bot, com a
 * Memória dentro; alfinete), Ligar, Agora recolhível e as abas Sessões e Rotinas. Canal: Sessões, Membros e
 * Configurações. Os filhos ficam soltos para o conteúdo chegar em sequência ao abrir (painel-direito.css).
 */
export function DetailsPanel(props: PanelProps) {
  const store = useStore()
  const bot = store.bots.get(props.botId) ?? null
  // Monta antes de abrir (PainelDireito): a visita conta só quando o painel está à vista.
  useScreenViewed(props.active ? 'bot_details' : null)
  if (!bot) return null
  return isGroup(bot) ? <ChannelPanel channel={bot} {...props} /> : <BotPanel bot={bot} {...props} />
}

// As abas de cada painel (D335): uma aba extra condicional entra só aqui, com `visible(bot)`, e o corpo dela no
// `body` do painel. A aba guardada que sumir volta para a primeira (Sessões).
// WhatsApp R5c: a aba só para bot com número conectado (a leitura `donnaWhatsapp` do Hub). Nada na barra esquerda.
type BotTabCtx = { bot: Bot; wa: BotWhatsapp | null; plugin: boolean }
// `chave` é a chave do catálogo (bots.painel.aba.*); o texto sai no idioma do app em PanelTabs.
const BOT_TABS: PanelTabDef<BotTabCtx>[] = [{ id: 'sessions', chave: 'bots.painel.aba.sessoes' }, { id: 'routines', chave: 'bots.painel.aba.rotinas' }, { id: 'whatsapp', chave: 'bots.painel.aba.whatsapp', visible: ({ wa, plugin }) => mostraAba(plugin, wa) }]
const CHANNEL_TABS: PanelTabDef<Bot>[] = [{ id: 'sessions', chave: 'bots.painel.aba.sessoes' }, { id: 'members', chave: 'bots.painel.aba.membros' }, { id: 'settings', chave: 'bots.painel.aba.configuracoes' }]

/**
 * O tablist do painel: barra cheia, N abas de largura igual (dono, 07/11); ←/→/Home/End movem a aba e o foco. `dots`:
 * o ponto de novidade (não lidas do WhatsApp; o número vai no `aria-label`). Faltando lugar, o rótulo corta com
 * reticências (o `title` mostra inteiro) e o número e o ponto ficam sempre à vista.
 */
function PanelTabs<C>({ ref, label, tabs, current, counts, dots, onSelect }: { ref?: Ref<HTMLDivElement>; label: string; tabs: PanelTabDef<C>[]; current: string; counts?: Record<string, number>; dots?: Record<string, string>; onSelect: (id: string) => void }) {
  const store = useStore()
  const capability = (id: string): SpaceCapability => id === 'routines' ? 'routines' : id === 'sessions' ? 'sessions' : id === 'whatsapp' ? 'connectors' : 'channels'
  const allowed = (id: string) => spaceAllows(store.spaceId, capability(id))
  const refs = useRef(new Map<string, HTMLButtonElement>())
  const tr = useT()
  const ids = tabs.filter(t => allowed(t.id)).map((t) => t.id)
  // O rótulo é uma chave sem parâmetros (BOT_TABS/CHANNEL_TABS).
  const nome = (t: PanelTabDef<C>) => (tr as unknown as (chave: ChaveTexto) => string)(t.chave as ChaveTexto)
  return (
    <div ref={ref} className="rp-seg" role="tablist" aria-label={label}>
      {tabs.map((t) => (
        <button key={t.id} data-tab={t.id} disabled={!allowed(t.id)} role="tab" aria-selected={t.id === current} tabIndex={t.id === current ? 0 : -1} aria-label={dots?.[t.id]}
          title={!allowed(t.id) ? tr('spaces.selector.unavailable') : `${nome(t)}${counts?.[t.id] !== undefined ? ` ${counts[t.id]}` : ''}`}
          ref={(n) => { if (n) refs.current.set(t.id, n); else refs.current.delete(t.id) }}
          onClick={() => onSelect(t.id)}
          onKeyDown={(e) => {
            const next = tabKeyTarget(ids, current, e.key)
            if (!next) return
            e.preventDefault()
            onSelect(next)
            refs.current.get(next)?.focus()
          }}>
          <span className="tx"><span className="l">{nome(t)}</span>{counts?.[t.id] !== undefined ? <span className="n">{counts[t.id]}</span> : null}</span>
          {dots?.[t.id] ? <span className="wdot" aria-hidden /> : null}
        </button>
      ))}
    </div>
  )
}

function PinButton({ pinned, onTogglePin }: { pinned: boolean; onTogglePin: () => void }) {
  const t = useT()
  return (
    <button className="rp-icon" aria-pressed={pinned} onClick={onTogglePin}
      title={t(pinned ? 'bots.painel.soltarDica' : 'bots.painel.fixarDica')} aria-label={t(pinned ? 'bots.painel.soltar' : 'bots.painel.fixar')}>
      <Icon name="pin" size={13} scaled={false} />
    </button>
  )
}

function BotPanel({ bot, routineId, tab, setTab, sessions, openSession, onOpenSession, onNewSession, pinned, onTogglePin, onCall, whatsapp, active }: PanelProps & { bot: Bot }) {
  const t = useT()
  // Fatia 3 (D454): cabeçalho, Agora, abas e sessões são traduzidos (idioma e direção do app); Rotinas e WhatsApp
  // ainda são legado e ficam fora da superfície.
  const superficie = { ...superficieIdioma(useIdioma()), className: 'idioma-superficie' }
  const cabeca = { ...superficieIdioma(useIdioma()), className: 'rp-head idioma-superficie' }
  const store = useStore()
  const caixa = useCaixa(store)
  const wa = caixa.bot(bot.id)
  const unread = wa?.inbox?.unread ?? 0
  const openSettings = useOpenBotSettings()
  const principal = useModel(store.principal)
  const state = sessions.combined
  const role = principal.id === bot.id ? t('bots.painel.principal') : bot.title?.trim() || bot.description?.trim() || ''
  const model = `${store.backendName(bot.backend)}${bot.model ? ` · ${bot.model}` : ''}`
  const tabs = visibleTabs(BOT_TABS, { bot, wa, plugin: caixa.leitura.leitura?.plugin.enabled === true })
  const current = currentTab(tabs, tab)
  // A3: a aba guardada sumiu (WhatsApp com o plugin desligado ou sem número): grava Sessões como seleção, para a aba não
  // voltar selecionada sozinha; se o foco caiu (estava nela), vai para Sessões. Só com a leitura do Hub em mãos.
  const lida = caixa.leitura.leitura !== null || caixa.leitura.erroLeitura !== null
  const sumiu = lida ? abaQueSumiu(tabs, tab) : null
  const tablist = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!sumiu) return
    setTab(sumiu as DetailsTab)
    const doc = tablist.current?.ownerDocument
    if (active && doc && (doc.activeElement === doc.body || doc.activeElement === null)) tablist.current?.querySelector<HTMLElement>(`[data-tab="${sumiu}"]`)?.focus({ preventScroll: true })
  }, [sumiu])
  return <>
    <header {...cabeca}>
      <span className="rp-face"><BotAvatar bot={bot} size={44} /></span>
      <div className="rp-who">
        <h2 className="rp-name">{bot.name}</h2>
        <span className="rp-role" dir="auto" title={[model, role].filter(Boolean).join(' · ')}>{[model, role].filter(Boolean).join(' · ')}</span>
        <span className={`rp-state s-${store.isOffline ? 'offline' : state}`}>{store.isOffline ? t('bots.painel.semConexao') : t(BOT_STATE_LABEL[state])}</span>
      </div>
      <div className="rp-acts">
        <button className="rp-icon" title={t('bots.painel.configuracoesDica', { bot: bot.name })} aria-label={t('bots.painel.configuracoes', { bot: bot.name })} onClick={() => openSettings?.(store, bot.id)}>
          <Icon name="gearshape" size={14} scaled={false} />
        </button>
        <PinButton pinned={pinned} onTogglePin={onTogglePin} />
      </div>
    </header>
    {onCall ? <button className="rp-call" disabled={store.isOffline || !!store.mismatch} onClick={onCall}><Icon name="phone" size={13} scaled={false} />{t('bots.painel.ligar')}</button> : null}
    <div {...superficie} style={{ display: 'contents' }}><NowCard bot={bot} sessions={sessions} onOpenSession={onOpenSession} />
    <PanelTabs ref={tablist} label={t('bots.painel.doBot')} tabs={tabs} current={current} counts={{ sessions: sessions.open.length }} onSelect={(id) => setTab(id as DetailsTab)}
      dots={unread > 0 ? { whatsapp: t.plural('bots.painel.whatsappNaoLidas', unread) } : undefined} /></div>
    <div className={`rp-body${current === 'whatsapp' ? ' wa-body' : ''}`} role="tabpanel" aria-label={t(current === 'routines' ? 'bots.painel.aba.rotinas' : current === 'whatsapp' ? 'bots.painel.aba.whatsapp' : 'bots.painel.aba.sessoes')}>
      {!spaceAllows(store.spaceId, current === 'routines' ? 'routines' : current === 'whatsapp' ? 'connectors' : 'sessions') ? <p role="status">{t('spaces.selector.unavailable')}</p> : current === 'whatsapp' && wa
        ? <AbaWhatsapp bot={bot} wa={wa} controle={whatsapp} />
        : current === 'routines'
        ? <RoutinesView botId={bot.id} initialId={routineId} />
        : <div {...superficie} style={{ display: 'contents' }}><SessionTree chat={bot} sessions={sessions} openSession={openSession} onOpenSession={onOpenSession} onNew={onNewSession} /></div>}
    </div>
  </>
}

/** O Agora: o que roda e o que pede você, com Aprovar (segurar, como no cartão) e Ver. Recolhido, o resumo. */
function NowCard({ bot, sessions, onOpenSession }: { bot: Bot; sessions: ChatSessions; onOpenSession: (id: string | null) => void }) {
  const t = useT()
  const idioma = useIdioma()
  const store = useStore()
  const [open, setOpen] = usePref(panelPrefs.nowOpen)
  const all = [{ id: bot.id, title: t('bots.painel.conversaPrincipal'), principal: true }, ...sessions.open.map((s) => ({ id: s.id, title: sessionTitle(s, t), principal: false }))]
  const states = all.map((s) => sessions.state(s.id))
  const running = all.filter((_, i) => states[i] === 'running')
  const pending = store.allEntries(bot.id).filter((e) => e.kind === 'permission' && e.data.status === 'pending')
  // MB-1b: a pergunta ao dono também pede você; responder é escrever na conversa, então aqui só "Ver".
  const questions = bot.pendingQuestions.filter((q) => q.chatId === bot.id)
  // Dono, 07/11: o cartão de escolha pendente também pede você (conta no resumo pela derivação de sessoes/estado.ts).
  const choices = store.allEntries(bot.id).filter((e) => genuiChoiceOf(e)?.hub.state === 'pending')
  const summary = nowSummary(states, t)
  const titleOf = (id: string) => all.find((s) => s.id === id)?.title ?? t('bots.painel.conversaPrincipal')
  return (
    <section className={`rp-now${open ? '' : ' shut'}`} aria-label={t('bots.painel.agora')}>
      <button className="now-head" aria-expanded={open} title={t(open ? 'bots.painel.recolherAgora' : 'bots.painel.expandirAgora')} onClick={() => setOpen(!open)}>
        <span className="now-live" aria-hidden />{t('bots.painel.agora')}
        {open ? null : <span className="now-sum">
          {summary.running ? <><LDot state="running" /><span>{t.plural('sessions.now.running', summary.running)}</span></> : null}
          {summary.needs ? <><LDot state="needsYou" /><span>{t.plural('sessions.now.needsYou', summary.needs)}</span></> : null}
          {!summary.running && !summary.needs ? <span>{t('sessions.now.idle')}</span> : null}
        </span>}
        <span className="chev" aria-hidden><Icon name="chevron.down" size={10} weight="semibold" scaled={false} /></span>
      </button>
      <Fold open={open} className="now-body">
        {running.map((s) => (
          <button key={s.id} className="now-item" title={t('bots.painel.abrir', { titulo: s.title })} onClick={() => onOpenSession(s.principal ? null : s.id)}>
            <LDot state="running" big /><span className="now-text"><b dir="auto">{s.title}</b>{activityText(t, idioma, bot) ? <> · <bdi>{activityText(t, idioma, bot)}</bdi></> : null}</span>
          </button>
        ))}
        {pending.map((e) => <PendingItem key={e.id} entry={e} where={titleOf(sessionOf(e, bot.id))} onView={() => onOpenSession(sessionOf(e, bot.id) === bot.id ? null : sessionOf(e, bot.id))} />)}
        {questions.map((q) => (
          <div key={q.entryId} className="now-item need">
            <Icon name="text.bubble" size={13} scaled={false} />
            <span className="now-text">{t('bots.painel.pergunta', { sessao: titleOf(q.sessionId ?? bot.id), texto: q.text })}</span>
            <span className="now-actions"><button className="rp-mini" onClick={() => onOpenSession(q.sessionId ?? null)}>{t('sessions.now.view')}</button></span>
          </div>
        ))}
        {choices.map((e) => (
          <div key={e.id} className="now-item need">
            <Icon name="text.bubble" size={13} scaled={false} />
            <span className="now-text">{t('sessions.now.choice', { sessao: titleOf(sessionOf(e, bot.id)), texto: messagePreview(e.data.text ?? '') })}</span>
            <span className="now-actions"><button className="rp-mini" onClick={() => onOpenSession(sessionOf(e, bot.id) === bot.id ? null : sessionOf(e, bot.id))}>{t('sessions.now.view')}</button></span>
          </div>
        ))}
        {!running.length && !pending.length && !questions.length && !choices.length ? <span className="now-free">{t('bots.painel.livre')}</span> : null}
      </Fold>
    </section>
  )
}

function PendingItem({ entry, where, onView }: { entry: Entry; where: string; onView: () => void }) {
  const store = useStore()
  const t = useT()
  const allow = sortedOptions(entry.data.options ?? []).find((o) => o.kind.startsWith('allow'))
  const answering = store.answering.has(entry.id)
  return (
    <div className="now-item need">
      <Icon name="hand.raised" size={13} scaled={false} />
      <span className="now-text">{entry.data.title ? t('bots.painel.pedeVoceTitulo', { sessao: where, titulo: entry.data.title }) : t('bots.painel.pedeVoce', { sessao: where })}</span>
      <span className="now-actions">
        {allow ? <HoldButton primary label={t('bots.painel.aprovar')} disabled={answering || store.isOffline} onConfirm={() => store.respond(entry, allow.optionId)}>{t('bots.painel.aprovar')}</HoldButton> : null}
        <button className="rp-mini" onClick={onView}>{t('sessions.now.view')}</button>
      </span>
    </div>
  )
}

/** A árvore de sessões: a conversa principal em cima e, presas por uma linha guia, as sessões abertas. Cada linha
 * diz título, estado, o que pede você ou a última interação (proposta do dono, 07/11; sessoes/detalhe.ts). */
function SessionTree({ chat, sessions, openSession, onOpenSession, onNew = null }: {
  chat: Bot; sessions: ChatSessions; openSession: string | null; onOpenSession: (id: string | null) => void; onNew?: NewSession | null
}) {
  const t = useT()
  const store = useStore()
  const tree = useRef<HTMLDivElement>(null)
  const menu = useEncerrar(store, chat, true, () => {
    const root = tree.current?.querySelector<HTMLButtonElement>('.srow.root') ?? null
    const main = document.querySelector<HTMLButtonElement>('.ctab.main')
    requestAnimationFrame(() => {
      if (document.activeElement !== document.body) return
      const target = interactive(root) ? root : interactive(main) ? main : null
      target?.focus({ preventScroll: true })
    })
  })
  const menuProps = (id: string, title: string) => ({
    'aria-haspopup': 'menu' as const,
    'aria-expanded': menu.menuId === id,
    onContextMenu: (e: React.MouseEvent<HTMLButtonElement>) => { e.preventDefault(); e.currentTarget.focus(); menu.abrir(id, e.clientX, e.clientY, title) },
    onKeyDown: (e: React.KeyboardEvent<HTMLButtonElement>) => { sessionMenuKey(e, () => { const r = e.currentTarget.getBoundingClientRect(); menu.abrir(id, r.left, r.bottom, title) }) },
  })
  const [archivedOpen, setArchivedOpen] = useState(false)
  const channel = isGroup(chat)
  const entries = store.allEntries(chat.id)
  const lineOf = (d: SessionDetail) => {
    const what = d.need ? t(NEED_LABEL[d.need.kind], { texto: d.need.text }) : d.last ? (d.last.mine ? t('sessions.detail.you', { texto: d.last.text }) : d.last.text) : t('sessions.detail.empty')
    return <span className="sd"><span className="sdst">{t(DETAIL_LABEL[d.state])}</span><span className="sdtx" dir="auto">{what}</span></span>
  }
  const row = (s: HubSession) => {
    const state: SessionState = sessions.state(s.id)
    const d = sessionDetail(chat, s, state, entries, sessions.known)
    const title = d.title ?? t('sessions.untitled')
    return (
      <button key={s.id} {...menuProps(s.id, title)} className={`srow rich${state === 'archived' ? ' done' : ''}${d.need ? ' need' : ''}${openSession === s.id ? ' on' : ''}`} onClick={() => onOpenSession(s.id)}
        title={t('sessions.tabs.tab', { titulo: title, estado: t(DETAIL_LABEL[d.state]) })} aria-current={openSession === s.id ? 'page' : undefined}>
        <LDot state={d.state === 'queued' ? 'idle' : d.state} />
        <span className="sbody"><span className="sl1"><span className="sn" dir="auto">{title}</span><span className="st">{rowTime(d.at)}</span></span>{lineOf(d)}</span>
      </button>
    )
  }
  // R1-02 (#509): a principal do Hub, com a mesma reserva de título e a hora da última interação.
  const main = sessionDetail(chat, sessions.principal ?? { id: chat.id, title: '', principal: true, lastUsedAt: chat.lastAt }, sessions.main, entries, sessions.known)
  return <>
    {onNew ? (
      // DON-903: sessions.new.label / sessions.new.busy
      <button className="rp-new" disabled={onNew.busy} aria-busy={onNew.busy || undefined} onClick={() => onNew.run()}>
        <span className="pl" aria-hidden><Icon name="plus" size={11} weight="semibold" scaled={false} /></span>{t(onNew.busy ? 'sessions.new.busy' : 'sessions.new.label')}
      </button>
    ) : null}
    {menu.camada}
    <div className="stree" ref={tree}>
      <button {...menuProps(sessions.principal?.id ?? chat.id, channel ? chat.name : main.title ?? t('sessions.untitled'))} className={`srow root rich${main.need ? ' need' : ''}${openSession === null ? ' on' : ''}`} onClick={() => onOpenSession(null)} title={channel ? t('bots.painel.principalCanal') : t('bots.painel.principalCom', { bot: chat.name })}>
        {channel ? <span className="ch-hash sm" aria-hidden><Icon name="number" size={10} weight="semibold" scaled={false} /></span> : <BotAvatar bot={chat} size={18} />}
        <span className="sbody"><span className="sl1"><span className="sn" dir="auto">{channel ? chat.name.toLocaleLowerCase('pt-BR') : main.title ?? t('sessions.untitled')}</span><span className="st">{t('bots.painel.principalSelo')}</span><span className="st">{rowTime(main.at)}</span></span>{lineOf(main)}</span>
      </button>
      {sessions.open.length ? <div className="skids" role="group" aria-label={t('bots.painel.sessoesAbertas')}>{sessions.open.map(row)}</div> : null}
    </div>
    {sessions.archived.length ? <>
      <button className="rp-fold" aria-expanded={archivedOpen} onClick={() => setArchivedOpen(!archivedOpen)}>
        <span className="chev" aria-hidden><Icon name="chevron.down" size={9} weight="bold" scaled={false} /></span>{t('bots.painel.arquivadas', { n: sessions.archived.length })}
      </button>
      <Fold open={archivedOpen} className="skids flat">{sessions.archived.map(row)}</Fold>
    </> : null}
  </>
}

const DETAIL_LABEL = { ...SESSION_LABEL, queued: 'sessions.state.queued' } as const satisfies Record<DetailState, ChaveTexto>
const NEED_LABEL = { permission: 'sessions.detail.needs.permission', question: 'sessions.detail.needs.question', choice: 'sessions.detail.needs.choice' } as const satisfies Record<SessionNeed['kind'], ChaveTexto>

function ChannelPanel({ channel, tab, setTab, sessions, openSession, onOpenSession, pinned, onTogglePin, editGroup }: PanelProps & { channel: Bot }) {
  const store = useStore()
  const t = useT()
  // Fatia 3 (D454): o painel do canal é todo traduzido (cabeçalho, abas, membros, ajustes e sessões).
  const superficie = superficieIdioma(useIdioma())
  const members = store.members(channel)
  const leader = leaderOf(channel)
  const leaderName = leader ? store.bots.get(leader)?.name : null
  const name = channel.name.toLocaleLowerCase('pt-BR')
  const tabs = visibleTabs(CHANNEL_TABS, channel)
  const current = currentTab(tabs, tab)
  return <div className="idioma-superficie" {...superficie} style={{ display: 'contents' }}>
    <header className="rp-head">
      <span className="ch-tile" aria-hidden><Icon name="number" size={18} weight="semibold" scaled={false} /></span>
      <div className="rp-who">
        <h2 className="rp-name">{name}</h2>
        <span className="rp-role" title={channel.description || undefined}>{channel.description?.trim() || t('bots.painel.semDescricao')}</span>
        <span className="rp-meta">{t.plural('bots.painel.bots', members.length)} · {leaderName ? comNos(t('bots.painel.lider', { bot: no('bot') }), { bot: <bdi dir="auto">{leaderName}</bdi> }) : t('bots.painel.semLider')}</span>
      </div>
      <div className="rp-acts">
        <button className="rp-icon" title={t('bots.painel.configuracoesCanal')} aria-label={t('bots.painel.configuracoesCanal')} onClick={() => setTab('settings')}>
          <Icon name="gearshape" size={14} scaled={false} />
        </button>
        <PinButton pinned={pinned} onTogglePin={onTogglePin} />
      </div>
    </header>
    <PanelTabs label={t('bots.painel.doCanal')} tabs={tabs} current={current} counts={{ sessions: sessions.open.length, members: members.length + 1 }} onSelect={(id) => setTab(id as DetailsTab)} />
    <div className="rp-body" role="tabpanel" aria-label={t(current === 'members' ? 'bots.painel.aba.membros' : current === 'settings' ? 'bots.painel.aba.configuracoes' : 'bots.painel.aba.sessoes')}>
      {current === 'members' ? <ChannelMembers channel={channel} members={members} leader={leader} editGroup={editGroup} />
        : current === 'settings' ? <ChannelSettings key={channel.id} channel={channel} members={members} leader={leader} />
        : !spaceAllows(store.spaceId, 'sessions') ? <p role="status">{t('spaces.selector.unavailable')}</p> : <SessionTree chat={channel} sessions={sessions} openSession={openSession} onOpenSession={onOpenSession} />}
    </div>
  </div>
}

function ChannelMembers({ channel, members, leader, editGroup }: { channel: Bot; members: Bot[]; leader: string | null; editGroup: () => void }) {
  const t = useT()
  const store = useStore()
  return <>
    <h3 className="mb-head">{t('bots.painel.pessoas', { n: 1 })}</h3>
    <div className="mb-row me">
      <span className="mb-av" aria-hidden><Icon name="person.fill" size={13} scaled={false} /></span>
      <span className="mt2"><span className="n2">{t('bots.painel.voce')}</span><span className="r2">{t('bots.painel.dono')}</span></span>
    </div>
    <h3 className="mb-head">{t('bots.painel.agentes', { n: members.length })}</h3>
    {members.map((bot) => {
      const state = botState(bot)
      const isLeader = bot.id === leader
      const removable = canRemoveMember(channel, bot.id) && channel.members.length > 1
      return (
        <div key={bot.id} className="mb-item">
          <button className="mb-row" title={t('bots.painel.abrirConversa', { bot: bot.name })} onClick={() => store.setSelection(bot.id)}>
            <BotAvatar bot={bot} size={30} />
            <span className="mt2">
              <span className="n2"><bdi dir="auto">{bot.name}</bdi></span>
              <span className="r2"><LDot state={state} /><span className="r2-text">{[t(BOT_STATE_LABEL[state]), bot.title?.trim()].filter(Boolean).join(' · ')}</span></span>
            </span>
            {isLeader ? <span className="mb-pill lead">{t('bots.painel.liderSelo')}</span> : null}
          </button>
          {channel.members.length > 1 ? (
            <button className="mb-x" disabled={!removable} onClick={() => store.removeMember(channel, bot.id)}
              title={t(isLeader ? 'bots.painel.troqueLider' : 'bots.painel.tirar')} aria-label={isLeader ? t('bots.painel.ehLider', { bot: bot.name }) : t('bots.painel.tirarBot', { bot: bot.name })}>
              <Icon name="xmark" size={10} weight="semibold" scaled={false} />
            </button>
          ) : null}
        </div>
      )
    })}
    <button className="mb-add" onClick={editGroup}>
      <span className="pl" aria-hidden><Icon name="plus" size={12} weight="semibold" scaled={false} /></span>{t('bots.painel.adicionar')}
    </button>
  </>
}

/** Nome e descrição salvam enquanto se digita (com espera curta); o líder salva ao escolher. */
function ChannelSettings({ channel, members, leader }: { channel: Bot; members: Bot[]; leader: string | null }) {
  const store = useStore()
  const t = useT()
  const [name, setName] = useState(channel.name)
  const [description, setDescription] = useState(channel.description ?? '')
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState<string | null>(null)
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const latest = useRef(channel)
  latest.current = channel
  useEffect(() => () => clearTimeout(timer.current), [])
  const save = async (change: Parameters<typeof channelDraft>[1]) => {
    setError(null)
    try {
      await store.updateGroup(channelDraft(latest.current, change))
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }
  const later = (change: { name?: string; description?: string }) => {
    clearTimeout(timer.current)
    timer.current = setTimeout(() => {
      if (change.name !== undefined && !change.name.trim()) return
      void save(change)
    }, 400)
  }
  const choose = async (id: string | null) => {
    if (id === leader || saving) return
    setSaving(id ?? '')
    await save({ leader: id })
    setSaving(null)
  }
  const offline = store.isOffline || !!store.mismatch
  return <>
    <label className="fld"><span>{t('bots.editor.canal.nomePlaceholder')}</span>
      <input value={name} maxLength={40} autoComplete="off" disabled={offline} onChange={(e) => { setName(e.target.value); later({ name: e.target.value.trim() }) }} />
    </label>
    <label className="fld"><span>{t('bots.painel.descricao')}</span>
      <textarea value={description} rows={3} maxLength={200} disabled={offline} onChange={(e) => { setDescription(e.target.value); later({ description: e.target.value }) }} />
    </label>
    <div className="fld" role="radiogroup" aria-label={t('bots.painel.liderCanal')}>
      <span>{t('bots.painel.liderCanal')}</span>
      <p className="fld-hint">{t('bots.painel.liderDica')}</p>
      <button className={`ld-row${leader === null ? ' on' : ''}`} role="radio" aria-checked={leader === null} disabled={offline} onClick={() => { void choose(null) }}>
        <span className="ld-none" aria-hidden><Icon name="person.2" size={12} scaled={false} /></span>
        <span className="mt2"><span className="n2">{t('bots.painel.semLiderTitulo')}</span><span className="r2">{t('bots.painel.todosNaVez')}</span></span>
        <span className="rd" aria-hidden />
      </button>
      {members.map((bot) => (
        <button key={bot.id} className={`ld-row${leader === bot.id ? ' on' : ''}`} role="radio" aria-checked={leader === bot.id} disabled={offline} onClick={() => { void choose(bot.id) }}>
          <BotAvatar bot={bot} size={24} />
          <span className="mt2"><span className="n2"><bdi dir="auto">{bot.name}</bdi></span><span className="r2"><span className="r2-text">{bot.title?.trim() || bot.description?.trim() || ' '}</span></span></span>
          <span className="rd" aria-hidden />
        </button>
      ))}
    </div>
    {error ? <p className="fld-error" role="alert">{error}</p> : null}
  </>
}

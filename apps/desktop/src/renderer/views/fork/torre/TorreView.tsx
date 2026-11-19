// VG-02 (docs/features/mission-board.md; proposta visual torre-controle-bots-nativos-20261103): o Mission Board,
// lugar próprio da faixa (98A), com atalho para Times. Só desenha a fotografia do Hub (`donnaControlTower`) e envia
// intenções de navegação: Abrir, Abrir para responder e Abrir para parar só levam à conversa autorizada, onde ficam o
// cartão original e o controle que já existe. Sem Pausar, sem folha nova de Parar e sem Ver andamento nesta etapa.
import { memo, useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from 'react'
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import {
  countByTone, groupBots, listedBots, openTarget, canOpenToStop, pendingOf, questionsOf, queueOf, statusOf,
  uninformedCount, usageOf, type Observation, type OpenTarget, type TowerBot, type TowerQuestion, type TowerTone,
} from '@shared/fork-torre'
import { isolado } from '@shared/bidi'
import { hora } from './formato'
import type { AppLanguage } from '@shared/fork-idioma'
import { localeDe } from '../../../lib/fork-i18n/index.ts'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import type { Entry } from '@shared/models'
import { parseRoutineReviewResult, type ReviewRoutine } from '@shared/fork-torre-rotinas'
import { editOrigin } from '@shared/composer-segments'
import { useComposerInput } from '../../../lib/use-composer-input'
import { Icon } from '../../../components/Icon'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import { useModel } from '../../../lib/observable'
import type { AppModel } from '../../../store/app-model'
import type { BotStore } from '../../../store/bot-store'
import { StoreContext, useApp } from '../../../store/context'
import { PermissionCard } from '../../thread/PermissionCard'
import { settingsScreen } from '../ajustes/tela'
import { timesScreen } from '../times/navigation'
import { OQueAconteceu, type SummarySave } from './OQueAconteceu'
import { PedidosEntreBots } from './PedidosEntreBots'
import { TrocasDeConta } from './TrocasDeConta'
import { PermissoesDelegadas } from './PermissoesDelegadas'
import { RotinasCriadas } from './RotinasCriadas'
import { torreScreen } from './navigation'
import { answerView, pendingView, questionView, responderOf, type PendingView, type QuestionView, type Responder } from './responder'
import { TowerSession, type TowerError, type TowerSource } from './torre-session'
import { ProjetosLive } from '../projetos/TarefasView'
import { keepDraftsFor, QuestionDrafts, questionDrafts, submitDraft } from './rascunhos-pergunta'
import { activityText } from '../../../lib/fork-atividade'
import './torre.css'

export { torreScreen }

const hm = hora

function pickStore(app: AppModel, computerId: string | null): BotStore | null {
  if (computerId) return app.store(computerId) ?? null
  return app.selectedStore ?? null
}

/** Uma sessão por computador e space enquanto o app está aberto; outro Hub no lugar começa outra. */
const sessions = new Map<string, TowerSession>()
function sessionFor(computerId: string, spaceId: string) {
  const key = JSON.stringify([computerId, spaceId])
  let s = sessions.get(key)
  if (!s || s.phase.kind === 'moved') {
    s = new TowerSession()
    sessions.set(key, s)
  }
  return s
}

export function TorreScreenView() {
  const app = useApp()
  const t = useT()
  const [store] = useState(() => pickStore(app, torreScreen.computerId))
  useModel(store ?? torreScreen)
  useModel(torreScreen)
  const spaceId = torreScreen.spaceId ?? store?.spaces?.personalSpaceId ?? null
  if (!store || !spaceId) {
    return <TorreFrame spaceName={null} computerName={store?.computer.name ?? null}><p className="torre-state">{t('torre.escolhaBot', { nome: isolado(t('torre.nome')) })}</p></TorreFrame>
  }
  // D541/D543: Projetos manuais do Space deste Board (qualquer Space conhecido do Hub); Voltar devolve ao Board.
  const projectsHere = knownSpace(store, spaceId)
  if (torreScreen.section === 'projects' && projectsHere) {
    return <StoreContext.Provider value={store}><ProjetosLive store={store} spaceId={spaceId} spaceName={spaceLabel(store, spaceId, t)} onBack={() => torreScreen.showSection('board')} /></StoreContext.Provider>
  }
  const navigate = (target: OpenTarget) => {
    torreScreen.close(false)
    torreScreen.navigate?.(store, target)
  }
  return (
    <StoreContext.Provider value={store}>
    <TorreLive
      store={store}
      source={store as unknown as TowerSource}
      spaceId={spaceId}
      spaceName={spaceLabel(store, spaceId, t)}
      personalSpaceId={store.spaces?.personalSpaceId ?? null}
      onNavigate={navigate}
      onRetry={() => store.restartStream()}
      onProjects={projectsHere ? () => torreScreen.showSection('projects') : null}
    />
    </StoreContext.Provider>
  )
}

function TorreLive(props: { store: BotStore; source: TowerSource; spaceId: string; spaceName: string; personalSpaceId: string | null; onNavigate: (t: OpenTarget) => void; onRetry: () => void; onProjects: (() => void) | null }) {
  const session = useMemo(() => sessionFor(props.store.computer.id, props.spaceId), [props.store, props.spaceId])
  useModel(session)
  useEffect(() => {
    session.open(props.source, props.spaceId)
    return () => session.close()
  }, [session, props.source, props.spaceId])
  // Captura a origem da fotografia; uma RPC atrasada não confirma decisões em outro Hub/Space/link.
  const snapshot = session.snapshot
  const client = props.source.connectedClient
  const epoch = props.source.principalEpoch
  const current = () => !!snapshot && !!client && !session.stale && !session.notice && session.phase.kind === 'ready'
    && session.snapshot?.hubId === snapshot.hubId && session.snapshot?.spaceId === snapshot.spaceId
    && session.space === snapshot.spaceId && props.source.hello?.hostId === snapshot.hubId
    && props.source.connectedClient === client && props.source.principalEpoch === epoch
  const onReviewRoutine: ReviewRoutine = async (body) => {
    if (!current() || !snapshot || !client || body.hubId !== snapshot.hubId || body.spaceId !== snapshot.spaceId) throw new Error('routine_review_offline')
    const item = snapshot.routineCreations.availability === 'available'
      ? snapshot.routineCreations.value.items.find((item) => item.entryId === body.entryId && item.entryRev === body.entryRev) : null
    if (!item) throw new Error('routine_review_changed')
    const answer = await client.call('donnaRoutineCreationReview', body)
    if (!current()) throw new Error('routine_review_context_changed')
    const result = parseRoutineReviewResult(answer, item, body.choice)
    session.reread()
    return result
  }
  return (
    <TorrePage
      key={JSON.stringify([snapshot?.hubId ?? null, props.spaceId])}
      session={session}
      spaceName={props.spaceName}
      computerName={props.store.computer.name}
      personalSpaceId={props.personalSpaceId}
      onNavigate={props.onNavigate}
      onRetry={props.onRetry}
      onProjects={props.onProjects}
      onTimes={() => timesScreen.show(props.spaceId === props.personalSpaceId ? null : props.spaceId, props.store.computer.id)}
      onUsage={() => settingsScreen.show('usage')}
      responder={responderOf(props.store)}
      drafts={questionDrafts(props.store.computer.id, props.spaceId)}
      onReviewRoutine={current() ? onReviewRoutine : null}
      onSaveSummary={(body) => props.source.connectedClient?.call('donnaResumoDiarioSave', body) ?? Promise.reject(new Error('offline'))}
    />
  )
}

function TorreFrame({ spaceName, computerName, end, children }: { spaceName: string | null; computerName: string | null; end?: ReactNode; children: ReactNode }) {
  const back = useRef<HTMLButtonElement>(null)
  const t = useT()
  const idioma = useIdioma()
  useEffect(() => back.current?.focus({ preventScroll: true }), [])
  // DON-917: a raiz de todo retorno da tela (página, vazio e erro) declara o idioma e a direção.
  return (
    <div className="torre-page idioma-superficie" {...superficieIdioma(idioma)}>
      <header className="torre-header drag">
        <button ref={back} className="torre-back no-drag" aria-label={t('torre.voltar')} title={t('torre.voltar')} onClick={() => torreScreen.close()}>
          <Icon name="arrow.left" size={14} weight="medium" />
        </button>
        <h1 className="torre-title">{t('torre.nome')}</h1>
        {spaceName ? <span className="torre-chip"><span className="torre-chip-key">{t('torre.chip.space')}</span> <bdi>{spaceName}</bdi></span> : null}
        {computerName ? <span className="torre-chip"><Icon name="desktopcomputer" size={12} /> <span className="torre-chip-key">{t('torre.chip.computador')}</span> <bdi>{computerName}</bdi></span> : null}
        <div className="torre-header-end no-drag">{end}</div>
      </header>
      {children}
    </div>
  )
}

export interface TorrePageProps {
  session: Pick<TowerSession, 'phase' | 'snapshot' | 'link' | 'events' | 'offlineSince' | 'notice' | 'stale'>
  spaceName: string
  computerName: string
  personalSpaceId: string | null
  onNavigate: (target: OpenTarget) => void
  onRetry: () => void
  onTimes: () => void
  onUsage: () => void
  /** MB-1: o cartão original e a mesma intenção da conversa; ausente, só Abrir. */
  responder?: Responder | null
  /** MB-1b R3: rascunhos das respostas, fora do componente (sobrevivem a desmontagem e reconexão). */
  drafts?: QuestionDrafts
  /** MB-3: horário e liga/desliga do resumo do dia (RPC do dono); ausente, só leitura. */
  onSaveSummary?: ((body: SummarySave) => Promise<unknown>) | null
  /** D541/D543: abre os Projetos deste Space; ausente (Space desconhecido), sem botão. */
  onProjects?: (() => void) | null
  /** D544: decisão posterior à criação; ausente, somente leitura. */
  onReviewRoutine?: ReviewRoutine | null
}


/** A página inteira a partir do estado da sessão; sem I/O (a prévia de provas a usa com dados sintéticos). */
export function TorrePage(p: TorrePageProps) {
  const { session } = p
  const t = useT()
  const idioma = useIdioma()
  const snapshot = session.snapshot
  const stale = session.stale && !!snapshot
  const [selected, setSelected] = useState<string | null>(null)
  const bots = snapshot?.bots ?? []
  // MB-1: o que foi respondido daqui continua à vista (só nesta tela, em memória) depois que o Hub o tira da lista.
  const [answeredHere, setAnsweredHere] = useState<{ entryId: string; conversationId: string; botId: string; name: string }[]>([])
  const responder = p.responder ? {
    ...p.responder,
    respond: (entry: Entry, option: string | null) => {
      const ref = bots.flatMap((b) => (pendingOf(b) ?? []).map((r) => ({ b, r }))).find((x) => x.r.entryId === entry.id)
      if (ref) setAnsweredHere((list) => list.some((x) => x.entryId === entry.id) ? list : [...list, { entryId: entry.id, conversationId: ref.r.conversation.conversationId, botId: ref.b.botId, name: ref.b.name }])
      p.responder!.respond(entry, option)
    },
  } : null
  const drafts = useMemo(() => p.drafts ?? new QuestionDrafts(), [p.drafts])
  // MB-1b R3: só numa fotografia ao vivo o rascunho de pergunta que saiu da lista do Hub é limpo.
  useEffect(() => {
    if (snapshot && !stale) keepDraftsFor(drafts, snapshot)
  }, [snapshot, stale, drafts])
  const pendingIds = new Set(bots.flatMap((b) => (pendingOf(b) ?? []).map((r) => r.entryId)))
  const recent = answeredHere.filter((a) => !pendingIds.has(a.entryId))
  // D341: só o que precisa do dono ou está acontecendo; Livre e sem estado não aparecem, nem sem conexão.
  // Sem conexão, as seções por estado seriam as de agora: os listados ficam numa lista só, como estavam.
  const listed = useMemo(() => listedBots(bots), [bots])
  const groups = useMemo(() => (stale ? (listed.length ? [{ group: 'stale' as const, bots: listed }] : []) : groupBots(bots)), [bots, listed, stale])
  const uninformed = uninformedCount(bots)
  // Com pedido entre bots listado (MB-2), o vazio calmo não diria a verdade: a seção de Times fala por si.
  const interBotPending = snapshot?.interBotRequests.availability === 'available' && snapshot.interBotRequests.value.items.length > 0
  // Sem a projeção de rotinas, não presumir que a seção está vazia.
  const routinesKnownEmpty = snapshot?.routineCreations.availability === 'available'
    && snapshot.routineCreations.value.items.length === 0 && snapshot.routineCreations.value.blocked.length === 0
  const current = listed.find((b) => b.botId === selected) ?? groups[0]?.bots[0] ?? null
  const at = snapshot ? hm(snapshot.collectedAt, idioma) : null
  const space = isolado(p.spaceName)
  const end = (
    <>
      {at ? <span className="torre-updated">{t(stale ? 'torre.ultimaAtualizacao' : 'torre.atualizado', { hora: at })}</span> : null}
      {p.onProjects ? <button className="torre-times torre-projects" aria-label={t('projetos.entradaAria', { space: p.spaceName })} onClick={p.onProjects}><Icon name="list.bullet.rectangle" size={13} /> {t('projetos.entrada')}</button> : null}
      <button className="torre-times" onClick={p.onTimes}><Icon name="person.2" size={13} /> {t('torre.abrirEquipes')}</button>
    </>
  )
  const goTo = (conversationId: string, botId: string) => {
    if (!snapshot) return null
    const target = openTarget(conversationId, botId, snapshot.spaceId, p.personalSpaceId)
    return target ? () => p.onNavigate(target) : null
  }

  let body: ReactNode
  if (session.phase.kind === 'moved') body = <p className="torre-state" role="alert">{t('torre.movido', { nome: isolado(t('torre.nome')) })}</p>
  else if (!snapshot && session.phase.kind === 'error') body = <p className="torre-state" role="status">{erroTorre(t, session.phase.codigo)}</p>
  else if (!snapshot) body = <p className="torre-state" role="status">{t('torre.abrindo', { nome: isolado(t('torre.nome')) })}</p>
  else {
    body = (
      <>
        {stale ? (
          <div className="torre-banner is-offline" role="status">
            <Icon name="shield" size={15} />
            <p><b>{session.offlineSince
              ? t('torre.offline.tituloDesde', { computador: isolado(p.computerName), hora: hm(session.offlineSince, idioma) })
              : t('torre.offline.titulo', { computador: isolado(p.computerName) })}</b> {t('torre.offline.texto')}</p>
            <button className="torre-button" onClick={p.onRetry}>{t('torre.tentarDeNovo')}</button>
          </div>
        ) : (
          <div className="torre-banner">
            <Icon name="checkmark.shield" size={15} />
            <p><b>{t('torre.filtro.titulo', { space })}</b> {t('torre.filtro.texto')}</p>
          </div>
        )}
        {session.notice ? <p className="torre-notice" role="status">{t('torre.aviso.reler')}</p> : null}
        {uninformed ? <p className="torre-notice" role="status">{t.plural('torre.semEstado', uninformed)}</p> : null}
        <Counts bots={listed} at={stale ? at : null} />
        <div className="torre-main">
          <div className="torre-list">
            {recent.length && responder ? (
              <section className="torre-group" aria-label={t('torre.respondido.titulo')}>
                <h2 className="torre-group-title">{t('torre.respondido.titulo')} <span>{recent.length.toLocaleString(localeDe(idioma))} · {t('torre.respondido.nota')}</span></h2>
                {recent.map((a) => {
                  const entry = responder.entry(a.conversationId, a.entryId)
                  const view = pendingView(entry, { live: !stale, personal: true })
                  return <div key={a.entryId} className="torre-bot"><b className="torre-recent-name"><bdi>{a.name}</bdi></b><PendingItem view={view} responder={responder} onOpen={goTo(a.conversationId, a.botId)} /></div>
                })}
              </section>
            ) : null}
            {groups.length === 0 && !interBotPending && routinesKnownEmpty ? <p className="torre-state" role="status">{t(stale ? 'torre.nadaUltima' : 'torre.nadaAgora')}</p> : null}
            {groups.map(({ group, bots: list }) => (
              <section key={group} className="torre-group" aria-label={group === 'stale' ? t('torre.grupo.stale', { space }) : t(`torre.grupo.${group}`)}>
                <h2 className="torre-group-title">{group === 'stale' ? t('torre.grupo.stale', { space }) : t(`torre.grupo.${group}`)} <span>{list.length.toLocaleString(localeDe(idioma))} · {group === 'stale' ? t('torre.grupo.staleNota', { hora: at! }) : t(`torre.grupo.${group}Nota`)}</span></h2>
                {list.map((bot) => {
                  const items = (stale ? [] : pendingOf(bot) ?? []).map((ref) => ({
                    ref,
                    view: pendingView(responder?.entry(ref.conversation.conversationId, ref.entryId) ?? null, { live: !stale, personal: snapshot.spaceId === p.personalSpaceId && !!responder }),
                    open: goTo(ref.conversation.conversationId, bot.botId),
                  }))
                  // MB-1b: perguntas do bot ao dono, com o campo de resposta pelo mesmo send da conversa.
                  const asks = (stale ? [] : questionsOf(bot) ?? []).map((q) => ({
                    q,
                    view: questionView({ live: !stale, personal: snapshot.spaceId === p.personalSpaceId && !!responder }),
                    open: goTo(q.conversation.conversationId, bot.botId),
                  }))
                  const inline = items.some((i) => i.view.kind !== 'openOnly') || asks.some((a) => a.view.kind === 'answer')
                  return (
                    <div key={bot.botId} className="torre-bot">
                      <BotCard bot={bot} stale={stale} at={at!} selected={current?.botId === bot.botId}
                        onSelect={() => setSelected(bot.botId)}
                        onOpen={bot.open ? goTo(bot.open.conversation.conversationId, bot.botId) : null}
                        onRespond={inline ? null : (() => { const first = pendingOf(bot)?.[0]?.conversation ?? questionsOf(bot)?.[0]?.conversation; return first ? goTo(first.conversationId, bot.botId) : null })()} />
                      {items.map((i) => <PendingItem key={i.ref.entryId} view={i.view} responder={responder} onOpen={i.open} />)}
                      {asks.map((a) => <QuestionItem key={a.q.entryId} question={a.q} botId={bot.botId} name={bot.name} view={a.view} responder={responder} drafts={drafts} onOpen={a.open} />)}
                    </div>
                  )
                })}
              </section>
            ))}
            <RotinasCriadas key={JSON.stringify([snapshot.hubId, snapshot.spaceId])} snapshot={snapshot}
              live={!stale && session.link === 'live' && session.events === 'live' && session.phase.kind === 'ready' && !session.notice}
              onReview={p.onReviewRoutine} />
            <PermissoesDelegadas snapshot={snapshot} live={!stale} goTo={goTo} />
            <TrocasDeConta snapshot={snapshot} />
            <PedidosEntreBots snapshot={snapshot} personal={snapshot.spaceId === p.personalSpaceId} goTo={goTo} />
            <p className="torre-foot"><Icon name="info.circle" size={13} /> {t('torre.rodape')}</p>
          </div>
          <div className="oqa-side">
            <OQueAconteceu snapshot={snapshot} personal={snapshot.spaceId === p.personalSpaceId} onSave={stale ? null : p.onSaveSummary} />
            {current ? (
              <Detail bot={current} stale={stale} at={at!}
                onOpen={current.open ? goTo(current.open.conversation.conversationId, current.botId) : null}
                onUsage={p.onUsage} />
            ) : null}
          </div>
        </div>
      </>
    )
  }
  return <TorreFrame spaceName={p.spaceName} computerName={p.computerName} end={end}>{body}</TorreFrame>
}

/** A falha do Board na frase do catálogo; as que citam o Board recebem o nome dele (`torre.nome`). */
function erroTorre(t: Tradutor, c: TowerError) {
  return c === 'invalido' || c === 'semConexao' ? t(`torre.erro.${c}`) : t(`torre.erro.${c}`, { nome: isolado(t('torre.nome')) })
}

const TONES: TowerTone[] = ['working', 'needsInput', 'waiting', 'error']

function Counts({ bots, at }: { bots: readonly TowerBot[]; at: string | null }) {
  const t = useT()
  const idioma = useIdioma()
  const counts = countByTone(bots)
  const shown = TONES.filter((tone) => counts[tone] > 0)
  if (!shown.length) return null
  return (
    <div className="torre-counts" role="group" aria-label={t('torre.contagens')}>
      {shown.map((tone) => (
        <div key={tone} className="torre-count">
          <Pill tone={tone}>{t(`torre.estado.${tone}`)}</Pill>
          <b>{counts[tone].toLocaleString(localeDe(idioma))}</b>
          {at ? <small>{t('torre.contagemAs', { hora: at })}</small> : null}
        </div>
      ))}
    </div>
  )
}

function Pill({ tone, faded, children }: { tone: TowerTone | 'offline' | 'plain'; faded?: boolean; children: ReactNode }) {
  return <span className={`torre-pill is-${tone}${faded ? ' is-faded' : ''}`}><i aria-hidden />{children}</span>
}

const Face = memo(function Face({ bot, size }: { bot: TowerBot; size: number }) {
  const config = normalizarAvatarDoBot(avatarDoBot(bot), bot.name)
  return <DonnabotAvatar config={config} size={size} animated={false} label={bot.name} />
})

/** A linha de baixo do cartão: só o que o Hub mediu. Atividade e motivo são texto do Hub: entram isolados. */
function workLine(bot: TowerBot, t: Tradutor, idioma: AppLanguage): { text: string; tone?: 'error' } | null {
  if (bot.runtime.availability !== 'available') return { text: t('torre.linha.semRuntime') }
  const w = bot.runtime.value
  const hub = (v: string | null) => (v ? isolado(v) : null)
  if (w.status === 'working') {
    const parts = [hub(activityText(t, idioma, w) || null) ?? t('torre.linha.atividadeNaoInformada'), w.startedAt ? t('torre.linha.desde', { hora: hm(w.startedAt, idioma) }) : null].filter(Boolean)
    return { text: parts.join(' · ') }
  }
  if (w.status === 'waiting') return { text: [hub(w.reason?.replace(/\.$/, '') ?? null) ?? t('torre.linha.aguardando'), w.resumeAt ? t('torre.linha.novaTentativa', { hora: hm(w.resumeAt, idioma) }) : null].filter(Boolean).join(' · ') }
  if (w.status === 'error') return { text: hub(w.reason) ?? t('torre.linha.erro'), tone: 'error' }
  if (w.status === 'needsInput') return { text: hub(w.reason) ?? t('torre.linha.precisa') }
  return null
}

/** Uma permissão original no Board: o mesmo cartão da conversa, ou só Abrir conversa com o motivo. */
export function PendingItem({ view, responder, onOpen }: { view: PendingView; responder: Responder | null; onOpen: (() => void) | null }) {
  const t = useT()
  const open = <button className="torre-link" disabled={!onOpen} onClick={() => onOpen?.()}>{t('torre.abrirConversa')}</button>
  if (view.kind === 'openOnly' || !responder) {
    return <div className="torre-pending is-open-only"><span className="torre-pending-note">{t(view.nota)}</span>{open}</div>
  }
  const entry = view.entry as Entry
  return (
    <div className={`torre-pending${view.kind === 'settled' ? ' is-settled' : ''}`}>
      <div className="torre-pending-head"><span className="torre-pending-note" role={view.kind === 'settled' ? 'status' : undefined}>{t(view.nota)}</span>{open}</div>
      <PermissionCard entry={entry} hostName={responder.hostName} answering={responder.answering(entry.id)} respond={(o) => responder.respond(entry, o)} />
    </div>
  )
}

/** MB-1b: a pergunta do bot e o campo de resposta. Enviar é escrever na conversa da pergunta (mesmo send e mesma
 *  origem humana do composer); o estado só muda quando o Hub registra a mensagem. */
export function QuestionItem({ question, botId, name, view, responder, drafts, onOpen }: { question: TowerQuestion; botId: string; name: string; view: QuestionView; responder: Responder | null; drafts: QuestionDrafts; onOpen: (() => void) | null }) {
  const { input, bind } = useComposerInput<HTMLTextAreaElement>()
  const t = useT()
  const id = question.entryId
  const chat = question.conversation.conversationId
  // R3: texto, origem e nonce no store de rascunhos; desmontar (sem conexão) não os perde.
  const readDraft = () => drafts.get(id)
  const draft = useSyncExternalStore(drafts.subscribe, readDraft, readDraft)
  // B1: o estado vem da entrada local do MESMO send; respondida só quando o Hub tira a pergunta da lista.
  const readState = () => (responder && draft.nonce ? responder.sendState(chat, draft.nonce) : null)
  const state = useSyncExternalStore(responder?.subscribe ?? noSubscribe, readState, readState)
  const step = answerView(state)
  // MX-214: com o aviso de senha colada aberto nesta conversa o envio fica desligado, com o motivo à vista.
  const bloqueado = useSyncExternalStore(responder?.subscribeBloqueio ?? noSubscribe, () => responder?.envioBloqueado?.(chat) ?? false, () => false)
  const open = <button className="torre-link" disabled={!onOpen} onClick={() => onOpen?.()}>{t('torre.abrirConversa')}</button>
  const answering = view.kind === 'answer' && !!responder && state !== 'accepted'
  const submit = () => { if (responder) submitDraft(responder, drafts, id, chat) }
  return (
    <div className="torre-question" data-question={id}>
      <div className="torre-pending-head">
        <span className="torre-question-mark"><Icon name="text.bubble" size={13} /> {t('torre.perguntaDe', { nome: isolado(name) })}</span>
        <span className="torre-pending-note" role={step.nota ? 'status' : undefined}>{t(step.nota ?? view.nota)}</span>
        {open}
      </div>
      <p className="torre-question-text" dir="auto">{question.text}</p>
      {answering ? (
        <form className="torre-question-form" onSubmit={(e) => { e.preventDefault(); submit() }}>
          <textarea ref={bind} className="torre-question-field" rows={2} value={draft.text} readOnly={!step.editable} dir="auto" placeholder={t('torre.suaResposta')} aria-label={t('torre.responderA', { nome: isolado(name) })}
            onChange={(e) => {
              const { kind, range } = input.change(draft.text, e.target.value)
              drafts.set(id, { botId, text: e.target.value, origin: editOrigin(draft.text, e.target.value, draft.origin, kind, range) })
            }}
            onKeyDown={(e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); submit() } }} />
          <button type="submit" className="torre-button is-primary" disabled={!step.editable || bloqueado || !draft.text.trim()} aria-busy={step.busy || undefined}>{t(step.button)}</button>
        </form>
      ) : null}
      {bloqueado && answering ? <p className="torre-question-alert" role="alert">{t('conversa.compositor.senha.envioBloqueado')}</p> : null}
      {step.alert ? <p className="torre-question-alert" role="alert">{t('torre.alertaTexto', { alerta: t(step.alert) })}</p> : null}
    </div>
  )
}

const noSubscribe = () => () => {}


function BotCard({ bot, stale, at, selected, onSelect, onOpen, onRespond }: {
  bot: TowerBot; stale: boolean; at: string; selected: boolean; onSelect: () => void; onOpen: (() => void) | null; onRespond: (() => void) | null
}) {
  const t = useT()
  const idioma = useIdioma()
  const status = statusOf(bot)
  const line = workLine(bot, t, idioma)
  const pending = pendingOf(bot)
  const queue = queueOf(bot)
  const questions = questionsOf(bot)
  const respond = !stale && onRespond
  return (
    <article className={`torre-card${selected ? ' is-selected' : ''}`} data-bot={bot.botId} onClick={onSelect} onFocus={onSelect} aria-current={selected ? 'true' : undefined}>
      <Face bot={bot} size={40} />
      <div className="torre-card-body">
        <div className="torre-card-top">
          <b className="torre-card-name"><bdi>{bot.name}</bdi></b>
          {bot.title ? <span className="torre-tag"><bdi>{bot.title}</bdi></span> : null}
          {stale ? <><Pill tone="offline">{t('torre.semConexao')}</Pill><span className="torre-muted">{t('torre.ultimaAtualizacaoCurta', { hora: at })}</span></> : null}
          <Pill tone={status.tone} faded={stale}>{t(`torre.estado.${status.tone}`)}</Pill>
        </div>
        <p className={`torre-card-line${line?.tone === 'error' ? ' is-error' : ''}`}>
          {line?.text}
          {pending?.length ? <span className="torre-card-pending">{line ? ' · ' : ''}{t.plural('torre.permissoes', pending.length)}</span> : null}
          {questions?.length ? <span className="torre-card-pending">{line || pending?.length ? ' · ' : ''}{t.plural('torre.perguntas', questions.length)}</span> : null}
        </p>
      </div>
      <div className="torre-card-queue">
        <b>{stale || queue === null ? t('torre.fila') : t.plural('torre.fila.esperando', queue)}</b>
        <span>{stale ? t('torre.fila.semDadoNovo') : queue === null ? t('torre.fila.naoDisponivel') : t('torre.fila.medida', { hora: at })}</span>
      </div>
      {respond ? (
        <button className="torre-button is-primary" onClick={(e) => { e.stopPropagation(); respond() }}>{t('torre.abrirParaResponder')}</button>
      ) : (
        <button className="torre-button" disabled={!onOpen} title={onOpen ? t('torre.abrirConversaBot') : t('torre.semConversaNoSpace')}
          onClick={(e) => { e.stopPropagation(); onOpen?.() }}>{t('torre.abrir')}</button>
      )}
    </article>
  )
}

function Row({ k, children }: { k: string; children: ReactNode }) {
  return <div className="torre-row"><span>{k}</span><div>{children}</div></div>
}

function Detail({ bot, stale, at, onOpen, onUsage }: { bot: TowerBot; stale: boolean; at: string; onOpen: (() => void) | null; onUsage: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  const status = statusOf(bot)
  const statusText = t(`torre.estado.${status.tone}`)
  const line = workLine(bot, t, idioma)
  const pending = pendingOf(bot)
  const queue = queueOf(bot)
  const work = bot.runtime.availability === 'available' ? bot.runtime.value : null
  const uso = (o: Observation<number>) => { const n = usageOf(o); return n === null ? null : n.toLocaleString(localeDe(idioma)) }
  return (
    <aside className="torre-detail" aria-label={t('torre.detalhes', { nome: isolado(bot.name) })}>
      <div className="torre-detail-head">
        <Face bot={bot} size={52} />
        <div>
          <h2><bdi>{bot.name}</bdi> {bot.title ? <span className="torre-tag"><bdi>{bot.title}</bdi></span> : null}</h2>
          <Pill tone={status.tone} faded={stale}>{statusText}</Pill>
        </div>
      </div>
      <div className="torre-box">
        <h3>{stale ? t('torre.agoraComoEstava', { hora: at }) : t('torre.agora')}</h3>
        <Row k={t('torre.linhaEstado')}>{statusText}</Row>
        <Row k={work?.status === 'working' ? t('torre.linhaFazendo') : t('torre.linhaMotivo')}>{line?.text ?? '—'}</Row>
        <Row k={t('torre.linhaConversa')}>{bot.open ? t('torre.conversaDoBot') : t('torre.nenhumaConversa')}</Row>
        <Row k={t('torre.linhaPendencias')}>{pending === null ? t('torre.estado.unknown') : pending.length ? t.plural('torre.permissoes', pending.length) : t('torre.nenhumaPermissao')}</Row>
        <Row k={t('torre.fila')}>{queue === null ? t('torre.fila.indisponivel') : `${t.plural('torre.fila.esperando', queue)} · ${t('torre.fila.medida', { hora: at })}`}</Row>
      </div>
      <div className="torre-box">
        <h3>{t('torre.uso.titulo')}</h3>
        <Row k={t('torre.uso.limite')}><UsageValue v={uso(bot.usage.subscriptionLimit)} /></Row>
        <Row k={t('torre.uso.contexto')}><UsageValue v={uso(bot.usage.contextTokens)} /></Row>
        <Row k={t('torre.uso.tokensHoje')}><UsageValue v={uso(bot.usage.dailyBotTokens)} /></Row>
        <p className="torre-small">{t('torre.uso.nota')}</p>
        <button className="torre-link" onClick={onUsage}>{t('torre.uso.abrir')}</button>
      </div>
      <button className="torre-button is-primary is-wide" disabled={!onOpen} onClick={() => onOpen?.()}>{t('torre.abrirConversaBot')}</button>
      {canOpenToStop(bot) ? (
        <button className="torre-button is-wide" disabled={stale || !onOpen} title={stale ? t('torre.esperaConexao') : undefined} onClick={() => onOpen?.()}>
          <Icon name="stop.fill" size={10} /> {t('torre.abrirParaParar')}
        </button>
      ) : null}
      <p className="torre-small torre-note">
        <Icon name="info.circle" size={13} /> <b>{t('torre.nota.abrir')}</b> {t('torre.nota.texto')}
      </p>
    </aside>
  )
}

function UsageValue({ v }: { v: string | null }) {
  const t = useT()
  return v === null ? <span className="torre-pill is-plain">{t('torre.estado.unknown')}</span> : <span>{v}</span>
}

/** O Space existe no Hub deste computador (Pessoal ou um da lista); um id solto não abre Projetos. */
function knownSpace(store: BotStore, spaceId: string) {
  const spaces = store.spaces
  return !!spaces && (spaceId === spaces.personalSpaceId || spaces.spaces.some((s) => s.id === spaceId))
}

function spaceLabel(store: BotStore, spaceId: string, t: Tradutor) {
  const spaces = store.spaces
  if (!spaces || spaceId === spaces.personalSpaceId) return t('torre.pessoal')
  return spaces.spaces.find((s) => s.id === spaceId)?.name ?? t('torre.space')
}

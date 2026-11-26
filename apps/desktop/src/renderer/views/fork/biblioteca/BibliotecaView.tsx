import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from 'react'
import type { Backend } from '@shared/models'
import { contaPadraoDoHub, escolhaDeConta } from '@shared/fork-criar-bot-destino'
import type { ManagedAccount } from '@shared/fork-engine-accounts'
import type { HostClient } from '../../../client/host-client'
import { engineSetup } from '../../../store/fork-engine-setup'
import { ContaDaIa } from '../../criar-bot/Destino'
import { avatarDoBot, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import type { NativeLink, NativeProfile } from '@shared/fork-native-catalog'
import { Button, ChoicePicker, IconButton, Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { Dialog, ModalHeader, useEscape } from '../../../components/Overlay'
import { DonnabotAvatar } from '../../../fork/avatars/Avatar'
import { useModel } from '../../../lib/observable'
import { opcoesDeMotor } from '../../criar-bot/ModeloDaIa'
import { AvisoDoMotor } from '../../fork/aviso-motor/AvisoDoMotor'
import { LibrarySession, type LibrarySource } from './biblioteca-session'
import { libraryActions, type LibraryAction } from './biblioteca-acoes'
import '../../criar-bot/funcao-etiqueta.css'
import './biblioteca.css'
import { traduzir } from '../../../lib/fork-i18n/index.ts'

// Biblioteca de especialistas (components/BibliotecaEspecialistas).
// Só apresenta o que o Hub manda e envia a intenção do gesto. Perfis, rostos, Almas e vínculos vêm da prévia do Hub.
// Editor normal só no Pessoal; fora dele, só a conversa exata do bot (pendência bot-dentro-13).

export const FORA_DO_PESSOAL = 'Editar este bot fora do Pessoal ainda não está disponível.'

/** O que a folha usa da store do computador (BotStore). */
export interface LibraryStore extends LibrarySource {
  readonly hello: { hostId: string; backends?: Backend[] } | null
  readonly hostName: string
  readonly bots: ReadonlyMap<string, unknown>
  /** A versão da store (Observable): um bot que chega ou a conexão que cai redesenham a folha. */
  readonly getVersion: () => number
  /** Fora do Pessoal: lê as contas de IA do Hub para a escolha (a mesma leitura da criação manual). */
  ready(): Promise<HostClient>
}

export interface LibraryTarget { store: LibraryStore; spaceId: string; personal: boolean; spaceName: string }

const clock = (t: number | null) => (t ? new Date(t).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' }) : '')

export function BibliotecaView({ target, session: given, onEditor, onConversation }: {
  target: LibraryTarget
  /** Só a prova visual injeta uma sessão já aberta; o app cria a sua. */
  session?: LibrarySession
  /** Pessoal: abre o Bot por dentro normal desse bot (que já está no app). */
  onEditor: (botId: string) => void
  /** Pessoal: a conversa do bot. Outro space: a conversa exata dele no space. */
  onConversation: (botId: string) => void
}) {
  const { store, spaceId, personal, spaceName } = target
  const [session] = useState(() => given ?? new LibrarySession(spaceId, personal))
  useModel(session)
  useSyncExternalStore(store.subscribe, store.getVersion)
  useEffect(() => {
    if (given) return
    session.open(store)
    return () => session.close()
  }, [session, store, given])
  const [open, setOpen] = useState<string | null>(null)
  const [backend, setBackend] = useState('')
  const [conta, setConta] = useState<string | null>(null)
  const [contas, setContas] = useState<{ estado: 'carregando' | 'pronto' | 'falhou'; lista: ManagedAccount[] }>({ estado: 'carregando', lista: [] })
  const podeConta = store.canManageEngineAccounts
  useEffect(() => {
    if (personal || !podeConta) return
    let live = true
    void store.ready().then((client) => engineSetup.accounts(client)).then(
      (r) => { if (live) setContas({ estado: 'pronto', lista: r.accounts }) },
      () => { if (live) setContas({ estado: 'falhou', lista: [] }) },
    )
    return () => { live = false }
  }, [store, personal, podeConta])
  const [readd, setReadd] = useState(false)
  const lastCard = useRef<string | null>(null)
  const body = useRef<HTMLDivElement>(null)

  const catalog = session.catalog
  const op = session.op
  const live = session.link === 'live'
  const linkOf = (key: string) => catalog?.links.find((l) => l.catalogKey === key) ?? null
  const profile = open ? catalog?.profiles.find((p) => p.catalogKey === open) ?? null : null
  // Fora do Pessoal, a conta de IA como na criação manual num Space: uma conectada do motor ou o padrão legado do Hub.
  const escolhaConta = escolhaDeConta(podeConta, backend)
  const contasDoBackend = contas.lista.filter((c) => c.backend === backend).map((c) => ({ conta: c, escolhivel: c.state === 'connected' }))
  const contaPadrao = contaPadraoDoHub(contas.lista, backend)
  const contaEscolhida = personal || escolhaConta !== 'escolher' ? null
    : conta ? contasDoBackend.find((c) => c.conta.id === conta && c.escolhivel)?.conta ?? null : contaPadrao
  const opKey = op.kind === 'sending' || op.kind === 'unknown' ? op.intent.body.catalogKey : op.kind === 'refused' ? op.catalogKey : op.kind === 'done' ? op.added.catalogKey : null

  // Sucesso provado no Pessoal: quando o bot chega ao app, abre o Bot por dentro normal.
  const waiting = personal && op.kind === 'done' && op.added.state === 'active' ? op.added.botId : null
  const arrived = waiting ? store.bots.has(waiting) : false
  const opened = useRef<string | null>(null)
  useEffect(() => {
    if (!waiting || !arrived || opened.current === waiting) return
    opened.current = waiting
    onEditor(waiting)
  }, [waiting, arrived, onEditor])

  // Prévia: o teclado vai ao Voltar; ao sair, volta ao botão do cartão.
  useEffect(() => {
    if (open) body.current?.querySelector<HTMLElement>('.bib-voltar button')?.focus()
    else if (lastCard.current) body.current?.querySelector<HTMLElement>(`[data-perfil="${lastCard.current}"] .bib-cartao-acao button`)?.focus()
  }, [open])
  const show = (key: string | null) => {
    if (key) lastCard.current = key
    setOpen(key)
    setBackend('')
    setConta(null)
  }

  // Esc na prévia volta à grade; na grade, a folha fecha (Sheet).
  useEscape(!!open && !readd, () => show(null), body)

  const add = () => {
    if (!profile) return
    void session.add(profile.catalogKey, personal ? null : backend || null, contaEscolhida?.id ?? null)
  }

  const actionsOf = (key: string, preview: boolean) => libraryActions(key, { personal, link: linkOf(key), op, preview })
  const ctxFor = (key: string, card: boolean): ActionContext => ({
    personal, spaceName, card,
    knows: (id) => store.bots.has(id),
    canAdd: session.canAdd,
    canRetry: live && session.authorized,
    motorMissing: !personal && !backend,
    contaMissing: !personal && !!backend && !contaEscolhida,
    sending: op.kind === 'sending',
    waiting,
    onShow: () => show(key),
    onAdd: add,
    onReadd: () => setReadd(true),
    onRetry: () => void session.retry(),
    onCurrent: () => session.showCurrent(),
    onEditor,
    onConversation,
  })

  return <div className="bib">
    <ModalHeader title="Biblioteca de especialistas" subtitle={`Adicionar em ${spaceName} · ${store.hostName}`} icon="book.closed" />
    {/* Avisos fora da área que rola: continuam à vista em qualquer posição da grade ou da prévia. */}
    <div className="bib-avisos">
      {!live && catalog ? <p className="bib-aviso" role="status"><Icon name="wifi.slash" size={13} /> Sem conexão com o computador dos seus bots. Última leitura às {clock(session.asOf)}. Adicionar fica indisponível até voltar.</p> : null}
      {!session.authorized && live && catalog ? <p className="bib-aviso" role="status">Este aparelho não pode adicionar bots neste computador agora.</p> : null}
      {op.kind === 'unknown' ? <Pendente session={session} name={catalog?.profiles.find((p) => p.catalogKey === op.intent.body.catalogKey)?.name ?? 'Bot'} /> : null}
      {op.kind === 'done' && op.added.state === 'removed' && open !== op.added.catalogKey ? <Recibo session={session} name={catalog?.profiles.find((p) => p.catalogKey === op.added.catalogKey)?.name ?? 'Bot'} /> : null}
    </div>
    <div className="bib-corpo" ref={body}>
      {session.phase.kind === 'loading' ? <div className="bib-reserva" aria-hidden><span /><span /><span /></div>
        : session.phase.kind === 'moved' ? <p className="bib-problema" role="status">A conexão passou para outro computador. Feche e abra a biblioteca de novo.</p>
          : session.phase.kind === 'error' && !catalog ? <div className="bib-problema" role="status">
            <span>{session.phase.message}</span>
            <Button kind="secondary" onClick={() => session.reread()} disabled={!live}>Tentar de novo</Button>
          </div>
            : catalog && profile ? <Previa profile={profile} actions={actionsOf(profile.catalogKey, true)} ctx={ctxFor(profile.catalogKey, false)} target={target} session={session}
              backend={backend} onBackend={(b) => { setBackend(b); setConta(null) }} onBack={() => show(null)}
              contaSlot={personal || !backend ? null : <ContaDaIa escolha={escolhaConta} contas={contasDoBackend} carregando={contas.estado === 'carregando'}
                falhou={contas.estado === 'falhou'} conta={conta} padraoDisponivel={!!contaPadrao}
                disabled={op.kind === 'sending' || !live} onConta={setConta} />}
              message={opKey === profile.catalogKey ? opMessage(session, personal, spaceName) : null} />
              : catalog ? <>
                <p className="bib-faixa"><Icon name="checkmark.shield" size={15} /><span><strong>{traduzir('pt-BR', 'marca.biblioteca.faixa')}</strong> Nada é criado até você abrir a prévia e escolher Adicionar. Cada um vira um bot normal deste space.</span></p>
                <ul className={`bib-grade${live ? '' : ' is-antigo'}`} aria-label="Especialistas">
                  {catalog.profiles.map((p) => <Cartao key={p.catalogKey} profile={p} link={linkOf(p.catalogKey)}
                    actions={actionsOf(p.catalogKey, false)} ctx={ctxFor(p.catalogKey, true)} />)}
                </ul>
              </> : null}
    </div>
    <Dialog open={readd && !!profile} title={`Adicionar o ${profile?.name ?? 'bot'} de novo?`}
      message="Começa outro bot, sem a conversa e a memória do anterior."
      actions={[{ title: 'Adicionar de novo', action: () => { setReadd(false); add() } }]} onClose={() => setReadd(false)} />
  </div>
}

function opMessage(session: LibrarySession, personal: boolean, spaceName: string): string | null {
  const op = session.op
  if (op.kind === 'sending') return 'Adicionando…'
  if (op.kind === 'refused') return op.message
  if (op.kind === 'done') {
    if (op.added.state === 'removed') return RECIBO_REMOVIDO
    return personal ? 'Adicionado. Abrindo o bot…' : `Adicionado a ${spaceName}.`
  }
  return null
}

function Pendente({ session, name }: { session: LibrarySession; name: string }) {
  return <div className="bib-pendente" role="status">
    <span><strong>{name}: sem resposta.</strong> Não sabemos se foi adicionado. Conferir resultado reenvia o mesmo pedido; não cria outro bot.</span>
    <Button kind="secondary" onClick={() => session.discard()}>Descartar pedido</Button>
    <Button onClick={() => void session.retry()} disabled={session.link !== 'live' || !session.authorized}>Conferir resultado</Button>
  </div>
}

/** R1-02: o recibo histórico Removido, com a única saída explícita para o estado atual (que não abre nada). */
function Recibo({ session, name }: { session: LibrarySession; name: string }) {
  return <div className="bib-pendente" role="status">
    <span><strong>{name}:</strong> {RECIBO_REMOVIDO}</span>
    <Button kind="secondary" onClick={() => session.showCurrent()}>Ver estado atual</Button>
  </div>
}

const RECIBO_REMOVIDO = 'Esse pedido já tinha sido atendido, mas o bot foi removido depois. Ver estado atual mostra o que existe agora neste space.'

/** Como as ações de `libraryActions` agem nesta folha; o destino de abrir vem sempre da própria ação. */
interface ActionContext {
  personal: boolean
  spaceName: string
  card: boolean
  knows: (botId: string) => boolean
  canAdd: boolean
  canRetry: boolean
  motorMissing: boolean
  contaMissing: boolean
  sending: boolean
  waiting: string | null
  onShow: () => void
  onAdd: () => void
  onReadd: () => void
  onRetry: () => void
  onCurrent: () => void
  onEditor: (botId: string) => void
  onConversation: (botId: string) => void
}

function Acoes({ actions, ctx }: { actions: LibraryAction[]; ctx: ActionContext }) {
  return <>{actions.map((a, i) => {
    const last = i === actions.length - 1
    const kind = !last || a.kind === 'show' || (ctx.card && a.kind === 'conversation') || a.kind === 'current' ? 'secondary' : 'primary'
    const common = { kind, key: a.kind } as const
    const data = { 'data-acao': a.kind, ...('botId' in a ? { 'data-bot': a.botId } : {}) }
    switch (a.kind) {
      case 'editor': {
        const known = ctx.knows(a.botId)
        return <span key={a.kind} {...data} className="bib-acao"><Button {...common} onClick={() => ctx.onEditor(a.botId)} disabled={!known || ctx.waiting === a.botId}
          title={known ? undefined : 'Este bot ainda não apareceu neste app.'}>{ctx.waiting === a.botId ? <Spinner /> : 'Abrir'}</Button></span>
      }
      case 'conversation':
        return <span key={a.kind} {...data} className="bib-acao"><Button {...common} onClick={() => ctx.onConversation(a.botId)}
          disabled={ctx.personal && !ctx.knows(a.botId)}>{ctx.personal ? 'Conversa' : 'Abrir conversa'}</Button></span>
      case 'show':
        return <span key={a.kind} {...data} className="bib-acao"><Button {...common} onClick={ctx.onShow}><Icon name="magnifyingglass" size={12} /> {a.again ? 'Ver e adicionar de novo' : 'Ver e adicionar'}</Button></span>
      case 'add':
        return <span key={a.kind} {...data} className="bib-acao"><Button {...common} onClick={ctx.onAdd} disabled={!ctx.canAdd || ctx.motorMissing || ctx.contaMissing}>{ctx.sending ? <Spinner /> : `Adicionar a ${ctx.spaceName}`}</Button></span>
      case 'readd':
        return <span key={a.kind} {...data} className="bib-acao"><Button {...common} onClick={ctx.onReadd} disabled={!ctx.canAdd || ctx.motorMissing || ctx.contaMissing}>{ctx.sending ? <Spinner /> : 'Adicionar de novo…'}</Button></span>
      case 'retry':
        return <span key={a.kind} {...data} className="bib-acao"><Button {...common} onClick={ctx.onRetry} disabled={!ctx.canRetry}>Conferir resultado</Button></span>
      case 'current':
        return <span key={a.kind} {...data} className="bib-acao"><Button {...common} onClick={ctx.onCurrent}>Ver estado atual</Button></span>
    }
  })}</>
}

const opens = (actions: LibraryAction[]) => actions.some((a) => a.kind === 'editor' || a.kind === 'conversation')

function Cartao({ profile, link, actions, ctx }: { profile: NativeProfile; link: NativeLink | null; actions: LibraryAction[]; ctx: ActionContext }) {
  const id = `bib-${profile.catalogKey}`
  const kinds = actions.map((a) => a.kind)
  return <li className="bib-cartao" data-perfil={profile.catalogKey}>
    <article aria-labelledby={id}>
      <span className="bib-cartao-rosto" onClick={ctx.onShow} aria-hidden>
        <DonnabotAvatar config={normalizarAvatarDoBot(avatarDoBot(profile), profile.name)} size={72} animated={false} label={profile.name} />
      </span>
      <h3 id={id} className="bib-nome">{profile.name}</h3>
      {profile.title ? <span className="funcao-etiqueta bib-funcao" title={profile.title}>{profile.title}</span> : null}
      <ul className="bib-exemplos">{profile.examples.map((e) => <li key={e}>{e}</li>)}</ul>
      <p className={`bib-estado${kinds.includes('retry') || kinds.includes('current') ? ' is-atencao' : ''}`}>
        {kinds.includes('retry') ? 'Sem resposta. Não sabemos se foi adicionado.'
          : kinds.includes('current') ? 'Pedido atendido antes; o bot dele foi removido.'
            : opens(actions) ? <span className="bib-selo"><Icon name="checkmark" size={11} weight="semibold" /> Já neste space</span>
              : link?.state === 'removed' ? 'Removido deste space. Não volta sozinho.'
                : 'Nada é criado antes de você confirmar.'}
      </p>
      <div className="bib-cartao-acao"><Acoes actions={actions} ctx={ctx} /></div>
    </article>
  </li>
}

function Previa({ profile, actions, ctx, target, session, backend, onBackend, onBack, contaSlot, message }: {
  profile: NativeProfile
  actions: LibraryAction[]
  ctx: ActionContext
  target: LibraryTarget
  session: LibrarySession
  backend: string
  onBackend: (id: string) => void
  onBack: () => void
  contaSlot: ReactNode
  message: string | null
}) {
  const { personal, spaceName, store } = target
  const kinds = actions.map((a) => a.kind)
  const settled = opens(actions) || kinds.includes('current') || kinds.includes('retry')
  const backends = store.hello?.backends ?? []
  const titleId = `bib-previa-${profile.catalogKey}`
  const note = kinds.includes('current') ? RECIBO_REMOVIDO
    : message ?? (opens(actions) && !personal ? FORA_DO_PESSOAL : !settled && ctx.motorMissing ? 'Escolha o motor antes de adicionar.' : !settled && ctx.contaMissing ? traduzir('pt-BR', 'marca.biblioteca.escolhaConta') : !session.authorized ? 'Este aparelho não pode adicionar bots neste computador agora.' : '')

  return <section className="bib-previa" aria-labelledby={titleId}>
    <div className="bib-previa-topo">
      <span className="bib-voltar"><IconButton title="Voltar à biblioteca" icon="chevron.left" onClick={onBack} /></span>
      <DonnabotAvatar config={normalizarAvatarDoBot(avatarDoBot(profile), profile.name)} size={56} animated={false} label={profile.name} />
      <div className="bib-previa-titulo">
        <h3 id={titleId}>{profile.name}{profile.title ? <span className="funcao-etiqueta bib-funcao">{profile.title}</span> : null}</h3>
        <p>{traduzir('pt-BR', 'marca.biblioteca.perfilPronto')} {profile.profileVersion}. Depois de adicionar, ele é um bot normal deste space.</p>
      </div>
    </div>
    <div className="bib-previa-corpo">
      <div className="bib-coluna">
        <h4>Instruções permanentes (Alma) <span className="bib-chega">Chega ao bot</span></h4>
        <p className="bib-alma">{profile.description}</p>
        <p className="bib-nota"><Icon name="slider.horizontal.3" size={12} /> Estilo de escrita não vem no perfil.</p>
        <h4>Exemplos do que pedir</h4>
        <ul className="bib-pilulas">{profile.examples.map((e) => <li key={e}>{e}</li>)}</ul>
      </div>
      <div className="bib-coluna">
        <h4>Pode precisar de…</h4>
        <p className="bib-texto">{profile.suggestedResources}</p>
        <p className="bib-nota">Recursos efetivos do bot: ainda não disponível neste app. Nada é liberado ao adicionar.</p>
        <h4>Motor</h4>
        {personal ? <p className="bib-texto">Segue o motor padrão do Pessoal.</p> : <>
          <div className="bib-motor">
            {settled ? <span /> : <span className="bib-etiqueta-atencao">Escolha necessária neste space</span>}
            <ChoicePicker selection={backend} options={[{ id: '', label: 'Escolher motor…' }, ...opcoesDeMotor(backends)]} onChange={onBackend}
              disabled={settled || ctx.sending || session.link !== 'live'} />
          </div>
          {backend ? <AvisoDoMotor backend={backend} /> : null}
          {contaSlot && !settled ? <><h4>{traduzir('pt-BR', 'botCreate.contaDeIa')}</h4>{contaSlot}</> : null}
        </>}
        {personal ? <p className="bib-nota">Contas de IA não são ligadas ao adicionar.</p> : null}
        <h4>Ao adicionar</h4>
        <ul className="bib-lista">
          <li>Entra em {spaceName}, no {store.hostName}</li>
          <li>Conversa e memória próprias, começando vazias</li>
          <li>Começa em Pedir aprovação</li>
          {personal ? <li>Sem contas, conectores, skills ou computador</li> : <li>{traduzir('pt-BR', 'marca.biblioteca.soConta')}</li>}
          <li>Não começa nenhum trabalho</li>
          {personal ? null : <li>Em outro space seria outro bot, sem memória nem acessos deste</li>}
        </ul>
      </div>
    </div>
    <div className="bib-rodape">
      <p className="bib-mensagem" role="status">
        {note}
        {message && opens(actions) && !personal ? <><br />{FORA_DO_PESSOAL}</> : null}
      </p>
      <Button kind="secondary" onClick={onBack}>Voltar</Button>
      <Acoes actions={actions} ctx={ctx} />
    </div>
  </section>
}

/**
 * O alvo da folha: o space de onde ela abriu, no Hub dessa store. Sem Spaces no Hub (sem o Pessoal canônico), não há
 * biblioteca: nunca cai noutro space nem no Pessoal por omissão.
 */
export function libraryTarget(store: LibraryStore & { spaces: { personalSpaceId: string; spaces: { id: string; name: string }[] } | null }, spaceId?: string): LibraryTarget | null {
  const spaces = store.spaces
  if (!spaces) return null
  const id = spaceId ?? spaces.personalSpaceId
  const personal = id === spaces.personalSpaceId
  const space = personal ? null : spaces.spaces.find((s) => s.id === id)
  if (!personal && !space) return null
  return { store, spaceId: id, personal, spaceName: personal ? 'Pessoal' : space!.name }
}

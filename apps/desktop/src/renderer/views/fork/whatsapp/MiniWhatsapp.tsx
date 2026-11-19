import { spaceAllows } from '../../../lib/fork-space-capabilities'
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react'
import type { Bot } from '@shared/models'
import { DropdownMenu } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { Dialog } from '../../../components/Overlay'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import { useStore } from '../../../store/context'
import { useOpenBotSettings } from '../bot-settings/bot-settings-entry'
import { caixaDe, SEM_CONEXAO, type CaixaWhatsapp } from './caixa-estado'
import { TextoDaMensagem, useWhatsapp } from './partes'
import {
  contagens, corDa, duracao, estadoNumero, filtrar, horaLista, horaMensagem, iconeDa, iniciais, itensConversa,
  transcricao, verLigado, ALVO_AO_ABRIR, refDaConversa, seletorDaLinha, type ConversaWa, type Faixa, type Filtro, type MensagemWa,
} from './caixa'
import { erroDoHub, type BotWhatsapp } from './regras'
import { superficieIdioma, useIdioma, useT, type Tradutor } from '../../../lib/fork-i18n/estado'
import { comNos, no, semParametros, Superficie } from '../../settings/rotulo'
import type { Mensagem } from '../bot-settings/mensagem'
import { NumeroBot } from './NumeroBot'
import { NomeConversa, nomeEscondido, Previa, Remetente, rotuloConversa, rotuloPrevia } from './contato'
import './mini-whatsapp.css'

// whatsapp R5c (DON-861/DON-867, D335–D338): o "mini WhatsApp" do bot. Pequeno, como um celular, na aba WhatsApp do
// painel direito; Expandir leva a mesma caixa para a tela grande no centro (TelaGrande.tsx). O dono lê e escreve
// (MX-18/D379); nada do que chega vira ordem, e a escrita do dono não passa pela fila do bot.

/** A caixa do Hub atual, relida ao conectar e a cada aviso `whatsappChanged`. */
export function useCaixa(store: BotStore): CaixaWhatsapp {
  // D350: a leitura do plugin e do número é a compartilhada (useWhatsapp): ligar ou desligar em Ajustes → Plugins
  // mostra ou tira a aba na hora; ela relê ao conectar, a cada aviso e enquanto o wacli baixa.
  useWhatsapp(store)
  const caixa = caixaDe(store, () => spaceAllows(store.spaceId, 'connectors') ? store.connectedClient : null)
  useModel(caixa)
  useEffect(() => { if (spaceAllows(store.spaceId, 'connectors')) caixa.sincronizar({ seq: store.whatsappEpoch, botId: store.whatsappBot }) }, [caixa, store.whatsappEpoch, store.whatsappBot])
  return caixa
}

/** A conversa aberta e a tela grande, guardadas na conversa do bot (ThreadView): o painel e o centro mostram a mesma. */
export interface WaControle {
  chat: string | null
  setChat: (id: string | null) => void
  expandida: boolean
  /** D374: o painel está à vista (aberto e sem a tela grande). Fora disso, o número do bot volta a esconder. */
  painelVisivel: boolean
  expandir: () => void
  recolher: () => void
}

/** O conteúdo da aba WhatsApp no painel (332 px). */
export function AbaWhatsapp({ bot, wa, controle }: { bot: Bot; wa: BotWhatsapp; controle: WaControle }) {
  const t = useT()
  const store = useStore()
  const caixa = useCaixa(store)
  const openSettings = useOpenBotSettings()
  const ver = verLigado(wa)
  const numero = estadoNumero(wa)
  const settings = () => openSettings?.(store, bot.id, 'whatsapp')
  // D454: a aba fica no painel (legado pt-BR/LTR); o conteúdo declara o idioma do app.
  return <Superficie>
    <div className={`wa-top${numero.ruim ? ' bad' : ''}`}>
      <span className="d" aria-hidden />
      <span className="wa-top-t">{semParametros(t)(numero.texto)}{numero.phone ? <> · <NumeroBot phone={numero.phone} ativo={controle.painelVisivel} /></> : null}</span>
      <span className="sp" />
      {ver ? (
        <button className="rp-icon wa-expand" title={t('whatsapp.caixa.expandir')} aria-label={t('whatsapp.caixa.expandirAria')} onClick={controle.expandir}>
          <Icon name="arrow.up.left.and.arrow.down.right" size={13} scaled={false} />
        </button>
      ) : null}
    </div>
    {ver
      ? <Caixa bot={bot} wa={wa} caixa={caixa} controle={controle} grande={false} faixa={numero.faixa} />
      : <Aviso icone="eye.slash" titulo={t('whatsapp.caixa.verDesligado')} acao={t('whatsapp.caixa.abrirConfig')} onAcao={settings}>
          {comNos(t('whatsapp.caixa.verDesligadoTexto', { ver: no('ver') }), { ver: <b>{t('whatsapp.estados.opcao.verConversas')}</b> })}
        </Aviso>}
  </Superficie>
}

/** Lista + conversa. No painel, a conversa abre por cima da lista (com "‹"); na tela grande, ao lado. */
export function Caixa({ bot, wa, caixa, controle, grande, faixa }: { bot: Bot; wa: BotWhatsapp; caixa: CaixaWhatsapp; controle: WaControle; grande: boolean; faixa: Faixa | null }) {
  const lista = caixa.lista(bot.id)
  useEffect(() => { void caixa.lerConversas(bot.id) }, [caixa, bot.id])
  const conversas = lista?.conversas ?? null
  const aberta = controle.chat ? conversas?.find((c) => c.whatsappChatId === controle.chat) ?? null : null
  // A conversa guardada que sumiu (prazo de guarda ou "Apagar esta conversa") volta para a lista.
  useEffect(() => { if (conversas && controle.chat && !aberta) controle.setChat(null) }, [conversas, controle, aberta])
  // Foco ao abrir pela linha e ao voltar (nunca BODY), antes de pintar: a lista fica escondida sob a conversa.
  const area = useRef<HTMLDivElement>(null)
  const foco = useRef<{ abrir: string } | { voltar: string } | null>(null)
  useLayoutEffect(() => {
    const f = foco.current
    const raiz = area.current
    if (!f || !raiz) return
    if ('abrir' in f && aberta?.whatsappChatId === f.abrir) raiz.querySelector<HTMLElement>(ALVO_AO_ABRIR[grande ? 'grande' : 'painel'])?.focus({ preventScroll: true })
    else if ('voltar' in f && !aberta) raiz.querySelector<HTMLElement>(seletorDaLinha(f.voltar))?.focus({ preventScroll: true })
    else return
    foco.current = null
  })
  const abrir = (id: string) => { foco.current = { abrir: id }; controle.setChat(id) }
  const voltar = () => { if (controle.chat) foco.current = { voltar: controle.chat }; controle.setChat(null) }
  const t = useT()
  const aviso = caixa.wacliFicou(bot.id) ? t('whatsapp.conversa.wacliFicou') : null
  const banner = <>
    {faixa ? <div className="wa-banner" role="status"><Icon name="wifi.slash" size={13} scaled={false} /><span><b>{semParametros(t)(faixa.titulo)}</b> {semParametros(t)(faixa.texto)}</span></div> : null}
    {aviso ? <div className="wa-banner" role="status"><Icon name="exclamationmark.triangle" size={13} scaled={false} /><span>{aviso}</span></div> : null}
  </>

  let corpo
  if (lista?.erro && !conversas) corpo = <Aviso icone="exclamationmark.triangle" titulo={t('whatsapp.caixa.naoLeuConversas')} acao={t('whatsapp.caixa.tentar')} onAcao={() => void caixa.lerConversas(bot.id)} erro><TextoDaMensagem m={lista.erro} /></Aviso>
  else if (!conversas) corpo = <div className="wa-loading" aria-busy>{t('whatsapp.caixa.carregandoConversas')}</div>
  else if (!conversas.length) corpo = <Aviso icone="bubble.left" titulo={t('whatsapp.caixa.nenhuma')}>{comNos(t('whatsapp.caixa.nenhumaTexto', { numero: no('numero'), bot: no('bot') }), { numero: wa.number.phone ? <NumeroBot phone={wa.number.phone} ativo={grande || controle.painelVisivel} /> : t('whatsapp.caixa.esteNumero'), bot: <bdi dir="auto">{bot.name}</bdi> })}</Aviso>
  else corpo = <Lista conversas={conversas} selecionada={grande ? controle.chat : null} apagada={!!faixa} onAbrir={abrir} />

  if (grande) {
    return (
      <div className="wa-x-body" ref={area}>
        <div className="wa-x-list">{banner}{corpo}</div>
        <div className="wa-x-conv">
          {aberta ? <Conversa key={aberta.whatsappChatId} bot={bot} conversa={aberta} caixa={caixa} ativo />
            : <div className="wa-x-none"><Icon name="bubble.left.and.bubble.right" size={22} scaled={false} /><span>{t('whatsapp.caixa.escolha')}</span></div>}
        </div>
      </div>
    )
  }
  return (
    <div className="wa-stage" ref={area}>
      <div className="wa-mini" inert={!!aberta}>{banner}{corpo}</div>
      {aberta ? <div className="wa-over"><Conversa key={aberta.whatsappChatId} bot={bot} conversa={aberta} caixa={caixa} onVoltar={voltar} ativo={controle.painelVisivel} /></div> : null}
    </div>
  )
}

const FILTROS: Filtro[] = ['todas', 'naoLidas', 'privadas', 'grupos']

function Lista({ conversas, selecionada, apagada, onAbrir }: { conversas: ConversaWa[]; selecionada: string | null; apagada: boolean; onAbrir: (id: string) => void }) {
  const t = useT()
  const [filtro, setFiltro] = useState<Filtro>('todas')
  const [busca, setBusca] = useState('')
  const n = contagens(conversas)
  const conta: Partial<Record<Filtro, number>> = { naoLidas: n.naoLidas, privadas: n.privadas }
  const shown = filtrar(conversas, filtro, busca)
  return <>
    <label className="wa-search">
      <Icon name="magnifyingglass" size={13} scaled={false} />
      <input type="search" dir="auto" placeholder={t('whatsapp.caixa.pesquisar')} value={busca} onChange={(e) => setBusca(e.target.value)} aria-label={t('whatsapp.caixa.pesquisarAria')} />
    </label>
    <div className="wa-pills" role="radiogroup" aria-label={t('whatsapp.caixa.filtrarAria')}>
      {FILTROS.map((f) => (
        <button key={f} role="radio" aria-checked={filtro === f} className={filtro === f ? 'on' : ''} onClick={() => setFiltro(f)}>
          {t(`whatsapp.caixa.filtro.${f}`)}{conta[f] ? <i dir="ltr">{conta[f]}</i> : null}
        </button>
      ))}
    </div>
    <div className={`wa-list${apagada ? ' dim' : ''}`} role="list" aria-label={t('whatsapp.caixa.listaAria')}>
      {shown.map((c) => <Linha key={c.whatsappChatId} c={c} sel={selecionada === c.whatsappChatId} onClick={() => onAbrir(c.whatsappChatId)} />)}
      {!shown.length ? <p className="wa-nada">{busca.trim() ? t('whatsapp.caixa.nadaBusca') : t('whatsapp.caixa.nadaFiltro')}</p> : null}
    </div>
  </>
}

export function Avatar({ c, size }: { c: Pick<ConversaWa, 'whatsappChatId' | 'kind' | 'name'>; size?: 'sm' }) {
  if (c.kind === 'group') return <span className={`wa-av g${size ? ` ${size}` : ''}`} aria-hidden><Icon name="person.2" size={size ? 14 : 17} scaled={false} /></span>
  const ini = iniciais(c)
  // Sem nome (só o número), o círculo cinza com a pessoa, como na proposta.
  const cor = ini ? corDa(c.whatsappChatId) : 'grafite'
  return (
    <span className={`wa-av${size ? ` ${size}` : ''}`} aria-hidden style={{ background: `var(--tag-${cor}-bg)`, color: `var(--tag-${cor}-ink)` }}>
      {ini || <Icon name="person" size={size ? 14 : 17} scaled={false} />}
    </span>
  )
}

export function Linha({ c, sel, onClick }: { c: ConversaWa; sel: boolean; onClick: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  const icone = iconeDa(c.last)
  // D387: o número (sem nome salvo) só escondido, inclusive no nome acessível da linha.
  const nome = rotuloConversa(t, c)
  const texto = rotuloPrevia(t, c)
  return (
    <button className={`wa-cv${sel ? ' sel' : ''}`} data-chat={refDaConversa(c.whatsappChatId)} role="listitem" onClick={onClick} aria-current={sel ? 'true' : undefined}
      aria-label={c.unread
        ? t.plural(c.pinned ? 'whatsapp.caixa.linhaNaoLidasFixada' : 'whatsapp.caixa.linhaNaoLidas', c.unread, { nome, texto })
        : t(c.pinned ? 'whatsapp.caixa.linhaFixada' : 'whatsapp.caixa.linha', { nome, texto })}>
      <Avatar c={c} />
      <span className="nm"><NomeConversa c={c} olho={false} /></span>
      <span className={`tm${c.unread ? ' u' : ''}`}><bdi dir="ltr">{horaLista(c.last.sentAt, undefined, idioma)}</bdi></span>
      <span className="ms">{icone ? <Icon name={icone} size={12} scaled={false} /> : null}<span><Previa c={c} /></span></span>
      <span className="end">
        {c.pinned ? <Icon name="pin" size={12} scaled={false} /> : null}
        {c.unread ? <span className="n" dir="ltr">{c.unread > 99 ? '99+' : c.unread}</span> : null}
      </span>
    </button>
  )
}

/** A conversa: balões, selo "conteúdo de fora", faixa "Só você vê…", "chegou enquanto desligado" e a faixa do responder. */
function Conversa({ bot, conversa, caixa, onVoltar, ativo }: { bot: Bot; conversa: ConversaWa; caixa: CaixaWhatsapp; onVoltar?: () => void; ativo: boolean }) {
  const t = useT()
  const idioma = useIdioma()
  const id = conversa.whatsappChatId
  const estado = caixa.conversa(bot.id, id)
  useEffect(() => { void caixa.lerMensagens(bot.id, id) }, [caixa, bot.id, id])
  // Abrir marca como lida; mensagem nova com a conversa aberta também.
  useEffect(() => { if (conversa.unread > 0) void caixa.marcarLida(bot.id, id) }, [caixa, bot.id, id, conversa.unread])
  const grupo = conversa.kind === 'group'
  const [confirmar, setConfirmar] = useState(false)
  // D507: o erro fica como dado e é traduzido ao desenhar.
  const [erroApagar, setErroApagar] = useState<Mensagem | null>(null)
  const apagar = () => {
    setErroApagar(null)
    caixa.apagarConversa(bot.id, id).catch((e: unknown) => setErroApagar(e instanceof Error && e.message === SEM_CONEXAO ? { chave: 'whatsapp.conversa.semConexao' } : erroDoHub(e)))
  }
  // MX-18: a marca "responde sozinho" e o erro de tirar ficam como dado.
  const [erroMarca, setErroMarca] = useState<Mensagem | null>(null)
  const alternarMarca = () => {
    setErroMarca(null)
    caixa.marcarConversa(bot.id, id, !conversa.marked).catch((e: unknown) => setErroMarca(e instanceof Error && e.message === SEM_CONEXAO ? { chave: 'whatsapp.conversa.semConexao' } : erroDoHub(e)))
  }
  const alternarAutonomoGrupo = () => {
    setErroMarca(null)
    caixa.alterarAutonomoGrupo(bot.id, id, !conversa.groupAuto).catch((e: unknown) => setErroMarca(e instanceof Error && e.message === SEM_CONEXAO ? { chave: 'whatsapp.conversa.semConexao' } : erroDoHub(e)))
  }
  const msgs = estado?.msgs ?? []
  const itens = itensConversa(msgs, undefined, idioma)
  const scroller = useRef<HTMLDivElement>(null)
  const fim = useRef(true)
  const ultima = msgs[0]?.messageId
  useLayoutEffect(() => {
    const el = scroller.current
    if (el && fim.current) el.scrollTop = el.scrollHeight
  }, [ultima, itens.length])
  return (
    <section className="wa-conv" aria-label={t('whatsapp.caixa.conversaCom', { nome: rotuloConversa(t, conversa) })}>
      <div className="wa-cvh">
        {onVoltar ? <button className="back" title={t('whatsapp.caixa.voltar')} aria-label={t('whatsapp.caixa.voltar')} onClick={onVoltar}><Icon name="chevron.left" size={13} weight="semibold" scaled={false} /></button> : null}
        <Avatar c={conversa} size="sm" />
        <span className="who"><b className="wa-cvh-t" tabIndex={-1}><NomeConversa c={conversa} ativo={ativo} /></b><span>{grupo ? comNos(t('whatsapp.caixa.grupoSo', { bot: no('bot') }), { bot: <bdi dir="auto">{bot.name}</bdi> }) : t('whatsapp.caixa.conversa')}</span></span>
        <span className="wa-fora" title={t('whatsapp.caixa.foraDica')}><Icon name="shield" size={9} weight="semibold" scaled={false} />{t('whatsapp.caixa.fora')}</span>
        <DropdownMenu className="wa-mais" language={superficieIdioma(idioma)} title={t('whatsapp.conversa.mais')} items={() => [
          { title: t(grupo ? (conversa.marked ? 'whatsapp.caixa.desmarcarGrupo' : 'whatsapp.caixa.marcarGrupo') : (conversa.marked ? 'whatsapp.caixa.tirarMarca' : 'whatsapp.caixa.marcarSozinha')), icon: 'checkmark.circle', selected: !!conversa.marked, action: alternarMarca },
          ...(grupo && conversa.marked ? [{ title: t(conversa.groupAuto ? 'whatsapp.caixa.desligarGrupo' : 'whatsapp.caixa.ligarGrupo'), icon: 'checkmark.circle' as const, selected: !!conversa.groupAuto, action: alternarAutonomoGrupo }] : []),
          { title: t('whatsapp.conversa.apagar'), icon: 'trash', destructive: true, action: () => setConfirmar(true) },
        ]}>
          <Icon name="ellipsis" size={13} weight="semibold" scaled={false} />
        </DropdownMenu>
      </div>
      <Dialog open={confirmar} title={t('whatsapp.conversa.apagarTitulo')}
        message={t(grupo ? 'whatsapp.conversa.apagarGrupo' : 'whatsapp.conversa.apagarContato', { nome: nomeEscondido(t, conversa) })}
        cancel={t('whatsapp.estados.cancelar')} language={superficieIdioma(idioma)}
        onClose={() => setConfirmar(false)} actions={[{ title: t('whatsapp.conversa.apagarAcao'), destructive: true, action: apagar }]} />
      {erroApagar ? <p className="wa-erro-curto" role="alert"><TextoDaMensagem m={erroApagar} /></p> : null}
      {erroMarca ? <p className="wa-erro-curto" role="alert"><TextoDaMensagem m={erroMarca} /></p> : null}
      <div className="wa-guard" role="note">
        <Icon name="shield" size={12} scaled={false} />
        <span><b>{t('whatsapp.caixa.soVoce')}</b> {comNos(t('whatsapp.caixa.soVoceTexto', { bot: no('bot') }), { bot: <bdi dir="auto">{bot.name}</bdi> })}</span>
      </div>
      <div className="wa-msgs" ref={scroller} role="log" aria-label={t('whatsapp.caixa.mensagensAria')}
        onScroll={(e) => {
          const el = e.currentTarget
          fim.current = el.scrollHeight - el.scrollTop - el.clientHeight < 8
          if (el.scrollTop < 120 && estado?.next && !estado.antigas) void caixa.lerMensagens(bot.id, id, true)
        }}>
        {estado?.next ? <button className="wa-older" disabled={estado.antigas} onClick={() => void caixa.lerMensagens(bot.id, id, true)}>{estado.antigas ? t('whatsapp.caixa.carregando') : t('whatsapp.caixa.anteriores')}</button> : null}
        {estado?.erro && !msgs.length
          ? <Aviso icone="exclamationmark.triangle" titulo={t('whatsapp.caixa.naoLeuMensagens')} acao={t('whatsapp.caixa.tentar')} onAcao={() => void caixa.lerMensagens(bot.id, id)} erro><TextoDaMensagem m={estado.erro} /></Aviso>
          : !estado || (estado.carregando && !msgs.length) ? <div className="wa-loading" aria-busy>{t('whatsapp.caixa.carregando')}</div> : null}
        {itens.map((i) => i.tipo === 'dia' ? <span key={i.id} className="wa-day">{i.texto}</span>
          : i.tipo === 'desligado' ? <div key={i.id} className="wa-gap"><Icon name="clock" size={10} scaled={false} />{t('whatsapp.caixa.desligado')}</div>
          : <Balao key={i.id} m={i.m} grupo={grupo} bot={bot.name} ativo={ativo} />)}
        {estado?.erro && msgs.length ? <p className="wa-erro-curto" role="alert"><TextoDaMensagem m={estado.erro} /> <button onClick={() => void caixa.lerMensagens(bot.id, id)}>{t('whatsapp.caixa.tentar')}</button></p> : null}
      </div>
      <Escrever botId={bot.id} chat={conversa} caixa={caixa} nome={rotuloConversa(t, conversa)} />
    </section>
  )
}

/** MX-18/D379: o dono escreve no mini WhatsApp — sai na hora, fora da fila do bot, do silêncio e dos limites. */
function Escrever({ botId, chat, caixa, nome }: { botId: string; chat: ConversaWa; caixa: CaixaWhatsapp; nome: string }) {
  const t = useT()
  const [texto, setTexto] = useState('')
  const [enviando, setEnviando] = useState(false)
  const [erro, setErro] = useState<Mensagem | null>(null)
  const [incerto, setIncerto] = useState(false)
  const enviar = () => {
    const limpo = texto.trim()
    if (!limpo || enviando) return
    setEnviando(true)
    setErro(null)
    setIncerto(false)
    caixa.enviarDono(botId, chat.whatsappChatId, limpo)
      .then((r) => {
        if (r === 'uncertain') setIncerto(true)
        else { setTexto(''); setIncerto(false) }
      })
      .catch((e: unknown) => setErro(e instanceof Error && e.message === SEM_CONEXAO ? { chave: 'whatsapp.conversa.semConexao' } : erroDoHub(e)))
      .finally(() => setEnviando(false))
  }
  return (
    <div className="wa-write">
      <form className="wa-input" onSubmit={(e) => { e.preventDefault(); enviar() }}>
        <input
          type="text"
          dir="auto"
          value={texto}
          maxLength={4096}
          placeholder={t('whatsapp.caixa.escrever', { nome })}
          aria-label={t('whatsapp.caixa.escreverAria', { nome })}
          disabled={enviando}
          onChange={(e) => setTexto(e.currentTarget.value)}
        />
        <button type="submit" className="send" disabled={enviando || !texto.trim()} aria-label={t('whatsapp.caixa.enviar')} title={t('whatsapp.caixa.enviar')}>
          <Icon name="arrow.up" size={13} weight="bold" scaled={false} />
        </button>
      </form>
      {erro ? <p className="wa-erro-curto" role="alert"><TextoDaMensagem m={erro} /></p> : null}
      {incerto ? <p className="wa-erro-curto" role="alert">{t('whatsapp.caixa.envioIncerto')}</p> : null}
      <p className="wa-write-note"><Icon name="lock" size={11} scaled={false} /><span>{t('whatsapp.caixa.escreverNota')}</span></p>
    </div>
  )
}

const DICA: Record<string, 'whatsapp.caixa.dicaFoto'> = { image: 'whatsapp.caixa.dicaFoto' }

/** R3b: a nota da voz no idioma do app; o nome do provedor é marca e vai como está. */
function notaVoz(t: Tradutor, voz: NonNullable<ReturnType<typeof transcricao>>): string {
  if (voz.nota === 'transcrito') return t('whatsapp.voz.transcrito', { provedor: voz.provedor ?? '' })
  return t(`whatsapp.voz.${voz.nota}`)
}

function Balao({ m, grupo, bot, ativo }: { m: MensagemWa; grupo: boolean; bot: string; ativo: boolean }) {
  const idioma = useIdioma()
  const icone = iconeDa(m)
  const tempo = m.call ? duracao(m.call.durationSecs) : null
  const cor = corDa(m.from.phone ?? m.from.name)
  const voz = transcricao(m)
  const t = useT()
  return (
    <div className="wa-m">
      {grupo ? <span className="who" style={{ color: `var(--tag-${cor}-ink)` }}><Remetente name={m.from.name} phone={m.from.phone} contact={m.from.contact} ativo={ativo} /></span> : null}
      <div className="wa-bub">
        {grupo ? <div className="wa-quote"><b>{comNos(t('whatsapp.caixa.respondendoA', { bot: no('bot') }), { bot: <bdi dir="auto">{bot}</bdi> })}</b></div> : null}
        {icone ? (
          <span className="wa-media">
            <span className="i"><Icon name={icone} size={12} scaled={false} /></span>
            <span><b><bdi dir="auto">{m.body}</bdi>{tempo ? <> · <bdi dir="ltr">{tempo}</bdi></> : null}</b>{m.media && DICA[m.media] ? <><br />{t(DICA[m.media]!)}</> : null}</span>
          </span>
        ) : <span className="wa-text" dir="auto">{m.body}</span>}
        {voz?.texto ? <span className="wa-text wa-voz-texto" dir="auto">{voz.texto}</span> : null}
        {voz ? <span className={`wa-voz-nota${voz.erro ? ' err' : ''}`} aria-live={voz.nota === 'transcrevendo' ? 'polite' : undefined}>{notaVoz(t, voz)}</span> : null}
        <span className="t"><bdi dir="ltr">{horaMensagem(m.sentAt, idioma)}</bdi></span>
        {m.offline ? <span className="late">{t('whatsapp.caixa.chegouDesligado')}</span> : null}
      </div>
    </div>
  )
}

/** Estados vazios e de erro: ícone, título, texto e, quando há, uma ação. Erro do Hub nunca vira lista vazia. */
export function Aviso({ icone, titulo, children, acao, onAcao, erro }: { icone: string; titulo: string; children: ReactNode; acao?: string; onAcao?: () => void; erro?: boolean }) {
  return (
    <div className={`wa-empty${erro ? ' err' : ''}`} role={erro ? 'alert' : undefined}>
      <div className="box">
        <span className="big"><Icon name={icone} size={18} scaled={false} /></span>
        <h3>{titulo}</h3>
        <p>{children}</p>
        {acao && onAcao ? <button className="btn" onClick={onAcao}>{acao}</button> : null}
      </div>
    </div>
  )
}

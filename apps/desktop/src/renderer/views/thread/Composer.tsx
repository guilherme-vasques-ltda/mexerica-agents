// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { originDevice, originReport, type OriginDraft, type OriginKind } from '@shared/guard-hub'
import type { EditRange } from '@shared/composer-segments'
import { draftKey } from '@shared/composer-drafts'
import { bloqueiaEnvio, editorDe, guardarEscrita, itensLivres, pendenciaVale, resolverNaoESenha, tratarColagem, type Pendencia } from '@shared/fork-senha-colada-campo'
import { useEffect, useId, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactNode } from 'react'
import { groupFastMode } from '@shared/fast-mode'
import { isGroup, isWorkingIn, needsInput, quoteOf, type Bot, type Entry } from '@shared/models'
import { BotAvatar } from '../../components/Avatar'
import { Icon } from '../../components/Icon'
import { IconButton } from '../../components/Controls'
import { usePresence } from '../../components/Overlay'
import { HStack, VStack } from '../../components/Stack'
import { Text } from '../../components/Text'
import { listEqual, useModel, useSelector } from '../../lib/observable'
import { useComposerInput } from '../../lib/use-composer-input'
import { chatEntriesOf, useStoreRef } from '../../store/context'
import { bloqueioSenha, destinoSenha } from '../../store/fork-senha-bloqueio'
import { emQueda } from '../../store/fork-queda-curta'
import { MAX_FILE_SIZE, type OutgoingFile, type SendOptions } from '../../store/bot-store'
import { pluginsFor } from '../marketplace/plugins'
import { composerEnter } from './composer-keyboard'
import { useConversaComposer } from '../fork/conversa/preferencias'
import { readPicked, readPasted } from './fork-composer-files'
import { CommandMenu, useCommandMenu } from './CommandMenu'
import { ComposerAttachment } from './ComposerAttachment'
import { QuoteRow } from './ChatRows'
import { useDictation } from './dictation'
import { useOpenBotSettings } from '../fork/bot-settings/bot-settings-entry'
import { ApprovalModeMenu, shieldLevel } from './ApprovalModeMenu'
import { AvisoSenhaColada } from '../fork/compositor/AvisoSenhaColada'
import { abrirCofreParaLoginNovo } from '../fork/compositor/gancho-cofre'
import { AvisoCofre } from '../fork/compositor/AvisoCofre'
import { faseAoAviso, faseAoResponder, type FaseCofre } from '../fork/compositor/cofre-estado'
import { ChipModelo } from '../fork/seletor-modelo/ChipModelo'
import { ContaDaConversa } from '../fork/contas-ia/FalhaDeConta'
import { estiloMenuAprovacao } from '../fork/seletor-modelo/posicao'
import { usePresaAoChip } from '../fork/seletor-modelo/presa'
import { BarraContexto } from '../fork/contexto/BarraContexto'
import { MotivoCompactando, useCompactacao } from '../fork/contexto/Compactando'
import { bloqueiaEnvioCompactando } from '../fork/contexto/estado'
import { applyStarterDraft } from '../fork/onboarding/starter-draft'
import type { StarterDraft } from '../fork/onboarding/useStarterTour'
import { builtInCommand, newTabText, slashCommands, slashQuery, type SlashCommand } from './slash-commands'
// DON-907 (D431): a fila como bandeja no mesmo bloco do campo (módulo próprio, D71).
import { BandejaFila, BarraEdicao, StatusDaFila, useFila } from '../fork/conversa/BandejaFila'
import { superficieIdioma, useIdioma, useT } from '../../lib/fork-i18n/estado'
import '../fork/compositor/compositor.css'

type Trailing = 'interrupt' | 'stop' | 'send'

/**
 * The message box under a chat: send, or stop while the bot (or the group) is working there. A leading `/` lists
 * the commands (a group's are its bots'); in a group, typing `@` suggests its bots. While `reply` is set, the
 * message it quotes sits at the top of the box and goes out with the next send.
 */
interface StarterDraftProps {
  starterDraft?: StarterDraft | null
  onStarterDraftHandled?: (origin?: OriginDraft) => void
}

export function Composer({ botId, sessionId = null, reply = null, cancelReply, onCall, onInterrupt, onNewTab = null, starterDraft, onStarterDraftHandled }: StarterDraftProps & {
  botId: string
  /** Fork (UI-01): a extra aberta; null = a principal, que envia como antes. */
  sessionId?: string | null
  reply?: Entry | null
  cancelReply?: () => void
  /** Starts a voice call through a separate control. */
  onCall?: (() => void) | null
  /** Interrupts the bot while it is speaking in a voice call. */
  onInterrupt?: (() => void) | null
  /** `/nova-aba`: cria uma aba do mesmo bot e começa a conversa nela com o texto; null = o Hub não oferece abas aqui. */
  onNewTab?: ((text: string, options: SendOptions) => Promise<boolean>) | null
}) {
  const store = useStoreRef()
  // Remount transient editor state (selection, IME) when its destination changes (Fork UI-01: also the session).
  return <DestinationComposer key={JSON.stringify([store.contextId, store.computer.id, store.spaceId, botId, sessionId])}
    botId={botId} sessionId={sessionId} reply={reply} cancelReply={cancelReply} onCall={onCall} onInterrupt={onInterrupt} onNewTab={onNewTab} starterDraft={starterDraft} onStarterDraftHandled={onStarterDraftHandled} />
}

function DestinationComposer({ botId, sessionId, reply, cancelReply, onCall, onInterrupt, onNewTab, starterDraft, onStarterDraftHandled }: StarterDraftProps & {
  botId: string; sessionId: string | null; reply: Entry | null; cancelReply?: () => void; onCall?: (() => void) | null; onInterrupt?: (() => void) | null; onNewTab: ((text: string, options: SendOptions) => Promise<boolean>) | null
}) {
  const store = useStoreRef()
  // MX-4 (C9): a fatia do compositor — o bot, a conexão/queda, a fila (entradas), os rascunhos e os membros do grupo.
  useSelector(store, (s) => {
    const b = s.bots.get(botId) ?? null
    return [
      b, chatEntriesOf(s, botId), s.connection, s.shownConnection.kind,
      s.mismatch?.kind ?? null, s.linkCaidoDesde, s.composerDrafts, s.routineDrafts.get(botId),
      ...(b && isGroup(b) ? s.members(b) : []),
    ]
  }, listEqual)
  const plugins = useModel(pluginsFor(store))
  const bot = store.bots.get(botId) ?? null
  const drafts = store.composerDrafts
  // Fork (UI-01): cada extra tem o próprio rascunho (texto e anexos); a principal fica com a chave de sempre.
  const draftId = draftKey(botId, sessionId)
  const draft = useSyncExternalStore(drafts.subscribe, () => drafts.get(draftId))
  // MX-214: com o aviso de senha colada aberto o campo está congelado: NENHUM caminho altera o rascunho (comando, menção,
  // ditado, rotina…), para o trecho voltar sempre ao texto exato em que foi escondido. Só "Não é senha" libera.
  const congelado = useRef(false)
  const setDraft = guardarEscrita(() => congelado.current, (text: string, kind: OriginKind = 'generated', range?: EditRange) => {
    drafts.edit(draftId, text, kind, range); store.avatarMoods.typing(botId)
  })
  const files = useSyncExternalStore(drafts.subscribe, () => drafts.files(draftId))
  // Lido na hora do uso (anexo que volta de um seletor assíncrono, remover chip), nunca de um render antigo.
  const compactandoRef = useRef(false)
  const setFiles = (files: OutgoingFile[]) => { if (!compactandoRef.current) drafts.setFiles(draftId, files) }
  const mounted = useRef(false)
  useLayoutEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  const dictation = useDictation(draft, (text) => { if (!congelado.current) setDraft(text, 'voice') })
  const { field, bind, input } = useComposerInput<HTMLTextAreaElement>()
  const measure = useRef<HTMLDivElement>(null)
  /** Text pasted into this draft since the last send. */
  const pasted = useRef<string[]>([])
  // Numa extra, Parar e a fila do bot são da principal (parar por sessão é do PR 5): o botão fica sempre Enviar.
  const working = sessionId === null && !!bot && (isWorkingIn(bot, botId) || (bot.status === 'waiting' && (bot.workingChat ?? bot.id) === botId))
  // D249 R3: waiting for the person's approval the turn is still open (a message queues, Parar ends it), but the
  // approval card is the main action: the box says why it queues and Parar loses the accent.
  const awaitingApproval = !!bot && working && needsInput(bot)
  const empty = !draft.trim() && !files.length
  // Compactando (desenho aprovado, opção A): o campo vira só leitura e o rascunho fica guardado; o envio volta sozinho
  // ao terminar (ou na falha, ou no limite de espera). NENHUM caminho altera o rascunho nem os anexos enquanto dura
  // (digitar, colar, soltar, menção, comando, ditado, rotina, anexo, Enter): só acende o motivo.
  const compactando = bloqueiaEnvioCompactando(useCompactacao(bot, botId, sessionId))
  compactandoRef.current = compactando
  const motivoCompactandoId = useId()
  const [tentou, setTentou] = useState(0)
  useEffect(() => {
    if (!tentou) return
    const timer = setTimeout(() => setTentou(0), 2400)
    return () => clearTimeout(timer)
  }, [tentou])
  useEffect(() => { if (!compactando) setTentou(0) }, [compactando])
  // MX-214: a colagem que parece senha fica em quarentena SÓ neste estado local (nunca no rascunho, na fila, em log
  // nem em metadado de envio) e, enquanto o aviso está aberto, nada é enviado e o campo fica congelado. Se a caixa
  // fechar ou trocar de destino, a pendência se perde.
  const [pendencia, setPendencia] = useState<Pendencia | null>(null)
  // Uma colagem nova enquanto o aviso está aberto não entra: a pessoa resolve o aviso e cola de novo.
  const [colagemRecusada, setColagemRecusada] = useState(false)
  const bloqueado = bloqueiaEnvio(pendencia)
  congelado.current = bloqueado || compactando
  const fila = useFila(botId, sessionId, bloqueado || compactando)
  const t = useT()
  // R2 (fatia 2): o campo e seus controles já são traduzidos: declaram o idioma e a direção escolhidos. O texto da
  // pessoa (campo, citação, nomes de arquivo) segue a própria direção (`dir="auto"`).
  const superficie = superficieIdioma(useIdioma())
  const editando = fila.edicao
  const fieldDirection = (editando ? editando.texto : draft) ? 'auto' : superficie.dir
  // MX-217: sem conexão, Enviar fica desligado (também pelo Enter) e a caixa diz que o texto fica guardado.
  const semConexao = emQueda(store)
  const canSend = !empty && !bloqueado && !compactando && !semConexao && (store.connection.kind === 'online' || (store.canQueue && !files.length))
  const canAttach = !!bot

  const members = bot && isGroup(bot) ? store.members(bot) : []

  // D169 R5 (Composer, AppShell): "Mensagem para Byte" / "Mensagem para #canal", as in the reference;
  // while the bot works the box says the message waits in its queue (the real state).
  const nome = bot?.name ?? ''
  const placeholder = semConexao ? t('conversa.mensagens.queda.semEnvio') : reply ? t('conversa.compositor.placeholder.responder') : bot && isGroup(bot) ? t('conversa.compositor.placeholder.canal', { canal: bot.name.toLocaleLowerCase('pt-BR') }) : sessionId !== null ? t('conversa.compositor.placeholder.sessao', { bot: nome }) : awaitingApproval ? t('conversa.compositor.placeholder.liberacao', { bot: nome }) : t('conversa.compositor.placeholder.bot', { bot: nome })

  // A routine's "Try it" fills the box.
  const routineDraft = sessionId === null ? store.routineDrafts.get(botId) : undefined
  useEffect(() => {
    if (routineDraft === undefined || congelado.current) return
    const incoming = store.consumeRoutineDraft(botId)
    if (incoming === undefined) return
    const previous = drafts.get(botId)
    drafts.edit(botId, previous ? `${previous}\n${incoming}` : incoming, 'generated')
    field.current?.focus({ preventScroll: true })
  }, [routineDraft, botId, store, drafts, bloqueado, compactando])

  useEffect(() => {
    if (!starterDraft) return
    if (sessionId !== null || congelado.current || editando || input.composing) {
      onStarterDraftHandled?.()
      return
    }
    const origin = applyStarterDraft(drafts, botId, starterDraft.prompt, starterDraft.previous())
    onStarterDraftHandled?.(origin)
    if (origin) field.current?.focus({ preventScroll: true })
  }, [starterDraft, onStarterDraftHandled, sessionId, botId, drafts, editando, field, input])

  // Picking a message to reply to brings the caret here.
  useEffect(() => {
    if (reply) field.current?.focus({ preventScroll: true })
  }, [reply])

  // Measure a separate, wrapping copy: collapsing the live textarea to zero breaks its
  // height transition, caret scroll and IME composition. Observe font/width changes too.
  useLayoutEffect(() => {
    const el = field.current
    const mirror = measure.current
    if (!el || !mirror) return
    const resize = () => {
      const style = getComputedStyle(el)
      const line = parseFloat(style.lineHeight)
      const padding = parseFloat(style.paddingTop) + parseFloat(style.paddingBottom)
      // D165 fase E (Composer): grows with the text up to 10 lines, then scrolls inside.
      const height = Math.min(mirror.getBoundingClientRect().height, line * 10 + padding)
      el.style.height = `${height}px`
    }
    resize()
    const observer = new ResizeObserver(resize)
    observer.observe(mirror)
    return () => observer.disconnect()
  }, [])

  const mentionQuery = (() => {
    if (!bot || !isGroup(bot)) return null
    const at = draft.lastIndexOf('@')
    if (at < 0) return null
    if (at > 0 && !/\s/.test(draft[at - 1]!)) return null
    const query = draft.slice(at + 1)
    return /\n/.test(query) || query.length > 24 ? null : query
  })()
  const suggestions: Bot[] =
    mentionQuery !== null ? members.filter((m) => !mentionQuery || m.name.toLowerCase().includes(mentionQuery.toLowerCase())) : []
  const sugestoes = itensLivres(pendencia, compactando ? [] : suggestions)

  const mention = (member: Bot) => {
    if (bloqueado || compactando) return
    const at = draft.lastIndexOf('@')
    if (at < 0) return
    setDraft(`${draft.slice(0, at)}@${member.name} `)
    field.current?.focus({ preventScroll: true })
  }

  const slash = slashQuery(draft)
  const conversa = useConversaComposer()
  const commands = slash === null ? [] : slashCommands(bot, slash, plugins.installedSkills, members, conversa.slashSkills, !!onNewTab)

  const run = (command: SlashCommand) => {
    if (!bot || bloqueado || compactando) return
    switch (command.action.kind) {
      case 'fast':
        if (isGroup(bot)) {
          // Each member's own Fast mode: all on, or all back off.
          const fast = groupFastMode(t, members)
          for (const m of fast?.members ?? []) store.setFastMode(m, fast?.state === 'off')
        } else {
          store.setFastMode(bot, !bot.fastMode)
        }
        setDraft('')
        break
      case 'skill':
        setDraft(`/${command.action.id} `)
        break
      case 'newTab':
        void onNewTab?.('', {})
        setDraft('')
        break
    }
    field.current?.focus({ preventScroll: true })
  }
  const menu = useCommandMenu(draft, itensLivres(pendencia, compactando ? [] : commands), run)

  const append = () => {
    const current = drafts.fileGuard(draftId)
    return (incoming: { name: string; data: Uint8Array }[]) => {
      if (!canAttach || compactandoRef.current || !mounted.current || !current() || !store.bots.has(botId)) return
      const accepted: OutgoingFile[] = []
      for (const f of incoming) {
        if (f.data.length > MAX_FILE_SIZE) {
          store.setError(t('conversa.compositor.grandeDemais', { arquivo: f.name, limite: '100 MB' }))
          continue
        }
        accepted.push({ id: crypto.randomUUID(), name: f.name, data: f.data })
      }
      if (accepted.length) setFiles([...drafts.files(draftId), ...accepted])
    }
  }

  const submit = () => {
    if (bloqueado) return
    if (compactando) return setTentou((n) => n + 1)
    const novaAba = onNewTab && !files.length ? newTabText(draft) : null
    if (onNewTab && novaAba) {
      const texto = draft
      const full = originReport(draft, drafts.origin(draftId), originDevice())
      const shift = draft.trim().length - novaAba.length
      const segments = full.segments.flatMap((s) => (s.end > shift ? [{ ...s, start: Math.max(0, s.start - shift), end: s.end - shift }] : []))
      void onNewTab(novaAba, { pasted: pasted.current, contentOrigin: { ...full, textLength: novaAba.length, segments } }).then((ok) => {
        if (!ok && !drafts.get(draftId)) setDraft(texto)
      })
      pasted.current = []
      setDraft('')
      return
    }
    const command = files.length ? null : builtInCommand(bot, draft, members, !!onNewTab)
    if (command) return run(command)
    if (!canSend) return
    // A extra é a desta caixa, capturada agora; sem o Hub anunciar `send`, nada sai e o texto fica.
    const session = sessionId === null ? undefined : store.sessions.intent(botId, sessionId)
    if (session === null) return void store.sessions.fail(botId, sessionId, 'unsupported')
    store.send(draft, botId, { files, pasted: pasted.current, replyTo: reply && quoteOf(reply), contentOrigin: originReport(draft, drafts.origin(draftId), originDevice()), session })
    cancelReply?.()
    pasted.current = []
    setDraft('')
    setFiles([])
    field.current?.focus({ preventScroll: true })
  }

  /** Quem a caixa edita agora: o rascunho ou uma mensagem da fila. A pendência pertence a um só. */
  const editorAgora = editorDe(editando?.entry.id)
  // Cancelou ou trocou a edição: o trecho é descartado, nunca devolvido a outro editor.
  useEffect(() => {
    if (pendencia && !pendenciaVale(pendencia, editorAgora)) { setPendencia(null); setColagemRecusada(false) }
  }, [pendencia, editorAgora])
  // Os outros caminhos de envio (chamada de voz, painel, Ilha) enxergam a pendência por este registro.
  const destino = destinoSenha(store.computer.id, botId)
  // Um envio recusado por este aviso (Reenviar, voz, painel…) mostra o motivo aqui no cartão, sem diálogo de erro.
  const envioBarrado = useModel(bloqueioSenha).foiBarrado(destino)
  useEffect(() => {
    if (!bloqueado) return
    bloqueioSenha.abrir(draftId, destino)
    return () => bloqueioSenha.fechar(draftId)
  }, [bloqueado, draftId, destino])
  // Um ditado em andamento termina ao entrar na quarentena: não continua sobre o texto antigo depois.
  useEffect(() => { if ((bloqueado || compactando) && dictation.listening) dictation.toggle() }, [bloqueado, compactando, dictation.listening])
  const encerrarPendencia = () => { setPendencia(null); setColagemRecusada(false) }
  // MX-277: depois de "Mover para o cofre" o cartão acompanha a janela segura (aberta → guardado), só com estado.
  const [cofre, setCofre] = useState<FaseCofre | null>(null)
  useEffect(() => {
    if (cofre === null) return
    return window.codync.forkCofre.onEstado((aviso) => setCofre((atual) => faseAoAviso(atual, aviso)))
  }, [cofre === null])
  /** O texto da caixa agora (o da mensagem da fila, se uma está em edição). */
  const textoAgora = () => (editando ? editando.texto : drafts.get(draftId))
  const moverCursor = (posicao: number) => requestAnimationFrame(() => field.current?.setSelectionRange(posicao, posicao))
  // "Não é senha": os trechos voltam ao campo como colados, no mesmo editor em que foram escondidos.
  const naoESenha = () => {
    if (!pendencia) return
    // Compactando, o campo é imutável: a quarentena espera (o trecho continua escondido) até o envio voltar.
    if (compactandoRef.current) return setTentou((n) => n + 1)
    const r = resolverNaoESenha(pendencia, editorAgora, textoAgora())
    if (r.acao === 'descartar') return encerrarPendencia()
    congelado.current = false // só aqui o campo é alterado com a pendência aberta
    if (editando) fila.texto(r.texto)
    else {
      setDraft(r.texto, 'pasted')
      for (const v of r.valores) pasted.current.push(v)
    }
    encerrarPendencia()
    field.current?.focus({ preventScroll: true })
    moverCursor(r.fim)
  }
  // "Mover para o cofre": os trechos são DESCARTADOS aqui (nunca saem do chat, nem por IPC) e o gancho abre a janela
  // segura VAZIA; a pessoa cola a senha de novo, direto nela. Relay (nuvem) não entra: o main também recusa.
  const moverParaCofre = () => {
    encerrarPendencia()
    if (!store.isLoopback) return setCofre('semSuporte')
    setCofre('abrindo')
    void abrirCofreParaLoginNovo(store.computer.id).then((r) => setCofre((atual) => (atual === 'guardado' ? atual : faseAoResponder(r))))
  }

  const trailing: Trailing = onInterrupt && !draft && !files.length ? 'interrupt' : working && !draft && !files.length ? 'stop' : 'send'
  const [symbol, label] = ({
    interrupt: ['stop.fill', t('conversa.compositor.interromper')],
    stop: ['stop.fill', t('conversa.compositor.parar')],
    send: ['mx-enviar', t('conversa.compositor.enviar')],
  } as const)[trailing]
  // Compactando, nem Parar vale: parar cancelaria a compactação e deixaria a conversa cheia sem a pessoa saber por quê.
  const enabled = trailing === 'interrupt' || (!compactando && (trailing !== 'send' || canSend))

  return (
    <VStack>
      <StatusDaFila fila={fila} bot={bot} />
      <ComposerReveal open={sugestoes.length > 0}>
        <div className="mentions">
          {sugestoes.map((m) => (
            <button key={m.id} className="press mention" onClick={() => mention(m)}>
              <BotAvatar bot={m} size={24} animated={false} />
              <Text size="subheadline" tone="text">{m.name}</Text>
            </button>
          ))}
        </div>
      </ComposerReveal>
      <ComposerReveal open={bloqueado || cofre !== null}>
        {bloqueado
          ? <AvisoSenhaColada nomeBot={bot && !isGroup(bot) ? bot.name : null} recusada={colagemRecusada} envioBarrado={envioBarrado} mover={moverParaCofre} naoESenha={naoESenha} />
          : cofre !== null ? <AvisoCofre fase={cofre} fechar={() => setCofre(null)} /> : null}
      </ComposerReveal>
      <ComposerReveal open={compactando}>
        <MotivoCompactando id={motivoCompactandoId} destaque={tentou > 0} />
      </ComposerReveal>
      <div
        className="composer idioma-superficie"
        {...superficie}
        onDragOver={(e) => {
          if (canAttach && !compactando && e.dataTransfer.types.includes('Files')) e.preventDefault()
        }}
        onDrop={(e) => {
          if (bloqueado || compactando) { e.preventDefault(); return }
          if (!e.dataTransfer.files.length) {
            const text = e.dataTransfer.getData('text/plain')
            if (text) pasted.current.push(text.replace(/\r\n?/g, '\n'))
            return
          }
          if (!canAttach || !e.dataTransfer.files.length) return
          e.preventDefault()
          void readPicked([...e.dataTransfer.files]).then(append())
        }}
      >
        <BandejaFila fila={fila} />
        <div className="composer-surface">
          <BarraEdicao fila={fila} />
          <CommandMenu commands={menu.shown} index={menu.index} hover={menu.hover} pick={run} />
          <ComposerReveal open={!!reply}>
            {reply ? (
              <HStack gap={4} className="composer-reply">
                <QuoteRow quote={quoteOf(reply)} className="composer-reply-quote" />
                <IconButton title={t('conversa.compositor.cancelarResposta')} icon="xmark" size={24} onClick={cancelReply} />
              </HStack>
            ) : null}
          </ComposerReveal>
          <ComposerReveal open={files.length > 0}>
            <div className="file-chips">
              {files.map((f) => (
                <ComposerAttachment key={f.id} file={f} remove={() => setFiles(files.filter((x) => x.id !== f.id))} />
              ))}
            </div>
          </ComposerReveal>
          <div className="composer-box">
            {/* One capsule: attachments, the field, and send / stop / call. */}
            {canAttach ? (
              <button className="press composer-add" disabled={compactando} aria-label={t('conversa.compositor.anexar')} title={t('conversa.compositor.anexar')} onClick={() => void window.codync.app.pickFiles().then(append())}>
                <Icon name="plus" size={16} color="var(--context-ink-2, var(--ink-2))" />
              </button>
            ) : null}
            <div className="composer-field">
              <div ref={measure} className="composer-measure" aria-hidden="true" dir={fieldDirection}>{(editando ? editando.texto : draft) + '\u200b'}</div>
              <textarea
                ref={bind}
                dir={fieldDirection}
                rows={1}
                value={editando ? editando.texto : draft}
                readOnly={bloqueado || compactando}
                aria-readonly={compactando || undefined}
                aria-describedby={compactando ? motivoCompactandoId : undefined}
                placeholder={placeholder}
                spellCheck
                aria-label={t(reply ? 'conversa.compositor.campo.responder' : 'conversa.compositor.campo.mensagem')}
                aria-keyshortcuts={conversa.shortcuts}
                onChange={(e) => {
                  if (compactando) return
                  if (editando) return fila.texto(e.target.value)
                  const previous = drafts.get(draftId)
                  const { kind, range } = input.change(previous, e.target.value)
                  setDraft(e.target.value, kind, range)
                  // Starting a command: list the computer's skills again.
                  if (bot && e.target.value === '/') void plugins.refresh()
                }}
                onKeyDown={(e) => {
                  const composing = input.composing || e.nativeEvent.isComposing || e.keyCode === 229
                  if (editando) {
                    // Editando uma da fila: Esc cancela, o Enter de enviar grava no mesmo lugar.
                    if (e.key === 'Escape') { e.preventDefault(); fila.cancelar(); return }
                    if (composerEnter({ key: e.key, shiftKey: e.shiftKey, altKey: e.altKey, metaKey: e.metaKey, ctrlKey: e.ctrlKey, repeat: e.repeat, isComposing: composing, keyCode: e.keyCode }, conversa.sendMode, editando.texto.includes('\n')) === 'send') {
                      e.preventDefault()
                      if (compactando) setTentou((n) => n + 1)
                      else void fila.salvar()
                    }
                    return
                  }
                  if (menu.onKey(e, composing)) return
                  if (fila.tecla({ key: e.key, shiftKey: e.shiftKey, altKey: e.altKey, metaKey: e.metaKey, ctrlKey: e.ctrlKey, isComposing: composing }, !draft && !files.length)) {
                    e.preventDefault()
                    return
                  }
                  if (e.key === 'Escape' && reply) {
                    e.preventDefault()
                    cancelReply?.()
                    return
                  }
                  // Return sends, Shift-Return adds a line; an input method's Return picks its candidate.
                  const action = composerEnter({ key: e.key, shiftKey: e.shiftKey, altKey: e.altKey, metaKey: e.metaKey, ctrlKey: e.ctrlKey, repeat: e.repeat, isComposing: input.composing || e.nativeEvent.isComposing, keyCode: e.keyCode }, conversa.sendMode, draft.includes('\n'))
                  if (action === 'ignore' && e.key === 'Enter' && e.repeat) e.preventDefault()
                  if (action === 'send') {
                    e.preventDefault()
                    submit()
                  }
                }}
                onPaste={(e) => {
                  // Compactando: nada entra por colagem (texto, trecho que parece senha, arquivo ou imagem).
                  if (compactando) { e.preventDefault(); setTentou((n) => n + 1); return }
                  const types = [...e.clipboardData.types]
                  const captured = [...e.clipboardData.files]
                  const hasFiles = types.includes('Files')
                  // MX-214: com o aviso aberto o campo está congelado: nenhuma colagem de texto entra (a pessoa decide e cola de novo).
                  if (types.includes('text/plain') && !captured.length) {
                    if (bloqueado) { e.preventDefault(); setColagemRecusada(true); return }
                    // O que parece senha não entra no campo: TODOS os trechos ficam em quarentena e o aviso decide.
                    const colado = e.clipboardData.getData('text/plain').replace(/\r\n?/g, '\n')
                    const tratada = tratarColagem(textoAgora(), e.currentTarget.selectionStart, e.currentTarget.selectionEnd, colado)
                    if (tratada) {
                      e.preventDefault()
                      setPendencia({ editor: editorAgora, pedacos: tratada.pedacos })
                      if (editando) fila.texto(tratada.texto)
                      else {
                        setDraft(tratada.texto, 'pasted')
                        if (tratada.inserido) pasted.current.push(tratada.inserido)
                      }
                      moverCursor(tratada.cursor)
                      return
                    }
                  }
                  // Pasted text is sent marked as pasted: memory never takes it as the user's own words.
                  if (types.includes('text/plain') && !captured.length) {
                    pasted.current.push(e.clipboardData.getData('text/plain').replace(/\r\n?/g, '\n'))
                  }
                  // ⌘V with a copied file or image: attach it (the field alone would paste a name).
                  if (!canAttach) return
                  if (!hasFiles || (types.includes('text/plain') && !captured.length)) return
                  e.preventDefault()
                  void readPasted(captured, () => window.codync.app.readClipboardFiles()).then(append())
                }}
              />
            </div>
            {bloqueado ? (
              <span className="composer-escondida" role="img" aria-label={t('conversa.compositor.senha.escondidaAria')}>
                <Icon name="eye.slash" size={14} />
                <span className="composer-escondida-pontos" aria-hidden="true">••••</span>
                <span aria-hidden="true">{t('conversa.compositor.senha.escondida')}</span>
              </span>
            ) : null}
            {dictation.available ? (
              <button
                className={`press composer-mic ${dictation.listening ? 'on' : ''}`}
                aria-label={t(dictation.listening ? 'conversa.compositor.pararDitado' : 'conversa.compositor.ditar')}
                title={t(dictation.listening ? 'conversa.compositor.pararDitado' : 'conversa.compositor.ditar')}
                aria-pressed={dictation.listening}
                disabled={bloqueado || compactando}
                onClick={dictation.toggle}
              >
                <Icon name="mx-gravar-audio" size={16} weight="medium" />
              </button>
            ) : null}
            {onCall && !editando ? (
              <button className="press composer-mic composer-call" aria-label={t('conversa.compositor.aoVivo')} title={t('conversa.compositor.aoVivo')} onClick={onCall}>
                <Icon name="mx-voz-ao-vivo" size={16} weight="medium" />
              </button>
            ) : null}
            {editando ? (
              <button className="press composer-send is-salvar" disabled={bloqueado || compactando || (!editando.texto.trim() && !editando.entry.data.attachments?.length)} onClick={() => void fila.salvar()}>
                {t('conversa.fila.salvar')}
              </button>
            ) : null}
            <button
              hidden={!!editando}
              // D169 (28/10): the mode as `is-*`: a bare `call` also matched the call screen's `.call` (views/call), whose
              // 12px side padding left the button 8px inside and pushed the waveform half out of the circle.
              className={`press composer-send is-${trailing}${trailing === 'stop' && awaitingApproval ? ' is-waiting' : ''}`}
              disabled={!enabled}
              aria-label={label}
              title={label}
              onClick={() => {
                if (trailing === 'interrupt') onInterrupt?.()
                else if (trailing === 'stop') store.stop(botId)
                else submit()
              }}
            >
              {(['stop.fill', 'mx-enviar'] as const).map((name) => (
                <span key={name} aria-hidden="true" className={`composer-symbol ${name === symbol ? 'active' : ''}`}>
                  <Icon name={name} size={name === 'stop.fill' ? 13 : 16} weight="bold" />
                </span>
              ))}
            </button>
          </div>
        </div>
      </div>
      {/* D473: o nível de aprovação e o chip do modelo descem para a linha do contexto, à esquerda. */}
      <BarraContexto botId={botId} sessionId={sessionId} inicio={bot && !isGroup(bot) ? <><ApprovalShield bot={bot} /><ChipModelo bot={bot} sessionId={sessionId} /><ContaDaConversa bot={bot} sessionId={sessionId} /></> : null} />
    </VStack>
  )
}

/**
 * D165 fase E, D169 (ApprovalModeMenu): the shield shows how this bot's actions are approved (the Hub's
 * legacy `permission`) and opens the reference's menu. It never changes the level from here: the other
 * level and "Configurações de aprovação" open the bot's settings on Comportamento, where the level is
 * chosen with that screen's own checks. The new levels stay "Em breve" until the Hub applies them.
 */
function ApprovalShield({ bot }: { bot: Bot }) {
  const store = useStoreRef()
  const openSettings = useOpenBotSettings()
  const [menu, setMenu] = useState(false)
  const button = useRef<HTMLButtonElement>(null)
  const t = useT()
  const level = shieldLevel(bot.permission)
  const label = t('conversa.aprovacao.escudo', { nivel: t(`conversa.aprovacao.nivel.${level.id}.titulo`), bot: bot.name })
  const close = () => { setMenu(false); button.current?.focus() }
  // D473 R1: o menu preso ao escudo, medido depois do layout e refeito enquanto aberto (fork/seletor-modelo/presa.ts).
  const place = usePresaAoChip(menu, button, estiloMenuAprovacao)
  return (
    <>
      <button ref={button} className="press composer-mode" aria-label={label} title={label} aria-haspopup="menu" aria-expanded={menu}
        disabled={!openSettings} onClick={() => setMenu((m) => !m)}>
        <Icon name={level.icon} size={16} weight="medium" />
      </button>
      <ApprovalModeMenu open={menu} anchor={button} botName={bot.name} permission={bot.permission} onClose={close}
        onSettings={() => openSettings?.(store, bot.id, 'behaviour')} place={place} />
    </>
  )
}

/** Keep the outgoing row until its height/fade transition finishes. */
function ComposerReveal({ open, children }: { open: boolean; children: ReactNode }) {
  const { mounted, shown } = usePresence(open)
  const last = useRef(children)
  if (open) last.current = children
  return <div className={`composer-reveal ${shown ? 'shown' : ''}`} inert={!open}>
    <div>{mounted ? (open ? children : last.current) : null}</div>
  </div>
}

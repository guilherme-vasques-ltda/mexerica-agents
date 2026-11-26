import { useCallback, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { isGroup, type Bot } from '@shared/models'
import {
  calcularEncaixeLargura,
  escopoDoSpace,
  MAX_PAINEIS,
  mesmaRefLadoALado,
  MIN_LARGURA_PAINEL,
  paineisVisiveis,
  type LadoALadoEscopo,
  type LadoALadoEscopoEstado,
  type LadoALadoPainel,
  type LadoALadoRef,
} from '@shared/fork-lado-a-lado'
import { BotAvatar, GroupAvatar } from '../../../components/Avatar'
import { Spinner } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { AnchoredMenu, IconButtonBase, type MenuItem } from '../../../components/Overlay'
import { Text } from '../../../components/Text'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado'
import type { AppModel, RosterItem } from '../../../store/app-model'
import type { BotStore } from '../../../store/bot-store'
import { StoreContext } from '../../../store/context'
import type { LadoALadoAviso } from '../../../store/fork-lado-a-lado'
import { sessionTitle, splitSessions } from '../sessoes/estado'
import { PersonalReadingVisible } from '../spaces/reading-visibility'
import { ThreadView } from '../../thread/ThreadView'
import {
  anuncioMudancaPaineis,
  opcoesDeConversa,
  opcoesDeConviteVazio,
  painelContemOpcao,
  rotuloDoPainel,
  type CandidatoFonte,
} from './quadro-helpers'
import './lado-a-lado.css'

export interface QuadroLadoALadoProps {
  app: AppModel
  escopo: LadoALadoEscopo | null
  estado: LadoALadoEscopoEstado
  itens: readonly RosterItem[]
  aviso: LadoALadoAviso | null
  onLimparAviso: () => void
  sidebarOpen: boolean
  sidebarButtonInset: boolean
  onCollapseSidebar: () => void
  onAbrir: (ref: LadoALadoRef, sessionId?: string | null) => void
  onRemover: (panelId: string) => void
  onTrocar: (panelId: string, ref: LadoALadoRef, sessionId?: string | null) => void
  onAtualizarSessao: (panelId: string, sessionId: string | null) => void
  onMover: (panelId: string, delta: -1 | 1) => void
  onFocar: (panelId: string) => void
  onCiclarFoco: (delta: -1 | 1) => string | null
  onSair: (focusRef?: LadoALadoRef, sessionId?: string | null) => void
}

function AvatarConversa({ bot, store, size }: { bot: Bot; store: BotStore; size: number }) {
  if (isGroup(bot)) {
    return <GroupAvatar members={store.members(bot)} size={size} animated={false} />
  }
  return <BotAvatar bot={bot} size={size} animated={false} />
}

export function QuadroLadoALado({
  app,
  escopo,
  estado,
  itens,
  aviso,
  onLimparAviso,
  sidebarOpen,
  sidebarButtonInset,
  onCollapseSidebar,
  onAbrir,
  onRemover,
  onTrocar,
  onAtualizarSessao,
  onMover,
  onFocar,
  onCiclarFoco,
  onSair,
}: QuadroLadoALadoProps) {
  const t = useT()
  const idioma = useIdioma()
  const superficie = superficieIdioma(idioma)
  const leituraVisivel = useContext(PersonalReadingVisible)
  const containerRef = useRef<HTMLElement>(null)
  const addBtnRef = useRef<HTMLButtonElement>(null)
  const panelRefs = useRef(new Map<string, HTMLElement>())
  const pendingFocusIdRef = useRef<string | null>(null)
  const [focusTick, setFocusTick] = useState(0)
  const [addMenuOpen, setAddMenuOpen] = useState(false)
  const [largura, setLargura] = useState(1080)
  const [anuncio, setAnuncio] = useState('')

  useLayoutEffect(() => {
    const el = containerRef.current
    if (!el) return
    const medir = (valor: number) => {
      const arredondado = Math.round(valor)
      if (arredondado > 0) setLargura((atual) => (atual === arredondado ? atual : arredondado))
    }
    medir(el.clientWidth)
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0]
      if (entry) medir(entry.contentRect.width)
    })
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const encaixe = useMemo(
    () => calcularEncaixeLargura(largura, estado.paineis.length),
    [largura, estado.paineis.length],
  )
  const visiveis = useMemo(
    () => paineisVisiveis(estado.paineis, estado.focoId, encaixe.cabem),
    [estado.paineis, estado.focoId, encaixe.cabem],
  )
  const visiveisIds = useMemo(() => new Set(visiveis.map((p) => p.id)), [visiveis])
  const painelAtivo = useMemo(
    () => estado.paineis.find((p) => p.id === estado.focoId) ?? visiveis[0] ?? estado.paineis[0] ?? null,
    [estado.paineis, estado.focoId, visiveis],
  )

  const fontes = useMemo<CandidatoFonte<RosterItem>[]>(
    () =>
      itens.map((item) => {
        const list = item.store.sessions.of(item.bot.id) ?? []
        const abertas = splitSessions(list).open
        return {
          ref: item.ref,
          botId: item.bot.id,
          botName: item.bot.name,
          bot: item,
          sessoesAbertas: abertas.map((s) => ({ id: s.id, title: sessionTitle(s, t) })),
        }
      }),
    [itens, t],
  )

  const opcoes = useMemo(() => opcoesDeConversa(fontes), [fontes])
  const convites = useMemo(
    () => (estado.paineis.length === 1 && largura >= MIN_LARGURA_PAINEL * 2 ? opcoesDeConviteVazio(fontes, estado.paineis, 6) : []),
    [fontes, estado.paineis, largura],
  )
  const mostrarVazio = estado.paineis.length === 1 && largura >= MIN_LARGURA_PAINEL * 2

  const resolverConversa = useCallback(
    (ref: LadoALadoRef, sessionId: string | null) => {
      const item = itens.find((i) => mesmaRefLadoALado(i.ref, ref))
      const store = item?.store ?? app.spaceMirrors.resolve(ref)
      const bot = item?.bot ?? store?.bots.get(ref.botId) ?? null
      let sessaoNome: string | null = null
      if (sessionId !== null && store && bot) {
        const s = store.sessions.of(bot.id)?.find((row) => row.id === sessionId)
        sessaoNome = s ? sessionTitle(s, t) : null
      }
      return { store, bot, botName: bot?.name ?? ref.botId, sessaoNome }
    },
    [itens, app, t],
  )

  const nomePorRef = useCallback(
    (ref: LadoALadoRef) => resolverConversa(ref, null).botName,
    [resolverConversa],
  )

  // Anúncios de leitor de tela ao adicionar ou fechar painéis no mesmo escopo.
  const escopoKey = escopoDoSpace(escopo)
  const prevEscopoKeyRef = useRef(escopoKey)
  const prevPaineisRef = useRef(estado.paineis)
  const onLimparAvisoRef = useRef(onLimparAviso)
  onLimparAvisoRef.current = onLimparAviso
  useEffect(() => {
    if (prevEscopoKeyRef.current !== escopoKey) {
      prevEscopoKeyRef.current = escopoKey
      prevPaineisRef.current = estado.paineis
      setAnuncio('')
      onLimparAvisoRef.current()
      return
    }
    const msg = anuncioMudancaPaineis(prevPaineisRef.current, estado.paineis, nomePorRef, t)
    prevPaineisRef.current = estado.paineis
    if (msg) setAnuncio(msg)
  }, [escopoKey, estado.paineis, nomePorRef, t])

  // Foca o compositor (ou o cabeçalho) após F6 / troca de pílula estreita.
  useEffect(() => {
    const targetId = pendingFocusIdRef.current
    if (!targetId) return
    pendingFocusIdRef.current = null
    const frame = requestAnimationFrame(() => {
      const el = panelRefs.current.get(targetId)
      if (!el) return
      const alvo =
        el.querySelector<HTMLElement>('.composer textarea')
        ?? el.querySelector<HTMLElement>('.lado-a-lado-painel-topo button:not(:disabled)')
        ?? el.querySelector<HTMLElement>('.lado-a-lado-painel-topo')
      alvo?.focus({ preventScroll: true })
    })
    return () => cancelAnimationFrame(frame)
  }, [focusTick, estado.focoId, visiveis])

  const focarComTeclado = useCallback(
    (panelId: string) => {
      onFocar(panelId)
      pendingFocusIdRef.current = panelId
      setFocusTick((n) => n + 1)
    },
    [onFocar],
  )

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'F6' || e.altKey || e.ctrlKey || e.metaKey || e.defaultPrevented) return
      if (estado.paineis.length < 2) return
      if (containerRef.current?.closest('[inert]')) return
      if (document.querySelector('[role="dialog"], [role="alertdialog"], [role="menu"], [aria-modal="true"], .context-layer')) return
      e.preventDefault()
      const proximoId = onCiclarFoco(e.shiftKey ? -1 : 1)
      if (proximoId) {
        pendingFocusIdRef.current = proximoId
        setFocusTick((n) => n + 1)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [estado.paineis.length, onCiclarFoco])

  const buildMenuItems = useCallback(
    (
      isSelected: (ref: LadoALadoRef, sessionId: string | null) => boolean,
      onPick: (ref: LadoALadoRef, sessionId: string | null) => void,
    ): MenuItem[] => {
      if (opcoes.length === 0) {
        return [{ title: t('conversa.ladoALado.semConversas'), disabled: true, action: () => {} }]
      }
      return opcoes.map((op, idx) => ({
        title: op.botName,
        detail: op.principal ? (op.temExtras ? t('conversa.ladoALado.principal') : undefined) : (op.sessionTitle ?? undefined),
        leading: <AvatarConversa bot={op.bot.bot} store={op.bot.store} size={18} />,
        selected: isSelected(op.ref, op.sessionId),
        divider: idx > 0 && op.principal && opcoes[idx - 1]?.temExtras,
        action: () => onPick(op.ref, op.sessionId),
      }))
    },
    [opcoes, t],
  )

  const addMenuItems = useCallback(
    () => buildMenuItems((ref, sid) => painelContemOpcao(estado.paineis, ref, sid), (ref, sid) => onAbrir(ref, sid)),
    [buildMenuItems, estado.paineis, onAbrir],
  )

  const noLimite = estado.paineis.length >= MAX_PAINEIS

  return (
    <section
      ref={containerRef}
      className={`lado-a-lado idioma-superficie${sidebarButtonInset ? ' com-inset-barra' : ''}`}
      role="region"
      aria-label={t('conversa.ladoALado.regiao')}
      {...superficie}
    >
      <div className="lado-a-lado-sr" role="status" aria-live="polite" aria-atomic="true">
        {anuncio}
      </div>

      <header className="lado-a-lado-barra no-drag">
        <div className="lado-a-lado-barra-inicio" title={t('conversa.ladoALado.atalhoPainel')}>
          <Text as="span" size="callout" weight="semibold" truncate>
            {t('conversa.ladoALado.titulo')}
          </Text>
          <Text as="span" size="caption" tone="secondary" tabular className="lado-a-lado-contagem">
            {t('conversa.ladoALado.contagem', { atual: String(estado.paineis.length), maximo: String(MAX_PAINEIS) })}
          </Text>
        </div>

        {encaixe.estreita ? (
          <div className="lado-a-lado-pilulas" role="group" aria-label={t('conversa.ladoALado.atalhoPainel')}>
            {estado.paineis.map((p) => {
              const { store, bot, botName, sessaoNome } = resolverConversa(p.ref, p.sessionId)
              const ativo = p.id === painelAtivo?.id
              const visivel = visiveis.some((v) => v.id === p.id)
              return (
                <button
                  key={p.id}
                  type="button"
                  className={`lado-a-lado-pilula${ativo ? ' is-ativo' : ''}${visivel ? ' is-visivel' : ''}`}
                  aria-pressed={ativo}
                  title={rotuloDoPainel(botName, p.sessionId, sessaoNome, t)}
                  onClick={() => focarComTeclado(p.id)}
                >
                  {store && bot ? <AvatarConversa bot={bot} store={store} size={15} /> : null}
                  <span className="lado-a-lado-pilula-nome truncate">
                    <bdi dir="auto">{botName}</bdi>
                  </span>
                  {sessaoNome ? (
                    <span className="lado-a-lado-pilula-sessao truncate">
                      <bdi dir="auto">{sessaoNome}</bdi>
                    </span>
                  ) : null}
                </button>
              )
            })}
          </div>
        ) : null}

        <div className="lado-a-lado-barra-acoes">
          <button
            ref={addBtnRef}
            type="button"
            className="lado-a-lado-btn-adicionar"
            data-limite={noLimite || undefined}
            title={t('conversa.ladoALado.adicionar')}
            aria-label={t('conversa.ladoALado.adicionar')}
            aria-haspopup="menu"
            aria-expanded={addMenuOpen}
            onClick={() => setAddMenuOpen((o) => !o)}
          >
            <Icon name="plus" size={12} weight="semibold" />
            <Text as="span" size="caption" weight="medium">
              {t('conversa.ladoALado.adicionarCurto')}
            </Text>
          </button>
          <AnchoredMenu
            open={addMenuOpen}
            onClose={() => setAddMenuOpen(false)}
            anchor={addBtnRef}
            items={addMenuItems}
            language={superficie}
            focusMenu
            menuLabel={t('conversa.ladoALado.adicionar')}
          />

          <button
            type="button"
            className="lado-a-lado-btn-sair"
            title={t('conversa.ladoALado.sair')}
            aria-label={t('conversa.ladoALado.sair')}
            onClick={() => onSair(painelAtivo?.ref, painelAtivo?.sessionId ?? null)}
          >
            <Icon name="arrow.down.right.and.arrow.up.left" size={12} weight="medium" />
            <Text as="span" size="caption" weight="medium" className="lado-a-lado-btn-sair-texto">
              {t('conversa.ladoALado.sair')}
            </Text>
          </button>
        </div>
      </header>

      {encaixe.estreita || aviso !== null ? (
        <div className="lado-a-lado-avisos">
          {encaixe.estreita ? (
            <div className="lado-a-lado-aviso" role="status" aria-live="polite">
              <Icon name="exclamationmark.triangle" size={13} className="lado-a-lado-aviso-icone" />
              <Text as="span" size="caption" tone="secondary" grow>
                {encaixe.modo === 'unico'
                  ? t('conversa.ladoALado.janelaEstreitaUnico')
                  : t('conversa.ladoALado.janelaEstreitaParcial', {
                      cabem: String(encaixe.cabem),
                      total: String(estado.paineis.length),
                    })}
              </Text>
              <div className="lado-a-lado-aviso-acoes">
                {sidebarOpen ? (
                  <button type="button" className="lado-a-lado-aviso-acao" onClick={onCollapseSidebar}>
                    {t('conversa.ladoALado.recolherBarra')}
                  </button>
                ) : null}
                <button type="button" className="lado-a-lado-aviso-acao" onClick={() => onSair(painelAtivo?.ref, painelAtivo?.sessionId ?? null)}>
                  {t('conversa.ladoALado.sair')}
                </button>
              </div>
            </div>
          ) : null}

          {aviso !== null ? (
            <div className="lado-a-lado-aviso is-alerta" role="status" aria-live="polite">
              <Icon name="info.circle" size={13} className="lado-a-lado-aviso-icone" />
              <Text as="span" size="caption" tone="text" grow>
                {aviso.kind === 'limite'
                  ? t('conversa.ladoALado.avisoLimite', { maximo: String(aviso.maximo) })
                  : aviso.count === 1
                    ? t('conversa.ladoALado.avisoRemovida')
                    : t('conversa.ladoALado.avisoRemovidas', { n: String(aviso.count) })}
              </Text>
              <IconButtonBase
                size={22}
                icon="xmark"
                title={t('conversa.ladoALado.fecharAviso')}
                onClick={onLimparAviso}
              />
            </div>
          ) : null}
        </div>
      ) : null}

      <div className="lado-a-lado-grade">
        {estado.paineis.map((painel, idxGlobal) => {
          const { store, bot, botName, sessaoNome } = resolverConversa(painel.ref, painel.sessionId)
          const visivel = visiveisIds.has(painel.id)
          const ativo = visivel && painel.id === painelAtivo?.id
          return (
            <PainelColuna
              key={painel.id}
              painel={painel}
              store={store}
              bot={bot}
              botName={botName}
              sessaoNome={sessaoNome}
              ativo={ativo}
              visivel={visivel}
              leituraVisivel={leituraVisivel}
              totalPaineis={estado.paineis.length}
              indice={idxGlobal}
              panelRefs={panelRefs}
              buildMenuItems={buildMenuItems}
              onFocar={onFocar}
              onTrocar={onTrocar}
              onAtualizarSessao={onAtualizarSessao}
              onMover={onMover}
              onRemover={onRemover}
              onSair={onSair}
              superficie={superficie}
            />
          )
        })}

        {mostrarVazio ? (
          <aside className="lado-a-lado-vazio" aria-label={t('conversa.ladoALado.vazioTitulo')}>
            <div className="lado-a-lado-vazio-cartao">
              <div className="lado-a-lado-vazio-icone" aria-hidden>
                <Icon name="donna.ladoalado" size={24} weight="light" />
              </div>
              <Text as="h2" size="title3" weight="semibold" tone="text">
                {t('conversa.ladoALado.vazioTitulo')}
              </Text>
              <Text as="div" size="callout" tone="secondary" className="lado-a-lado-vazio-detalhe">
                {t('conversa.ladoALado.vazioDetalhe')}
              </Text>
              {convites.length > 0 ? (
                <div className="lado-a-lado-vazio-lista">
                  {convites.map((c) => (
                    <button
                      key={`${c.ref.computerId}/${c.ref.spaceId ?? ''}/${c.botId}`}
                      type="button"
                      className="lado-a-lado-vazio-item"
                      onClick={() => onAbrir(c.ref, null)}
                    >
                      <AvatarConversa bot={c.bot.bot} store={c.bot.store} size={22} />
                      <Text as="span" size="callout" weight="medium" truncate>
                        <bdi dir="auto">{c.botName}</bdi>
                      </Text>
                      <Icon name="plus" size={12} className="lado-a-lado-vazio-item-plus" />
                    </button>
                  ))}
                </div>
              ) : (
                <Text as="div" size="caption" tone="secondary">
                  {t('conversa.ladoALado.semConversas')}
                </Text>
              )}
            </div>
          </aside>
        ) : null}
      </div>
    </section>
  )
}

function PainelColuna({
  painel,
  store,
  bot,
  botName,
  sessaoNome,
  ativo,
  visivel,
  leituraVisivel,
  totalPaineis,
  indice,
  panelRefs,
  buildMenuItems,
  onFocar,
  onTrocar,
  onAtualizarSessao,
  onMover,
  onRemover,
  onSair,
  superficie,
}: {
  painel: LadoALadoPainel
  store: BotStore | null
  bot: Bot | null
  botName: string
  sessaoNome: string | null
  ativo: boolean
  visivel: boolean
  leituraVisivel: boolean
  totalPaineis: number
  indice: number
  panelRefs: React.RefObject<Map<string, HTMLElement>>
  buildMenuItems: (
    isSelected: (ref: LadoALadoRef, sessionId: string | null) => boolean,
    onPick: (ref: LadoALadoRef, sessionId: string | null) => void,
  ) => MenuItem[]
  onFocar: (panelId: string) => void
  onTrocar: (panelId: string, ref: LadoALadoRef, sessionId?: string | null) => void
  onAtualizarSessao: (panelId: string, sessionId: string | null) => void
  onMover: (panelId: string, delta: -1 | 1) => void
  onRemover: (panelId: string) => void
  onSair: (focusRef?: LadoALadoRef, sessionId?: string | null) => void
  superficie: { lang: string; dir: 'ltr' | 'rtl' }
}) {
  const t = useT()
  const swapBtnRef = useRef<HTMLButtonElement>(null)
  const [swapOpen, setSwapOpen] = useState(false)
  // Em RTL a linha começa pela direita: mover "para a esquerda" é avançar no índice.
  const rtl = superficie.dir === 'rtl'
  const deltaEsquerda: -1 | 1 = rtl ? 1 : -1
  const deltaDireita: -1 | 1 = rtl ? -1 : 1
  const swapItems = useCallback(
    () =>
      buildMenuItems(
        (ref, sid) => mesmaRefLadoALado(painel.ref, ref) && painel.sessionId === sid,
        (ref, sid) => onTrocar(painel.id, ref, sid),
      ),
    [buildMenuItems, onTrocar, painel.id, painel.ref, painel.sessionId],
  )

  return (
    <section
      ref={(el) => {
        if (el) panelRefs.current.set(painel.id, el)
        else panelRefs.current.delete(painel.id)
      }}
      className={`lado-a-lado-painel${visivel ? '' : ' is-oculto'}`}
      data-ativo={ativo || undefined}
      inert={!visivel || undefined}
      aria-hidden={!visivel || undefined}
      role="region"
      aria-label={rotuloDoPainel(botName, painel.sessionId, sessaoNome, t)}
      onFocusCapture={() => {
        if (visivel && !ativo) onFocar(painel.id)
      }}
      onMouseDownCapture={() => {
        if (visivel && !ativo) onFocar(painel.id)
      }}
    >
      <div className="lado-a-lado-painel-topo" tabIndex={-1}>
        <div className="lado-a-lado-painel-identidade">
          {store && bot ? <AvatarConversa bot={bot} store={store} size={20} /> : null}
          <Text as="span" size="callout" weight="semibold" truncate>
            <bdi dir="auto">{botName}</bdi>
          </Text>
          {sessaoNome ? (
            <Text as="span" size="caption" tone="secondary" truncate className="lado-a-lado-painel-sessao">
              <bdi dir="auto">{sessaoNome}</bdi>
            </Text>
          ) : null}
        </div>

        <div className="lado-a-lado-painel-acoes">
          <IconButtonBase
            buttonRef={swapBtnRef}
            size={24}
            icon="arrow.left.arrow.right"
            title={t('conversa.ladoALado.trocar')}
            menuOpen={swapOpen}
            onClick={() => setSwapOpen((o) => !o)}
          />
          <AnchoredMenu
            open={swapOpen}
            onClose={() => setSwapOpen(false)}
            anchor={swapBtnRef}
            items={swapItems}
            language={superficie}
            focusMenu
            menuLabel={t('conversa.ladoALado.trocar')}
          />

          {totalPaineis > 1 ? (
            <>
              <IconButtonBase
                size={24}
                icon="chevron.left"
                title={t('conversa.ladoALado.moverEsquerda')}
                disabled={rtl ? indice >= totalPaineis - 1 : indice <= 0}
                onClick={() => onMover(painel.id, deltaEsquerda)}
              />
              <IconButtonBase
                size={24}
                icon="chevron.right"
                title={t('conversa.ladoALado.moverDireita')}
                disabled={rtl ? indice <= 0 : indice >= totalPaineis - 1}
                onClick={() => onMover(painel.id, deltaDireita)}
              />
            </>
          ) : null}

          <IconButtonBase
            size={24}
            icon="arrow.up.left.and.arrow.down.right"
            title={t('conversa.ladoALado.soEsta')}
            onClick={() => onSair(painel.ref, painel.sessionId)}
          />
          <IconButtonBase
            size={24}
            icon="xmark"
            title={t('conversa.ladoALado.fecharPainel', { nome: botName })}
            onClick={() => onRemover(painel.id)}
          />
        </div>
      </div>

      <div className="lado-a-lado-painel-corpo">
        {store && bot ? (
          <PersonalReadingVisible.Provider value={leituraVisivel && visivel}>
            <StoreContext.Provider value={store}>
              <ThreadView
                key={`${painel.id}:${painel.ref.computerId}/${painel.ref.spaceId ?? ''}/${painel.ref.botId}`}
                botId={painel.ref.botId}
                panelScope={{
                  panelId: painel.id,
                  active: ativo,
                  initialSessionId: painel.sessionId,
                  onSessionChange: (sid) => onAtualizarSessao(painel.id, sid),
                }}
              />
            </StoreContext.Provider>
          </PersonalReadingVisible.Provider>
        ) : (
          <div className="lado-a-lado-painel-carregando">
            <Spinner size={16} />
          </div>
        )}
      </div>
    </section>
  )
}

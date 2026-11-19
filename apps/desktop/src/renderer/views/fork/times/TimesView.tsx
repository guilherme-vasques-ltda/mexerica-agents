// Times, primeira fatia (times-03; docs/reference/redesenho/components/TimesView): o quadro dos bots que já existem,
// com a Lista como alternativa. A tela só envia intenções; o Hub confere (prévia) e grava (Salvar). Nenhum bot lê
// esta árvore nesta entrega, e nada aqui dá contas, conexões, conversas ou memória a um bot. D454 (fatia 9c): no
// idioma escolhido; avisos e motivos guardados como `Frase` (D507), traduzidos ao desenhar.
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { memberOf, moveBot, setReportsTo, type TimesBot, type TimesGraph } from '@shared/fork-times'
import { Button, SegmentedChoice } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { AnchoredMenu, Sheet, useEscape, type MenuItem } from '../../../components/Overlay'
import { useJanelaBaixa } from '../../../lib/janela-baixa'
import { useModel } from '../../../lib/observable'
import { pref, usePref } from '../../../lib/prefs'
import type { AppModel } from '../../../store/app-model'
import type { BotStore } from '../../../store/bot-store'
import { useApp } from '../../../store/context'
import { boardLayout } from './board-layout'
import { boardPrefsKey, readBoardPrefs, writeBoardPrefs, type BoardPrefs } from './board-prefs'
import { restoreFocusAfterLayerExit, restoreFocusIfLost } from './focus'
import { timesScreen } from './navigation'
import { TF, TimesChatInfo, useTimesTextos } from './parts'
import { cru, ErroFrase, frase, type Frase } from '../../../lib/fork-i18n/frase'
import { AREA, ancestorsOf, structureTree, treeIndex } from './structure'
import { TimesAreaSheet } from './TimesAreaSheet'
import { TimesBoard, type BoardAction } from './TimesBoard'
import { TimesInspector, type InspectorAction } from './TimesInspector'
import { TimesList } from './TimesList'
import { TimesModeloSheet } from './TimesModeloSheet'
import { planoMarketing, type Linha } from './modelo-marketing'
import { MovePicker, ReportsPicker, RoutePicker } from './TimesPickers'
import { TimesSession } from './times-session'
import './times.css'

export { timesScreen }

type Mode = 'board' | 'list'
/** A chave do menu de Novo time (Em branco ou Modelo), ao lado das chaves de bot e de área. */
const NOVO = 'novo:'
const modePref = pref<Mode>('donnaTimesMode', 'board', (v): v is Mode => v === 'board' || v === 'list')

/**
 * O Hub da tela, escolhido uma vez ao abrir e pelo computador, nunca pelo UUID do space (o Pessoal tem o mesmo em
 * todos os Hubs) nem pela conexão: o do space aberto na faixa; no Pessoal, o do bot selecionado, mesmo desconectado;
 * sem seleção, o da faixa. Se ele cair, a tela fica só leitura; nunca troca para outro Hub.
 */
function pickStore(app: AppModel, open: { computerId: string | null; fallbackComputerId: string | null }): BotStore | null {
  if (open.computerId) return app.store(open.computerId) ?? null
  return app.selectedStore ?? (open.fallbackComputerId ? app.store(open.fallbackComputerId) ?? null : null)
}

/**
 * Uma sessão por computador e space, viva enquanto o app está aberto: fechar Times (Voltar, abrir um bot, trocar
 * de space) não perde o rascunho nem uma gravação sem resposta. Reabrir relê o Hub; outro Hub no lugar encerra.
 */
const sessions = new Map<string, TimesSession>()
function sessionFor(store: BotStore, spaceId: string | null) {
  const key = JSON.stringify([store.computer.id, spaceId])
  let session = sessions.get(key)
  if (!session || session.phase.kind === 'moved') {
    session = new TimesSession()
    sessions.set(key, session)
  }
  return session
}

export function TimesScreenView() {
  const app = useApp()
  const spaceId = timesScreen.spaceId
  const [store] = useState(() => pickStore(app, timesScreen))
  const { T } = useTimesTextos()
  if (!store) return <Page title={T.title}><div className="times-state"><p>{T.noHub}</p></div></Page>
  return <TimesPage store={store} spaceId={spaceId} />
}

function Page({ title, header, children }: { title: string; header?: ReactNode; children: ReactNode }) {
  const back = useRef<HTMLButtonElement>(null)
  const { T, superficie } = useTimesTextos()
  useEffect(() => back.current?.focus({ preventScroll: true }), [])
  return (
    <div className="times-page idioma-superficie" {...superficie}>
      <header className="times-header drag">
        <button ref={back} className="times-back no-drag" aria-label={T.back} title={T.back} onClick={() => timesScreen.close()}>
          <Icon name="arrow.left" size={14} weight="medium" className="espelhar-rtl" />
        </button>
        <h1 className="times-title">{title}</h1>
        <p className="times-lead">{T.lead}</p>
        <div className="times-header-end no-drag">{header}</div>
      </header>
      {children}
    </div>
  )
}

type Picker =
  | { kind: 'reports' | 'move' | 'route'; botId: string }
  | { kind: 'area'; nodeId: string | null; parentId: string | null }
  | { kind: 'modelo' }

function TimesPage({ store, spaceId }: { store: BotStore; spaceId: string | null }) {
  const { T, tx, superficie } = useTimesTextos()
  const session = useMemo(() => sessionFor(store, spaceId), [store, spaceId])
  useModel(session)
  useEffect(() => {
    session.open(store, spaceId)
    return () => session.close()
  }, [session, store, spaceId])
  // O item cujo cartão recebe o foco de volta quando o Hub responde (só se o foco se perdeu).
  const pendingFocus = useRef<string | null>(null)
  const [mode, setMode] = usePref(modePref)
  const [selected, setSelected] = useState<string | null>(null)
  const [picker, setPicker] = useState<Picker | null>(null)
  const [menu, setMenu] = useState<{ key: string; anchor: HTMLElement } | null>(null)
  /** Quem abriu a folha (cartão, linha ou botão): Esc, Cancelar e a resposta devolvem o foco a ele, se se perdeu. */
  const invoker = useRef<HTMLElement | null>(null)
  const menuAnchor = useRef<HTMLElement | null>(null)
  const novoRef = useRef<HTMLSpanElement>(null)
  const cancelFocusReturn = useRef<() => void>(() => {})
  useEffect(() => () => cancelFocusReturn.current(), [])
  const [ghost, setGhost] = useState<{ from: string; to: string } | null>(null)
  /** Por que um pedido de mudança não pôde abrir agora (só leitura, conferência em curso…). */
  const [hint, setHint] = useState<Frase | null>(null)
  useEffect(() => { if (session.editable) setHint(null) }, [session.editable])

  const snapshot = session.snapshot
  const graph = session.graph
  const hostId = store.hello?.hostId ?? 'hub'
  const space = session.space ?? spaceId ?? 'pessoal'
  const prefsKey = boardPrefsKey(hostId, space)
  const [prefs, setPrefsState] = useState<BoardPrefs>(() => readBoardPrefs(safeStorage(), prefsKey))
  useEffect(() => setPrefsState(readBoardPrefs(safeStorage(), prefsKey)), [prefsKey])
  const setPrefs = (next: BoardPrefs) => {
    setPrefsState(next)
    writeBoardPrefs(safeStorage(), prefsKey, next)
  }
  const collapsed = useMemo(() => new Set(prefs.collapsed), [prefs.collapsed])
  const bots = useMemo(() => new Map<string, TimesBot>((snapshot?.bots ?? []).map((b) => [b.id, b])), [snapshot])
  const primaryBotId = snapshot?.primaryBotId ?? null
  const root = useMemo(() => (graph ? structureTree(graph, snapshot?.bots ?? [], primaryBotId) : null), [graph, snapshot, primaryBotId])
  const index = useMemo(() => (root ? treeIndex(root) : null), [root])
  const layout = useMemo(() => (root && graph ? boardLayout(root, graph, { collapsed, offsets: prefs.offsets, showRoutes: prefs.showRoutes }) : null), [root, graph, collapsed, prefs.offsets, prefs.showRoutes])
  const drafted = useMemo(() => {
    const out = new Set<string>()
    if (!session.draft || !snapshot) return out
    for (const m of session.draft.members) {
      const before = memberOf(snapshot, m.botId)
      const a = m.reportsTo.kind === 'bot' ? m.reportsTo.botId : 'owner'
      const b = before ? (before.reportsTo.kind === 'bot' ? before.reportsTo.botId : 'owner') : null
      if (a !== b) out.add(m.botId)
    }
    return out
  }, [session.draft, snapshot])
  const spaceName = spaceLabel(store, session.space ?? spaceId)
  // D371: regras por equipe só no Pessoal, pelo link atual do Hub.
  const regrasLink = store.connection.kind === 'online' && (!store.spaces || store.spaces.personalSpaceId === (session.space ?? spaceId)) ? store.connectedClient : null

  // Um item escolhido que sumiu (bot apagado, área removida) sai da seleção.
  useEffect(() => {
    if (selected && index && !index.byKey.has(selected)) setSelected(null)
  }, [index, selected])

  /** Abre os ancestrais recolhidos de `key` na árvore atual (a do rascunho novo), para que ele fique à vista. */
  const latest = useRef({ index, prefs, setPrefs })
  latest.current = { index, prefs, setPrefs }
  const reveal = (key: string) => {
    const { index: now, prefs: current, setPrefs: save } = latest.current
    if (!now) return
    const up = ancestorsOf(now, key)
    if (up.some((k) => current.collapsed.includes(k))) save({ ...current, collapsed: current.collapsed.filter((k) => !up.includes(k)) })
  }
  const toggle = (key: string) => setPrefs({ ...prefs, collapsed: collapsed.has(key) ? prefs.collapsed.filter((k) => k !== key) : [...prefs.collapsed, key] })

  const propose = (label: Frase, change: (g: TimesGraph) => TimesGraph, focusKey: string | null = selected) => {
    pendingFocus.current = focusKey
    void session.propose(label, change).then((ok) => {
      setGhost(null)
      const key = pendingFocus.current
      pendingFocus.current = null
      if (!key) return
      // Dois quadros: o rascunho novo desenha (e abre o que estava recolhido acima dele), e só então o cartão existe
      // onde ficou. O foco volta só se ele se perdeu; texto, cursor ou outro controle escolhidos durante a espera ficam.
      requestAnimationFrame(() => {
        if (ok) reveal(key)
        requestAnimationFrame(() => restoreFocusIfLost(document.querySelector<HTMLElement>(`.times-layer [data-times-nav="${CSS.escape(key)}"], .times-layer [data-times-row="${CSS.escape(key)}"]`)))
      })
    })
  }
  const returnAfterClose = (home: HTMLElement | null) => {
    cancelFocusReturn.current()
    const key = home?.dataset.timesNav ?? home?.dataset.timesRow
    const layer = document.activeElement?.closest('[role="dialog"], [role="menu"]') ?? null
    cancelFocusReturn.current = restoreFocusAfterLayerExit(() => {
      if (home?.isConnected) return home
      return key ? document.querySelector<HTMLElement>(`.times-layer [data-times-nav="${CSS.escape(key)}"], .times-layer [data-times-row="${CSS.escape(key)}"]`) : null
    }, layer)
  }
  const closePicker = () => {
    returnAfterClose(invoker.current)
    setPicker(null)
  }
  const submit = (label: Frase, change: (g: TimesGraph) => TimesGraph) => {
    const key = picker && picker.kind !== 'area' && picker.kind !== 'modelo' ? picker.botId : selected
    closePicker()
    propose(label, change, key)
  }

  const act = (action: BoardAction | InspectorAction, key: string, from: HTMLElement) => {
    if (action === 'menu') {
      menuAnchor.current = from
      invoker.current = from
      setMenu({ key, anchor: from })
      return
    }
    if (!session.editable) {
      setHint(readonlyReason(session))
      return
    }
    setHint(null)
    invoker.current = homeOf(from)
    if (action === 'reports' || action === 'move' || action === 'route') setPicker({ kind: action, botId: key })
    else if (action === 'editArea') setPicker({ kind: 'area', nodeId: key.slice(AREA.length), parentId: null })
    else if (action === 'newSubarea') setPicker({ kind: 'area', nodeId: null, parentId: key.slice(AREA.length) })
    else if (action === 'unteam') {
      propose(frase('times.quadro.tirarNomeDaEquipe', { nome: bots.get(key)?.name ?? 'Bot' }), (g) => moveBot(g, key, { teamId: null, parentBotId: null }), key)
    }
  }
  const onReport = (botId: string, to: string) => {
    if (!session.editable) return setHint(readonlyReason(session))
    const target = to === 'owner' ? TF.owner : cru(bots.get(to)?.name ?? 'bot')
    setGhost({ from: botId, to })
    setSelected(botId)
    propose(frase('times.quadro.nomeReportaA', { nome: bots.get(botId)?.name ?? 'Bot', alvo: target }), (g) => setReportsTo(g, botId, to === 'owner' ? { kind: 'owner' } : { kind: 'bot', botId: to }), botId)
  }

  /** Fechar o menu (Esc, clique fora ou um item) devolve o foco ao cartão ou linha que o abriu, se ele se perdeu. */
  const closeMenu = () => {
    const home = homeOf(menu?.anchor ?? null)
    returnAfterClose(home)
    setMenu(null)
  }
  // Com o menu aberto (⋯, Shift+F10 ou a tecla de menu), o foco vai para o primeiro item e as setas, Home, End e Enter
  // são dele: o quadro e a Lista por trás não recebem essas teclas. Esc continua com o menu (fecha e devolve o foco).
  useEffect(() => {
    if (!menu) return
    const items = () => [...document.querySelectorAll<HTMLElement>('.anchored-panel [role="menuitem"], .anchored-panel [role="menuitemradio"]')]
    const frame = requestAnimationFrame(() => items()[0]?.focus({ preventScroll: true }))
    const onKey = (e: KeyboardEvent) => {
      const list = items()
      if (!list.length || e.altKey || e.metaKey || e.ctrlKey) return
      const at = list.indexOf(document.activeElement as HTMLElement)
      let next: number
      if (e.key === 'ArrowDown') next = (at + 1) % list.length
      else if (e.key === 'ArrowUp') next = at <= 0 ? list.length - 1 : at - 1
      else if (e.key === 'Home') next = 0
      else if (e.key === 'End') next = list.length - 1
      else if (e.key === 'Enter' || e.key === ' ') {
        // O próprio botão focado executa (clique nativo); nada chega ao fundo.
        e.stopPropagation()
        if (at < 0) { e.preventDefault(); list[0]?.focus({ preventScroll: true }) }
        return
      } else return
      e.preventDefault()
      e.stopPropagation()
      list[next]?.focus({ preventScroll: true })
    }
    window.addEventListener('keydown', onKey, true)
    return () => {
      cancelAnimationFrame(frame)
      window.removeEventListener('keydown', onKey, true)
    }
  }, [menu])

  const menuItems = (): MenuItem[] => {
    if (!menu || !graph) return []
    const editable = session.editable
    if (menu.key === NOVO) {
      const motivo = modeloReason()
      return [
        { title: tx(frase('times.quadro.emBranco')), icon: 'plus', action: () => openSheet({ kind: 'area', nodeId: null, parentId: null }) },
        { title: tx(motivo ?? frase('times.quadro.modeloMarketing')), icon: 'person.2', detail: motivo ? undefined : tx(frase('times.quadro.modeloDetalhe')), action: () => { if (!motivo) openSheet({ kind: 'modelo' }) } },
      ]
    }
    if (menu.key.startsWith(AREA)) {
      if (!editable) return [{ title: tx(readonlyReason(session)), action: () => {} }]
      return [
        { title: tx(frase('times.quadro.editarArea')), icon: 'pencil', action: () => act('editArea', menu.key, menu.anchor) },
        { title: tx(frase('times.quadro.novaSubarea')), icon: 'plus', action: () => act('newSubarea', menu.key, menu.anchor) },
      ]
    }
    const m = memberOf(graph, menu.key)
    if (!m) return []
    if (!editable) return [{ title: tx(readonlyReason(session)), action: () => {} }]
    const primary = menu.key === primaryBotId
    return [
      ...(primary ? [] : [
        { title: tx(frase('times.quadro.reportaAMenu')), icon: 'arrow.uturn.backward', detail: tx(frase('times.quadro.teclaR')), action: () => act('reports', menu.key, menu.anchor) },
        { title: tx(frase('times.quadro.moverMenu')), icon: 'arrow.triangle.branch', detail: tx(frase('times.quadro.teclaM')), action: () => act('move', menu.key, menu.anchor) },
      ]),
      { title: tx(frase('times.quadro.rotaPropostaMenu')), icon: 'point.3.connected.trianglepath.dotted', action: () => act('route', menu.key, menu.anchor) },
      ...(!primary && (m.teamId || m.parentBotId) ? [{ title: tx(frase('times.quadro.tirarDaEquipe')), icon: 'xmark', divider: true, action: () => act('unteam', menu.key, menu.anchor) }] : []),
    ]
  }

  /** D353: o modelo só no Pessoal nesta fatia (o createBot do app cria no Pessoal) e com o quadro editável. */
  const modeloReason = (): Frase | null => {
    if (!session.editable) return readonlyReason(session)
    // Sem lista de spaces (Hub sem spaces), a tela já trata o space como Pessoal (spaceLabel).
    const personal = store.spaces?.personalSpaceId
    if (personal && (session.space ?? spaceId) !== personal) return frase('times.quadro.modeloSoPessoal')
    return null
  }
  const openSheet = (next: Picker) => {
    if (!session.editable) return setHint(readonlyReason(session))
    setPicker(next)
  }
  const openNovo = (from: HTMLElement) => {
    menuAnchor.current = from
    invoker.current = from
    setMenu({ key: NOVO, anchor: from })
  }
  const plano = useMemo(() => (graph ? planoMarketing(graph, snapshot?.bots ?? [], primaryBotId, !!session.draft) : null), [graph, snapshot, primaryBotId, session.draft])
  const executar = (linhas: Linha[]) => session.marketing.executar(linhas, {
    // Como no Novo bot: o motor padrão sai da lista atual de motores do Hub.
    motores: async () => {
      if (!(await store.refreshBackends())) throw new ErroFrase(frase('times.quadro.motoresFalhou'))
      return store.hello?.backends ?? []
    },
    novoId: () => crypto.randomUUID(),
    criar: (draft) => store.save(draft),
    sincronizar: () => session.sync(),
    propor: async (label, change) => {
      const ok = await session.propose(label, change)
      if (!ok && session.check.kind === 'refused') throw new ErroFrase(session.check.message)
      return ok
    },
  })

  const header = (
    <>
      {/* D249 E6: the team chat isn't there yet; it says so in an (i), outside the main view. */}
      <TimesChatInfo />
      <SegmentedChoice selection={mode} options={[{ id: 'board', label: T.board }, { id: 'list', label: T.list }]} onChange={setMode} />
      <span ref={novoRef} className="times-novo"><Button disabled={!session.editable} title={session.editable ? undefined : tx(readonlyReason(session))} onClick={() => novoRef.current && openNovo(novoRef.current)}>{T.newTeam} ▾</Button></span>
    </>
  )
  const status = hint ? { text: hint, alert: false, retry: false } : statusLine(session)
  const phase = session.phase
  // E4: below 600 px of height the details panel is a drawer over the board, opened by its (i).
  const baixa = useJanelaBaixa()
  const [gaveta, setGaveta] = useState(false)
  const gavetaBotao = useRef<HTMLButtonElement>(null)
  useEscape(baixa && gaveta, () => { setGaveta(false); gavetaBotao.current?.focus({ preventScroll: true }) }, gavetaBotao)

  return (
    <Page title={tx(frase('times.quadro.tituloSpace', { space: spaceName }))} header={header}>
      {status ? <p className={`times-banner${status.alert ? ' is-alert' : status.warn ? ' is-warn' : ''}`} role={status.alert ? 'alert' : 'status'}>{status.alert || status.warn ? <Icon name="exclamationmark.triangle" size={13} /> : null}{tx(status.text)}{status.retry ? <button type="button" className="times-link" onClick={() => session.reread()}>{tx(frase('times.quadro.lerDeNovo'))}</button> : null}</p> : null}
      {!graph || !root || !index || !layout ? (
        <div className="times-state">
          <p role={phase.kind === 'error' ? 'alert' : undefined}>{phase.kind === 'loading' ? T.loading : phase.kind === 'moved' ? T.moved : phase.kind === 'error' ? tx(phase.message) : T.loading}</p>
          {phase.kind === 'error' && retriable(phase.failure) ? <Button onClick={() => session.reread()}>{T.retry}</Button> : null}
        </div>
      ) : (
        <div className="times-body">
          <section className="times-main">
            {graph.nodes.length === 0 ? (
              <div className="times-empty">
                <p>{tx(frase('times.quadro.primeiraEquipe'))}</p>
                <div className="times-empty-actions">
                  <Button kind="secondary" disabled={!session.editable} onClick={() => setPicker({ kind: 'area', nodeId: null, parentId: null })}>{T.newTeam}</Button>
                  {modeloReason() ? null : <Button onClick={() => openSheet({ kind: 'modelo' })}>{tx(frase('times.quadro.usarModelo'))}</Button>}
                </div>
              </div>
            ) : null}
            {mode === 'board' ? (
              <TimesBoard layout={layout} index={index} graph={graph} bots={bots} primaryBotId={primaryBotId} prefs={prefs} setPrefs={setPrefs}
                selected={selected} onSelect={setSelected} editable={session.editable} drafted={drafted} ghost={ghost}
                onReport={onReport} onAction={act} onToggle={toggle} />
            ) : (
              <TimesList root={root} graph={graph} bots={bots} primaryBotId={primaryBotId} collapsed={collapsed} selected={selected}
                onSelect={setSelected} onToggle={toggle} onAction={act} />
            )}
            {baixa ? (
              <button ref={gavetaBotao} type="button" className="mac-chat-button ringed times-drawer-toggle" title={tx(frase('times.quadro.detalhes'))} aria-label={tx(frase('times.quadro.detalhes'))}
                aria-expanded={gaveta} aria-controls="times-side" onClick={() => setGaveta((g) => !g)}>
                <Icon name="info.circle" size={16} weight="medium" />
              </button>
            ) : null}
          </section>
          <TimesInspector session={session} graph={graph} bots={bots} primaryBotId={primaryBotId} selected={selected} editable={session.editable}
            drawer={baixa ? { open: gaveta } : null} onAction={act} onPropose={(label, change) => propose(label, change)} />
        </div>
      )}
      {graph ? <><AnchoredMenu open={menu !== null} onClose={closeMenu} anchor={menuAnchor} items={menuItems} language={superficie} />
      <Sheet open={picker !== null} onClose={closePicker} width={520} maxHeight="min(640px, calc(100vh - 80px))" label={tx(pickerLabel(picker))}>
        {/* O Sheet abre num portal no body (legado pt-BR/LTR): a folha declara o idioma dela. */}
        <div className="idioma-superficie" {...superficie} style={{ display: 'contents' }}>
        {picker && graph ? (
          picker.kind === 'modelo' ? (
            plano ? <TimesModeloSheet plano={plano} graph={graph} bots={bots} execucao={session.marketing} executar={executar} onDone={closePicker} onCancel={closePicker} /> : null
          ) : picker.kind === 'area' ? (
            <TimesAreaSheet graph={graph} bots={bots} primaryBotId={primaryBotId} nodeId={picker.nodeId} parentId={picker.parentId} newId={() => crypto.randomUUID()} onSubmit={submit} onCancel={closePicker}
              regras={regrasLink ? { link: regrasLink, saved: !!snapshot?.nodes.some((n) => n.id === picker.nodeId) } : null} />
          ) : picker.kind === 'reports' ? (
            <ReportsPicker graph={graph} bots={bots} primaryBotId={primaryBotId} botId={picker.botId} onSubmit={submit} onCancel={closePicker} />
          ) : picker.kind === 'move' ? (
            <MovePicker graph={graph} bots={bots} primaryBotId={primaryBotId} botId={picker.botId} onSubmit={submit} onCancel={closePicker} />
          ) : (
            <RoutePicker graph={graph} bots={bots} primaryBotId={primaryBotId} botId={picker.botId} onSubmit={submit} onCancel={closePicker} />
          )
        ) : null}
        </div>
      </Sheet></> : null}
    </Page>
  )
}

function pickerLabel(picker: Picker | null): Frase {
  if (!picker) return TF.title
  if (picker.kind === 'modelo') return frase('times.quadro.equipeMarketing')
  if (picker.kind === 'area') return picker.nodeId ? frase('times.quadro.editarAreaTitulo') : picker.parentId ? frase('times.quadro.novaSubareaTitulo') : TF.newTeam
  return picker.kind === 'reports' ? TF.reportsTo : picker.kind === 'move' ? frase('times.quadro.mover') : frase('times.quadro.rotaProposta')
}

function spaceLabel(store: BotStore, spaceId: string | null): Frase {
  const spaces = store.spaces
  if (!spaces || !spaceId || spaceId === spaces.personalSpaceId) return frase('times.quadro.pessoal')
  const name = spaces.spaces.find((s) => s.id === spaceId)?.name
  return name !== undefined ? cru(name) : frase('times.quadro.space')
}

/** The read can be tried again: not when the Hub said this isn't the owner or that it has no Times. */
const retriable = (failure: string) => failure !== 'owner' && failure !== 'old_hub'

/** `alert`: urgent (role=alert); `warn`: a failure said politely (role=status) but marked like one: icon and `ink`. */
function statusLine(session: TimesSession): { text: Frase; alert: boolean; retry: boolean; warn?: boolean } | null {
  if (session.phase.kind === 'moved') return { text: TF.moved, alert: true, retry: false }
  if (session.link === 'offline') return { text: session.snapshot ? frase('times.quadro.soLeituraUltima') : TF.readonly, alert: false, retry: false }
  // D249 R4: a read error without a board is said once, in the middle, with Tentar de novo (not here too).
  if (session.notice && !session.draft && 'chave' in session.notice && session.notice.chave === 'times.quadro.sessao.releituraFalhou') return { text: session.notice, alert: false, retry: true, warn: true }
  if (session.events === 'paused') return { text: frase('times.quadro.reconectando'), alert: false, retry: true }
  return null
}

/** Por que a tela está só lendo agora, em palavras. */
function readonlyReason(session: TimesSession): Frase {
  if (session.link === 'offline') return TF.readonly
  if (session.phase.kind !== 'ready' || !session.snapshot) return frase('times.quadro.aindaAbrindo')
  if (session.check.kind === 'checking') return frase('times.quadro.espereConferencia')
  if (session.save.kind === 'saving') return frase('times.quadro.espereSalvar')
  if (session.save.kind === 'unknown') return frase('times.quadro.resolvaGravacao')
  if (session.stale) return frase('times.quadro.mudouOutroAparelho')
  return frase('times.quadro.soLeituraAgora')
}

/** O lugar estável de um invocador: o cartão ou a linha que contém um botão ⋯ (que pode sumir), senão ele mesmo. */
function homeOf(el: HTMLElement | null): HTMLElement | null {
  if (!el) return null
  const card = el.closest<HTMLElement>('[data-times-card]')?.querySelector<HTMLElement>('[data-times-nav]')
  return el.closest<HTMLElement>('[data-times-nav], [data-times-row]') ?? card ?? el
}

function safeStorage() {
  try {
    return window.localStorage
  } catch {
    return null
  }
}

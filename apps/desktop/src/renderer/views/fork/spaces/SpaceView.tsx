import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import type { Space } from '@shared/fork-spaces'
import { isHostMessage, screenAgent, type Entry, type ScreenState } from '@shared/models'
import { ltr } from '@shared/bidi'
import { IconButton } from '../../../components/Controls'
import { Icon } from '../../../components/Icon'
import { AnchoredMenu, type MenuItem } from '../../../components/Overlay'
import { Text } from '../../../components/Text'
import { botName, continuityText, conversationGroups, conversationLabel } from '../../../lib/fork-space-context'
import { useModel } from '../../../lib/observable'
import { superficieIdioma, useIdioma } from '../../../lib/fork-i18n/estado'
import { frase, textoDe, type Frase } from '../../../lib/fork-i18n/frase'
import { localeDe } from '../../../lib/fork-i18n/index'
import type { AppLanguage } from '@shared/fork-idioma'
import type { SpaceContext } from '../../../store/fork-space-context'
import { MarkdownText } from '../../thread/Markdown'
import { WebTurnWarning } from '../../thread/fork-web-turn-warning'
import { SpaceTile } from './SpaceTile'
import { SpaceComposer } from './SpaceComposer'
import { avisoOpen } from './erro-envio'
import './space-view.css'

// D543 f1: leitura e envio explícito no contexto capturado; o Pessoal escondido não é destino de fallback.

const clock = (t: number, idioma: AppLanguage) => ltr(new Date(t).toLocaleTimeString(localeDe(idioma), { hour: '2-digit', minute: '2-digit' }))

export function SpaceView({ context, space, onEdit, onMove, canMove, onLibrary, onCreateBot, onActivatePrincipal, onDeleteSpace, lifecycle }: {
  context: SpaceContext
  space: Space
  onEdit: () => void
  onMove: (delta: -1 | 1) => void
  canMove: { up: boolean; down: boolean }
  /** BN-03: a biblioteca de especialistas deste space. */
  onLibrary?: () => void
  onCreateBot: () => void
  /** D543 E1 (tela 1): Space legado sem principal oferece ativar/excluir na própria vista. */
  onActivatePrincipal?: () => void
  onDeleteSpace?: () => void
  lifecycle?: boolean
}) {
  useModel(context)
  const idioma = useIdioma()
  const tx = (f: Frase) => textoDe(f, idioma)
  const [menu, setMenu] = useState(false)
  const [showClosed, setShowClosed] = useState(false)
  const [running, setRunning] = useState(false)
  const menuButton = useRef<HTMLButtonElement>(null)
  const list = useRef<HTMLElement>(null)
  const { conversations, principal } = context.list
  const names = (id: string) => botName(id, principal, context.bots, context.foreignLabels)
  const label = (c: typeof conversations[number]) => tx(conversationLabel(c, names(c.botId)))
  const groups = conversationGroups(conversations)
  const selected = conversations.find((c) => c.id === context.conversationId) ?? null
  const items = (): MenuItem[] => [
    { title: tx(frase('spaces.view.editar')), icon: 'pencil', action: onEdit },
    ...(onLibrary ? [{ title: tx(frase('spaces.view.biblioteca')), icon: 'book.closed', action: onLibrary }] : []),
    ...(canMove.up ? [{ title: tx(frase('spaces.view.moverCima')), icon: 'arrow.up', action: () => onMove(-1) }] : []),
    ...(canMove.down ? [{ title: tx(frase('spaces.view.moverBaixo')), icon: 'arrow.down', action: () => onMove(1) }] : []),
  ]

  // Opening a space puts the keyboard in its conversations (Pessoal's field it was in is hidden now), then, once the Hub
  // answers, on the open one (or Tentar de novo), only if the keyboard is still there: an update never moves focus.
  const settled = context.list.state !== 'loading'
  useEffect(() => { list.current?.focus() }, [space.id])
  useEffect(() => {
    const nav = list.current
    if (settled && nav && document.activeElement === nav) (nav.querySelector<HTMLElement>('[aria-current="true"]') ?? nav.querySelector<HTMLElement>('button'))?.focus()
  }, [settled, space.id])

  // A retry swaps the button that had the keyboard for what it reads: the keyboard stays in the conversations.
  const keepFocus = () => (list.current?.querySelector<HTMLElement>('[aria-current="true"]') ?? list.current)?.focus()
  const retry = () => {
    context.retry()
    keepFocus()
  }

  const onListKey = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return
    const rows = [...(list.current?.querySelectorAll<HTMLButtonElement>('button.space-view-row') ?? [])]
    const at = rows.indexOf(document.activeElement as HTMLButtonElement)
    const next = rows[at + (e.key === 'ArrowDown' ? 1 : -1)]
    if (next) {
      e.preventDefault()
      next.focus()
    }
  }

  const row = (c: typeof conversations[number]) => (
    <button key={c.id} className="space-view-row" aria-current={c.id === context.conversationId ? 'true' : undefined} onClick={() => context.select(c.id)}>
      <bdi className="space-view-row-title" dir="auto">{label(c)}</bdi>
      {!c.primary ? <bdi className="space-view-row-detail" dir="auto">{tx(names(c.botId))}</bdi> : null}
    </button>
  )

  return (
    <div className="space-view idioma-superficie" {...superficieIdioma(idioma)}>
      <aside className="space-view-side glass-panel" aria-label={tx(frase('spaces.view.rotulo', { nome: space.name }))}>
        <div className="space-view-head drag">
          {space.tile ? <SpaceTile tile={space.tile} size={26} /> : null}
          <Text as="h2" size="callout" weight="semibold" truncate className="space-view-name" dir="auto">{space.name}</Text>
          <IconButton title={tx(frase('spaces.view.novoBot'))} icon="plus" size={28} className="no-drag" disabled={context.link !== 'live'} onClick={onCreateBot} />
          <IconButton title={tx(frase('spaces.view.maisAcoes'))} icon="ellipsis" size={28} className="no-drag" buttonRef={menuButton} menuOpen={menu} onClick={() => setMenu((o) => !o)} />
          <AnchoredMenu open={menu} onClose={() => setMenu(false)} anchor={menuButton} items={items} language={superficieIdioma(idioma)} />
        </div>
        <ComputerUse screen={context.screen} name={names} tx={tx} />
        <nav ref={list} className="space-view-list" aria-label={tx(frase('spaces.view.conversas'))} tabIndex={-1} onKeyDown={onListKey}>
          <Text as="h3" size="caption" tone="secondary" className="space-view-section">{tx(frase('spaces.view.conversas'))}</Text>
          {context.list.state === 'loading' ? <div className="space-view-reserve" aria-hidden><span /><span /><span /></div>
            : context.list.state === 'error' ? <ListProblem text={context.list.error ? tx(context.list.error.text) : ''} action={tx(frase('spaces.view.tentar'))} onRetry={retry} />
              : !conversations.length ? <p className="space-view-empty">{tx(frase('spaces.view.nenhumaConversa'))}</p>
                : <>
                  {groups.open.map(row)}
                  {groups.closed.length ? (
                    <>
                      <button className="space-view-fold" aria-expanded={showClosed} onClick={() => setShowClosed((o) => !o)}>{tx(frase('spaces.view.fechadas', { n: ltr(groups.closed.length) }))}</button>
                      {showClosed ? groups.closed.map(row) : null}
                    </>
                  ) : null}
                </>}
        </nav>
      </aside>

      <main className="space-view-main" aria-label={selected ? label(selected) : tx(frase('spaces.view.rotulo', { nome: space.name }))}>
        <div className="space-view-top drag">
          {selected ? (
            <div className="space-view-heading">
              <Text as="h3" size="body" weight="semibold" truncate className="space-view-title" dir="auto">{label(selected)}</Text>
              <button className="space-view-running no-drag" aria-expanded={running} onClick={() => setRunning((o) => !o)}>{tx(frase('spaces.view.comoRodando'))}</button>
            </div>
          ) : null}
        </div>
        {selected && running ? (
          <div className="space-view-runinfo">
            <span>{tx(continuityText(selected) ?? frase('spaces.view.semContinuidade'))}</span>
            <span>{tx(frase(selected.engineAccountId ? 'spaces.view.comConta' : 'spaces.view.semConta'))}</span>
          </div>
        ) : null}
        {context.link !== 'live' ? (
          <div className="space-view-banner" role="status">
            {tx(context.link === 'offline'
              ? (context.asOf ? frase('spaces.view.semConexaoDesde', { hora: clock(context.asOf, idioma) }) : frase('spaces.view.semConexao'))
              : (context.asOf ? frase('spaces.view.pausadoDesde', { hora: clock(context.asOf, idioma) }) : frase('spaces.view.pausado')))}
            {context.link === 'paused' ? <button className="space-view-link no-drag" onClick={retry}>{tx(frase('spaces.view.tentar'))}</button> : null}
          </div>
        ) : null}
        {!space.principalBotId && space.kind === 'space' ? (
          <div className="space-view-berga" role="status">
            <Text as="div" size="callout">{tx(frase('spaces.view.bergaInativo'))}</Text>
            {lifecycle ? (
              <div className="space-view-berga-acoes">
                <button className="space-view-link" onClick={onActivatePrincipal}>{tx(frase('spaces.sheet.ativar'))}</button>
                <button className="space-view-link" onClick={onDeleteSpace}>{tx(frase('spaces.sheet.excluir'))}</button>
              </div>
            ) : <span className="space-view-note">{tx(frase('spaces.view.atualizeHub'))}</span>}
          </div>
        ) : null}
        <div className="space-view-history">
          {!selected && context.missing && context.list.state === 'ready' ? <ListProblem text={tx(frase('spaces.view.naoAparece'))} action={tx(frase('spaces.view.atualizar'))} onRetry={retry} />
            : !selected ? (context.list.state === 'ready' && conversations.length ? <p className="space-view-empty">{tx(frase('spaces.view.escolha'))}</p> : null)
            : context.history.state === 'loading' ? <div className="space-view-reserve" aria-hidden><span /><span /></div>
              : context.history.state === 'error' ? <ListProblem text={context.history.error ? tx(context.history.error.text) : ''} action={tx(frase('spaces.view.tentar'))} onRetry={() => { context.select(selected.id); keepFocus() }} />
                : (
                  <>
                    {context.history.hasMore ? (
                      <button className="space-view-older" disabled={context.history.older} onClick={() => context.loadOlder()}>
                        {tx(frase(context.history.older ? 'spaces.view.carregando' : 'spaces.view.anteriores'))}
                      </button>
                    ) : null}
                    {context.history.error ? <p className="space-view-note" role="status">{tx(context.history.error.text)}</p> : null}
                    {!context.history.entries.length ? <p className="space-view-empty">{tx(frase('spaces.view.nenhumaMensagem'))}</p> : null}
                    {context.history.entries.map((e) => <HistoryRow key={e.id} entry={e} bot={names(selected.botId)} idioma={idioma} onRetry={() => context.retryWebTurn(e)} />)}
                  </>
                )}
        </div>
        {selected && !selected.closed ? <SpaceComposer key={context.conversationKey} context={context} /> : selected ? (
          <div className="space-view-bar">
            {tx(frase(selected.closed ? 'spaces.view.fechada' : 'spaces.view.semEnvio'))}
          </div>
        ) : null}
      </main>
    </div>
  )
}

/**
 * R1: who uses the computer, as this space's stream says. Only `using` names a bot (its principal or one of its own);
 * unavailable says so without a name; idle or no state yet shows nothing, never Ready. Read only, no conversation.
 */
function ComputerUse({ screen, name, tx }: { screen: ScreenState | null; name: (id: string) => Frase; tx: (f: Frase) => string }) {
  const agent = screenAgent(screen)
  if (agent) return <p className="space-view-use" role="status"><Icon name="cursorarrow.motionlines" size={11} color="var(--accent)" />{tx(frase('spaces.view.usandoComputador', { bot: name(agent) }))}</p>
  if (screen?.agentUse === 'unavailable') return <p className="space-view-use" role="status">{tx(frase('spaces.view.computadorIndisponivel'))}</p>
  return null
}

function ListProblem({ text, onRetry, action }: { text: string; onRetry: () => void; action: string }) {
  return (
    <div className="space-view-problem" role="status">
      <span>{text}</span>
      <button className="space-view-link" onClick={onRetry}>{action}</button>
    </div>
  )
}

export function HistoryRow({ entry, bot, idioma, onRetry }: { entry: Entry; bot: Frase; idioma: AppLanguage; onRetry?: () => Promise<void> }) {
  if (isHostMessage(entry)) return null
  const text = typeof entry.data.text === 'string' ? entry.data.text : ''
  const files = [...(entry.data.attachments ?? []), ...(entry.data.files ?? [])].map((f) => f.name).filter(Boolean)
  const when = new Date(entry.createdAt).toLocaleString(localeDe(idioma))
  if (entry.kind === 'permission') return <p className="space-view-note">{textoDe(frase('spaces.view.pediuAprovacao', { bot }), idioma)}</p>
  if (entry.kind === 'notice') {
    const aviso = avisoOpen(entry.data.code)
    const line = aviso ? textoDe(frase(aviso), idioma) : text || entry.data.heading
    return line ? <p className="space-view-note" dir="auto">{line}</p> : null
  }
  return (
    <div className={`space-view-msg${entry.kind === 'user' ? ' is-user' : ''}`} title={when}>
      <div className="space-view-bubble">
        {entry.kind === 'user' ? <span className="space-view-text" dir="auto">{text}</span> : <div dir="auto"><MarkdownText text={text} /></div>}
        {files.length ? <bdi className="space-view-files" dir="auto">{files.join(' · ')}</bdi> : null}
        {onRetry && entry.kind === 'user' && entry.data.status === 'sent' && entry.data.webTurnFailed === true
          ? <WebTurnWarning onRetry={onRetry} />
          : null}
      </div>
    </div>
  )
}

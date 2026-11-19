// Rota-03b (D354, opção A): o ícone de recibo na linha de nome do turno e o popover "Como esta resposta foi feita".
import { useContext, useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { anchorPlacement, useEscape, usePresence } from '../../../components/Overlay'
import { Spinner } from '../../../components/Controls'
import { useModel } from '../../../lib/observable'
import { useStoreRef } from '../../../store/context'
import { engineSetup } from '../../../store/fork-engine-setup'
import { receiptsFor } from '../../../store/fork-recibo'
import { contasFor } from '../contas-ia/contas-store'
import { OpenBotSettings } from '../bot-settings/bot-settings-entry'
import { changedModel, receiptView, showsIcon, type ReceiptAnchor } from './presentation'
import { superficieIdioma, useIdioma, useT, tradutor } from '../../../lib/fork-i18n/estado'
import './recibo.css'

function useReceipts() {
  const store = useStoreRef()
  return { store, cache: useModel(receiptsFor(store)) }
}

const RouteIcon = () => (
  <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="6" cy="19" r="3" /><path d="M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15" /><circle cx="18" cy="5" r="3" /></svg>
)

/** Depois da hora, só na resposta que abre o turno. Sem recibo, nada aparece (o popover segue em Mais ações). */
export function ReceiptTrigger({ anchor, botId, botName }: { anchor: ReceiptAnchor; botId: string; botName: string }) {
  const { store, cache } = useReceipts()
  // Só lê a lista que o app já tem; quem a carrega é o popover.
  const contas = useModel(contasFor(store, () => store.ready(), engineSetup))
  const [rect, setRect] = useState<DOMRect | null>(null)
  const t = useT()
  const button = useRef<HTMLButtonElement>(null)
  const request = anchor.request
  useEffect(() => { if (request) cache.prefetch(request) }, [cache, request, cache.epoch])
  const load = cache.get(request)
  // Aberto, o ícone fica mesmo durante a releitura (Tentar de novo, na fila): o popover vive nele.
  if (!request || !rect && (cache.blocked || !showsIcon(load))) return null
  const changed = changedModel(t, load, cache.get(anchor.previous))
  const view = receiptView(t, load, contas.accounts ?? (contas.error ? 'failed' : null), botName)
  return (
    <>
      <button ref={button} className="receipt-trigger" aria-haspopup="dialog" aria-expanded={!!rect} aria-label={view.label} title={t('conversa.recibo.titulo')}
        onClick={() => setRect(rect ? null : bubbleRect(button.current!))}>
        <RouteIcon />{changed ? <span className="receipt-changed">{changed}</span> : null}
      </button>
      {rect ? <ReceiptPopover anchor={anchor} rect={rect} botId={botId} botName={botName} returnFocus={button.current} onClose={() => setRect(null)} /> : null}
    </>
  )
}

/**
 * Onde o recibo abre: abaixo do balão inteiro (ou acima, se não couber), a 8 px, sem cobrir a mensagem.
 * Alinha ao ícone quando parte dele; vindo do menu, à borda do balão.
 */
export function bubbleRect(from: HTMLElement): DOMRect {
  const bubble = from.closest('.message')?.querySelector<HTMLElement>('.bubble, .file-message') ?? from
  const b = bubble.getBoundingClientRect()
  const left = bubble.contains(from) ? from.getBoundingClientRect().left : b.left
  // anchorPlacement já afasta 6 px; os 2 px de cada lado completam os 8.
  return new DOMRect(left, b.top - 2, 0, b.height + 4)
}

/** O recibo aberto, junto de `rect`; Esc e clique fora fecham e devolvem o foco. */
export function ReceiptPopover({ anchor, rect, botId, botName, returnFocus, onClose }: { anchor: ReceiptAnchor; rect: DOMRect; botId: string; botName: string; returnFocus?: HTMLElement | null; onClose: () => void }) {
  const { store, cache } = useReceipts()
  const contas = useModel(contasFor(store, () => store.ready(), engineSetup))
  const openSettings = useContext(OpenBotSettings)
  const idioma = useIdioma()
  const t = tradutor(idioma)
  const panel = useRef<HTMLDivElement>(null)
  const title = useId()
  const { mounted, shown } = usePresence(true, 120)
  const request = anchor.request
  useEffect(() => { if (request) cache.open(request) }, [cache, request, cache.epoch])
  const load = cache.get(request)
  const view = receiptView(t, load, contas.accounts ?? (contas.error ? 'failed' : null), botName, !!request)
  const routed = view.kind === 'routed'
  useEffect(() => { if (routed && !contas.accounts && !contas.loading) void contas.load() }, [routed, contas])
  useLayoutEffect(() => { panel.current?.focus({ preventScroll: true }) }, [mounted])
  const close = () => { onClose(); returnFocus?.focus({ preventScroll: true }) }
  useEscape(true, close, panel)
  if (!mounted) return null
  const { style, width } = anchorPlacement(rect)
  return createPortal(
    <div className={`anchored ${shown ? 'shown' : ''}`} onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div ref={panel} className="anchored-panel receipt-pop idioma-superficie" {...superficieIdioma(idioma)} role="dialog" aria-labelledby={title} tabIndex={-1} style={{ ...style, width }}>
        {view.kind === 'loading' ? (
          <div className="receipt-loading"><Spinner size={14} /><span id={title}>{view.label}…</span></div>
        ) : view.kind === 'routed' ? (
          <>
            <div className="receipt-eyebrow">{t('conversa.recibo.respondidoCom')}</div>
            <h2 id={title}><bdi dir="ltr">{view.model}</bdi></h2>
            <dl>
              <dt>{t('conversa.recibo.campo.conta')}</dt><dd className={view.accountGone ? 'gone' : undefined}><bdi>{view.account}</bdi></dd>
              <dt>{t('conversa.recibo.campo.motor')}</dt><dd><bdi>{view.engine}</bdi></dd>
              {view.effort ? <><dt>{t('conversa.recibo.campo.esforco')}</dt><dd>{view.effort}</dd></> : null}
              {anchor.rulesVersion ? <><dt>{t('delegacao.recibo.rotulo')}</dt><dd>{t('delegacao.recibo', { versao: anchor.rulesVersion })}</dd></> : null}
            </dl>
            <hr />
            <span className={`receipt-tag ${view.tag.tone}`}>{view.tag.text}</span>
            <p>{view.why}</p>
            {view.motorLink && openSettings ? <button className="receipt-link" onClick={() => { onClose(); openSettings(store, botId, 'engine') }}>{view.motorLink}</button> : null}
            <p className="receipt-note">{t('conversa.recibo.nota')}</p>
          </>
        ) : (
          <>
            <div className="receipt-eyebrow">{t('conversa.recibo.titulo')}</div>
            <div className={view.warn ? 'receipt-warn' : undefined}>
              {view.warn ? <i aria-hidden="true">!</i> : null}
              <div>
                <h2 id={title}>{view.title}</h2>
                <p>{view.body}</p>
                {anchor.rulesVersion ? <p className="receipt-note">{t('delegacao.recibo', { versao: anchor.rulesVersion })}</p> : null}
                {view.note ? <p className="receipt-note">{view.note}</p> : null}
              </div>
            </div>
            {view.retry && request ? <button className="receipt-retry" onClick={() => cache.open(request)}>{t('conversa.recibo.tentarDeNovo')}</button> : null}
          </>
        )}
      </div>
    </div>,
    document.body,
  )
}

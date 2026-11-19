import { spaceAllows } from '../../../lib/fork-space-capabilities'
import { useCallback, useEffect, useRef, useState, type RefObject } from 'react'
import { isGroup, type Bot } from '@shared/models'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import type { SessionFailure, SessionFailureNotice, SidePanelTab } from '../../../store/fork-sessions'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import { superficieIdioma, useIdioma, useT } from '../../../lib/fork-i18n/estado.ts'
import './ganchos.css'

let consumed = 0

/** Consome o pedido de abrir o painel (store.openSessionsPanel) na conversa dele, mesmo se ela montar depois. */
export function useConsumePanelRequest(store: BotStore, chatId: string, open: (tab: SidePanelTab) => void) {
  const model = useModel(store.sessions)
  const request = model.panelRequest
  const handler = useRef(open)
  handler.current = open
  useEffect(() => {
    if (!request || request.chatId !== chatId || request.seq <= consumed) return
    consumed = request.seq
    handler.current(request.tab)
  }, [request, chatId])
}


/**
 * UI-01: extras só no Pessoal e só em conversa com bot. Canais e Spaces continuam com a principal (sem inferir do
 * catálogo); o Hub valida o resto (bot, conta, modelo, estado) em cada pedido.
 */
export function extrasAllowed(store: Pick<BotStore, 'spaceId' | 'spaces'>, chat: Bot | null): chat is Bot {
  return !!chat && !isGroup(chat) && spaceAllows(store.spaceId === store.spaces?.personalSpaceId ? undefined : store.spaceId, 'sessions')
}

/** "Nova sessão" desta conversa: null se o Hub (neste link) não anuncia `create` e `send`, ou a conversa não tem extras. */
export function useNewSession(store: BotStore, chat: Bot | null, open: (sessionId: string) => void): { run: (onFail?: () => void) => void; busy: boolean } | null {
  const model = useModel(store.sessions)
  const openRef = useRef(open)
  openRef.current = open
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => { alive.current = false }
  }, [])
  // Criar sem poder escrever nela deixaria uma aba morta: exige `create` e `send`.
  if (!extrasAllowed(store, chat) || !model.can(chat.id, 'create') || !model.can(chat.id, 'send')) return null
  const chatId = chat.id
  return {
    busy: model.busy(chatId),
    // Abre a aba devolvida só se esta conversa ainda está na tela; nada é enviado. D527: `onFail` (o "+" do cartão
    // pega o foco de volta) só com a conversa ainda na tela.
    run: (onFail) => { void model.create(chatId).then((r) => { if (!alive.current) return; if (r.ok) openRef.current(r.session.id); else onFail?.() }) },
  }
}

/**
 * D527: a sessão criada abre com o cursor na caixa dela. O "+" fica desabilitado enquanto cria e perde o foco, e a
 * caixa remonta por sessão; por isso o foco vai só depois de a aba aberta ser a criada.
 */
export function useOpenFocused(open: (id: string) => void, openId: string | null, root: RefObject<HTMLElement | null>) {
  const [pending, setPending] = useState<string | null>(null)
  useEffect(() => {
    if (pending === null || pending !== openId) return
    setPending(null)
    root.current?.querySelector<HTMLTextAreaElement>('.composer textarea')?.focus({ preventScroll: true })
  }, [pending, openId, root])
  return useCallback((id: string) => { setPending(id); open(id) }, [open])
}

/** Consome o "Nova sessão" do menu do bot (store.sessions.requestNew) na conversa dele, logo depois de abri-la. */
export function useConsumeNewRequest(store: BotStore, chatId: string, run: (() => void) | null) {
  const model = useModel(store.sessions)
  const request = model.newRequest
  const handler = useRef(run)
  handler.current = run
  useEffect(() => {
    if (!request || !handler.current) return
    // Consumo do próprio modelo (por origem); um pedido antigo (a conversa não abriu a tempo) não cria nada depois.
    if (model.takeNew(chatId, NEW_REQUEST_TTL)) handler.current()
  }, [model, request, chatId, run === null])
}
export const NEW_REQUEST_TTL = 2_000

/** Numa aba de sessão em que não dá para escrever: Hub sem `send`, canal, Space ou sessão encerrada. */
export function SessionComposerNotice({ botName, closed = false, onBack }: { botName: string; closed?: boolean; onBack: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  return (
    <div className="session-composer-notice idioma-superficie" role="status" {...superficieIdioma(idioma)}>
      <span>{t(closed ? 'sessions.composer.closed' : 'sessions.composer.unavailable', { nome: botName })}</span>
      <button onClick={onBack}>{t('sessions.composer.back')} <kbd>{t('sessions.composer.backKey')}</kbd></button>
    </div>
  )
}

/** O texto vem do código, nunca da mensagem do Hub ou do motor. */
const FAILURE_TEXT = {
  unsupported: 'sessions.error.unsupported',
  linkChanged: 'sessions.error.linkChanged',
  closed: 'sessions.error.closed',
  unknownSession: 'sessions.error.unknownSession',
  selectionUnavailable: 'sessions.error.selectionUnavailable',
  offline: 'sessions.error.offline',
  failed: 'sessions.error.failed',
  misrouted: 'sessions.error.misrouted',
} as const satisfies Record<SessionFailure, ChaveTexto>

/** A falha de criar ou enviar numa sessão, por cima do compositor; o texto digitado continua onde estava. */
export function SessionFailureBanner({ notice, onDismiss }: { notice: SessionFailureNotice; onDismiss: () => void }) {
  const t = useT()
  const idioma = useIdioma()
  const title = notice.sessionId === null ? 'sessions.error.create.title' : notice.failure === 'misrouted' ? 'sessions.error.misrouted.title' : 'sessions.error.send.title'
  return (
    <div className="session-composer-notice is-error idioma-superficie" role="alert" {...superficieIdioma(idioma)}>
      <span><strong>{t(title)}</strong> {t(FAILURE_TEXT[notice.failure])}</span>
      <button onClick={onDismiss} aria-label={t('sessions.error.dismissLabel')}>{t('sessions.error.dismiss')}</button>
    </div>
  )
}

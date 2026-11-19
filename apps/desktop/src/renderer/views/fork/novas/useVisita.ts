import { useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from 'react'
import { prefs } from '../../../lib/prefs'
import { advanceContiguous, advanceMark, countBelow, forgetSessions, markKey, markSaver, nextVisit, openAt, openVisit, type Seen, type Visit } from './leitura'

// D432 (docs/reference/design/barrinha-novas-20261107): o ciclo da visita a uma conversa, fora do ThreadView herdado (D71).
// Abrir: a marca deste aparelho (ou o `bot.unread` do Hub) define as novas, congeladas para a linha e as marcas em
// accent. Ler: o que passa pela tela, em sequência, avança a marca, gravada durante a visita (R1-01/R1-02 do #511).

export function useVisita({ botId, sessionId, messages, unread, following, streamRev, hasOlder, visible, openSessions }: {
  botId: string
  sessionId: string | null
  /** As mensagens suas e do bot carregadas, em ordem. */
  messages: readonly Seen[]
  /** `bot.unread` do Hub agora (só vale sem marca, na principal). */
  unread: number
  following: boolean
  /** O `rev` do hello do stream atual (store.streamRev); null antes do 1º hello. */
  streamRev: number | null
  /** Ainda há histórico mais antigo por carregar. */
  hasOlder: boolean
  /** As linhas à vista (visible-rows.ts). */
  visible: ReadonlySet<string>
  /** As sessões extras abertas deste bot, quando a lista chegou (para esquecer as arquivadas ou apagadas). */
  openSessions: ReadonlySet<string> | null
}) {
  const key = markKey(botId, sessionId)
  const [visit, setVisit] = useState<Visit | null>(null)
  if (!visit && messages.length) setVisit(openVisit(messages, prefs.chatReadMarks.get()[key], sessionId === null, unread))
  else if (visit) {
    // R2-01: carga e catch-up pela revisão do stream (rev até o do hello), não pelo relógio; ao vivo, só fora do fim.
    const next = nextVisit(visit, messages, following, streamRev)
    if (next !== visit) setVisit(next)
  }

  // O que esteve na tela nesta visita; a marca só avança sobre ele em sequência (R1-01).
  const seen = useRef(new Set<string>())
  const read = useRef<number | null>(null)
  const saver = useMemo(() => markSaver((at) => {
    const next = advanceMark(prefs.chatReadMarks.get(), key, at)
    if (next) prefs.chatReadMarks.set(next)
  }, 1000), [key])
  const byId = useMemo(() => new Map(messages.map((m) => [m.id, m])), [messages])
  let lastVisible = 0
  for (const id of visible) lastVisible = Math.max(lastVisible, byId.get(id)?.at ?? 0)
  // Só conta como vista a resposta que ficou na tela por DWELL_MS: a rolagem suave de um pulo (barrinha, pílula)
  // passa pelo meio sem lê-lo.
  const since = useRef(new Map<string, number>())
  const [tick, setTick] = useState(0)
  useEffect(() => {
    const now = Date.now()
    for (const id of [...since.current.keys()]) if (!visible.has(id)) since.current.delete(id)
    for (const id of visible) if (!since.current.has(id) && byId.get(id)?.mine === false) since.current.set(id, now)
    if (!since.current.size) return
    const timer = setTimeout(() => {
      const limit = Date.now() - DWELL_MS
      let added = false
      for (const [id, at] of since.current) if (at <= limit && !seen.current.has(id)) { seen.current.add(id); added = true }
      if (added) setTick((n) => n + 1)
    }, DWELL_MS)
    return () => clearTimeout(timer)
  }, [visible, byId])
  useEffect(() => {
    if (!visit) return
    const from = read.current ?? visit.mark
    const earliest = messages.find((m) => !m.mine)?.at ?? Infinity
    const at = advanceContiguous(from, messages, seen.current, hasOlder && earliest > from)
    if (at > from) {
      read.current = at
      saver.push(at)
    }
  }, [tick, visit, messages, hasOlder, saver])
  // R1-02: grava já ao sair da conversa e ao fechar ou recarregar a janela.
  useEffect(() => {
    const flush = () => saver.flush()
    window.addEventListener('pagehide', flush)
    window.addEventListener('beforeunload', flush)
    return () => {
      window.removeEventListener('pagehide', flush)
      window.removeEventListener('beforeunload', flush)
      flush()
    }
  }, [saver])
  // Marcas de sessões que não estão mais abertas saem do mapa (só com a lista de sessões em mãos).
  useEffect(() => {
    if (!openSessions) return
    const next = forgetSessions(prefs.chatReadMarks.get(), botId, openSessions)
    if (next) prefs.chatReadMarks.set(next)
  }, [botId, openSessions])

  const unseen = visit?.unseen ?? EMPTY
  return { unseen, lineId: visit?.line ?? null, below: countBelow(unseen, seen.current, messages, lastVisible) }
}
const EMPTY: ReadonlySet<string> = new Set()
/** Tempo mínimo na tela para uma resposta contar como lida. */
export const DWELL_MS = 600

/**
 * Ao abrir (ou quando a linha chega com o catch-up e você ainda está no fim): as novas que não cabem numa tela abrem
 * na linha, perto do topo; senão, a conversa fica no fim com a linha à vista.
 */
export function useAbrirNaLinha(scroller: RefObject<HTMLElement | null>, lineId: string | null, ready: boolean, following: { current: boolean }, leave: () => void) {
  const done = useRef<string | null>(null)
  useLayoutEffect(() => {
    const el = scroller.current
    if (!el || !ready || !lineId || done.current === lineId || !following.current) return
    const row = el.querySelector<HTMLElement>(`[data-chat-item="${CSS.escape(lineId)}"]`)
    if (!row) return
    done.current = lineId
    const header = parseFloat(getComputedStyle(el).getPropertyValue('--chat-header-height')) || 96
    const top = el.scrollTop + row.getBoundingClientRect().top - el.getBoundingClientRect().top - header - 36
    if (openAt(el.scrollHeight - top, el.clientHeight) === 'line') {
      following.current = false
      leave()
      el.scrollTop = top
    }
  })
}

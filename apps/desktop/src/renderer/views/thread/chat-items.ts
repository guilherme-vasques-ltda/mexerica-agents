import { useRef } from 'react'
import type { Entry } from '@shared/models'
import { groupExchanges, type BotExchange, type ChatSlot } from './bot-exchange'
import type { AppLanguage } from '@shared/fork-idioma'
import { formatarData, type Tradutor } from '../../lib/fork-i18n'
import { placeMemoryNotices } from '../fork/memoria-auto/placement'
import { entriesOfSession } from '../fork/sessoes/estado'
import { naFila } from '../fork/conversa/fila'
import { pedidoNaFila } from '../fork/conversa/pedidos'
import { receiptAnchors, type ReceiptAnchor } from '../fork/recibo/presentation'
import { stepsByReply } from './cards/tool-steps'

export type ChatItem =
  | { id: string; kind: 'separator'; date: number }
  | { id: string; kind: 'entry'; entry: Entry; groupStart: boolean }
  | { id: string; kind: 'exchanges'; entry: Entry; exchanges: BotExchange[] }

/**
 * Chat-visible entries plus time separators (gaps > 1 h) and author grouping. A bot's messages
 * arrive whole (Grok Bot's `send_message`); what it writes along the way is trace. Runs of bot
 * exchanges with one peer fold into one item.
 */
export function buildChat(entries: Entry[]): ChatItem[] {
  const out: ChatItem[] = []
  let lastDate: number | null = null
  let lastAuthor: string | null = null
  const slots: ChatSlot[] = groupExchanges(placeMemoryNotices(entries))
  for (const { entry: e, exchanges } of slots) {
    const id = e.kind === 'user' && e.data.clientNonce ? `user-${e.data.clientNonce}` : e.id
    if (lastDate === null || e.createdAt - lastDate > 3_600_000) {
      out.push({ id: `sep-${id}`, kind: 'separator', date: e.createdAt })
      lastAuthor = null
    }
    if (exchanges) {
      out.push({ id, kind: 'exchanges', entry: e, exchanges })
      lastAuthor = null
      lastDate = e.createdAt
      continue
    }
    // In a group each bot is its own author.
    const author = e.kind === 'user' ? 'user' : e.kind === 'agent' ? `agent:${e.data.author ?? ''}` : e.kind
    out.push({ id, kind: 'entry', entry: e, groupStart: author !== lastAuthor })
    lastAuthor = author === 'user' || e.kind === 'agent' ? author : null
    lastDate = e.createdAt
  }
  return out
}

/** O que a tela precisa da lista bruta: entradas da sessão, itens, passos por resposta e âncoras de recibo. */
export interface ChatView {
  entries: Entry[]
  items: ChatItem[]
  steps: ReturnType<typeof stepsByReply>
  anchors: Map<string, ReceiptAnchor>
}

/**
 * MX-4 (C10): a conversa pronta, cacheada pela lista da loja. A troca de conversa remonta a tela e o
 * pipeline não se repete na volta; a loja troca a lista a cada evento do chat, então a chave sempre bate.
 */
const chatViews = new WeakMap<Entry[], Map<string, ChatView>>()

export function chatView(chatEntries: Entry[], chatId: string, sessionId: string | null, known: ReadonlySet<string>): ChatView {
  const key = JSON.stringify([chatId, sessionId, known.size ? [...known].sort() : []])
  let inner = chatViews.get(chatEntries)
  const cached = inner?.get(key)
  if (cached) return cached
  const entries = entriesOfSession(chatEntries, chatId, sessionId, known).filter((e) => !naFila(e) && !pedidoNaFila(e))
  const view: ChatView = { entries, items: buildChat(entries), steps: stepsByReply(entries), anchors: receiptAnchors(entries) }
  if (!inner) chatViews.set(chatEntries, (inner = new Map()))
  inner.set(key, view)
  return view
}

/**
 * Rows that arrived after the list first showed, while it follows the newest: they pop in
 * (`.row-in`, Grok Bot's). Earlier pages and rows that came in while reading history don't.
 */
export function useArrivals(ids: string[], following: boolean): Set<string> {
  const seen = useRef<Set<string> | null>(null)
  const fresh = useRef(new Set<string>())
  if (seen.current === null) seen.current = new Set(ids)
  for (const id of ids) {
    if (seen.current.has(id)) continue
    seen.current.add(id)
    if (following) fresh.current.add(id)
  }
  return fresh.current
}

/** A new row pops in from its bubble's side. */
export function rowIn(item: ChatItem): string {
  return item.kind === 'entry' && item.entry.kind === 'user' ? 'row-in mine' : 'row-in'
}

// TRAD-1 (D165 fase E): pt-BR, "09:12", "14 out", "segunda-feira".
const time = new Intl.DateTimeFormat('pt-BR', { hour: '2-digit', minute: '2-digit' })
const monthDayFormat = new Intl.DateTimeFormat('pt-BR', { month: 'short', day: 'numeric' })
const monthDay = { format: (date: Date) => monthDayFormat.format(date).replace('.', '').replace(' de ', ' ') }
const weekday = new Intl.DateTimeFormat('pt-BR', { weekday: 'long' })

const sameDay = (a: Date, b: Date) => a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate()

export const relativeTime = {
  time: (ms: number) => time.format(new Date(ms)),
  /** Chat separator: Hoje 09:27 · Ontem 17:20 · 16 set 09:02 (fatia 2: no idioma do app, conversa.mensagens.separador.*). */
  separator(t: Tradutor, idioma: AppLanguage, ms: number) {
    const date = new Date(ms)
    const now = new Date()
    const yesterday = new Date(now.getTime() - 86_400_000)
    const hora = time.format(date)
    if (sameDay(date, now)) return t('conversa.mensagens.separador.hoje', { hora })
    if (sameDay(date, yesterday)) return t('conversa.mensagens.separador.ontem', { hora })
    const data = idioma === 'pt-BR' ? monthDay.format(date) : formatarData(idioma, date, { month: 'short', day: 'numeric' })
    return t('conversa.mensagens.separador.data', { data, hora })
  },
  /** Roster stamp: 03:45 · Ontem · quarta-feira · 16 set */
  day(ms: number) {
    const date = new Date(ms)
    const now = new Date()
    if (sameDay(date, now)) return time.format(date)
    if (sameDay(date, new Date(now.getTime() - 86_400_000))) return 'Ontem'
    if (now.getTime() - ms < 6 * 86_400_000) return weekday.format(date)
    return monthDay.format(date)
  },
  /** Grok-style compact age: now · 5m · 3h · 2d · Sep 3 */
  short(ms: number) {
    const s = Math.max(0, (Date.now() - ms) / 1000)
    if (s < 60) return 'agora'
    if (s < 3600) return `${Math.floor(s / 60)}m`
    if (s < 86_400) return `${Math.floor(s / 3600)}h`
    if (s < 86_400 * 7) return `${Math.floor(s / 86_400)}d`
    return monthDay.format(new Date(ms))
  },
}

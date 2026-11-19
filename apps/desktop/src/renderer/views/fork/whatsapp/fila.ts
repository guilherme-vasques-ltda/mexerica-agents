// MX-18: a fila de saída do WhatsApp como o app mostra (donnaWhatsappQueue / donnaWhatsappQueueDecide).
// Puro, sem React nem I/O: o que o Hub manda vira dado; a view traduz estado e motivo.

import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'

export type EstadoFila = 'pending' | 'held' | 'sent' | 'uncertain' | 'expired' | 'cancelled' | (string & {})
export type OrigemFila = 'owner_card' | 'bot_human' | 'bot_routine' | (string & {})

export interface ItemFila {
  id: string
  botId: string
  /** JID da conversa: nunca telefone solto; a tela mostra o nome da conversa quando a acha. */
  chatId: string
  isGroup: boolean
  replyTo: string
  text: string
  origin: OrigemFila
  state: EstadoFila
  reason: string | null
  notBefore: number
  attempts: number
  sentAt: number | null
  createdAt: number
}

/** Resposta de outro formato é erro, nunca uma lista vazia. */
export function conferirFila(raw: unknown): ItemFila[] {
  const v = raw as { items?: unknown } | null
  if (!v || !Array.isArray(v.items) || !v.items.every((i) => i && typeof i.id === 'string' && typeof i.chatId === 'string' && typeof i.state === 'string')) {
    throw new Error('resposta_invalida')
  }
  return v.items as ItemFila[]
}

export const ESTADO_FILA: Record<string, ChaveTexto> = {
  pending: 'whatsapp.fila.estado.pendente',
  held: 'whatsapp.fila.estado.segurada',
  sent: 'whatsapp.fila.estado.enviada',
  uncertain: 'whatsapp.fila.estado.incerta',
  expired: 'whatsapp.fila.estado.vencida',
  cancelled: 'whatsapp.fila.estado.cancelada',
}

export const estadoFila = (i: Pick<ItemFila, 'state'>): ChaveTexto => ESTADO_FILA[i.state] ?? 'whatsapp.fila.estado.outro'

/** O motivo curto da linha (chave do catálogo); `null` quando o estado já diz tudo. */
export function motivoFila(i: Pick<ItemFila, 'state' | 'reason' | 'notBefore'>): ChaveTexto | null {
  if (i.state === 'held') {
    if (i.reason === 'quiet') return 'whatsapp.fila.motivo.silencio'
    if (i.reason === 'limit') return 'whatsapp.fila.motivo.limite'
    if (i.reason === 'not_connected') return 'whatsapp.fila.motivo.semNumero'
    if (i.reason === 'not_installed') return 'whatsapp.fila.motivo.semWacli'
    return 'whatsapp.fila.motivo.espera'
  }
  if (i.state === 'pending' && i.reason === 'retry') return 'whatsapp.fila.motivo.tentativa'
  if (i.state === 'uncertain') return 'whatsapp.fila.motivo.incerta'
  return null
}

/** O dono decide: incerto vira "enviou" ou "não saiu"; pendente/segurada pode ser cancelada. */
export function decisoesDoItem(i: Pick<ItemFila, 'state'>): ('sent' | 'cancelled')[] {
  if (i.state === 'uncertain') return ['sent', 'cancelled']
  if (i.state === 'pending' || i.state === 'held') return ['cancelled']
  return []
}

/** O resumo do rodapé: quantos esperam, quantos pedem decisão. */
export function resumoFila(items: readonly ItemFila[]) {
  const atencao = items.filter((i) => i.state === 'uncertain').length
  const esperando = items.filter((i) => i.state === 'pending' || i.state === 'held').length
  return { total: items.length, esperando, atencao }
}

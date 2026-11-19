// Pedidos de outros bots na bandeja "Na fila" (entrega do dono 10/11/2026, MX-77): as regras puras dos pedidos
// ask_bot/message_bot pendentes — quais avisos entram na bandeja do destinatário, de onde vieram e como se
// misturam às mensagens do dono. Arquivo próprio para não conflitar com a fila de mensagens (fila.ts, PR #680).
// As ações vão ao Hub pelos mesmos métodos (`queuedRemove`/`queuedEdit`/`queuedSendNow`), estendidos no host a
// esse aviso (host/src/agent/bot/fork_fila.rs).
import type { Entry } from '@shared/models'
import { filaDaConversa, mover } from './fila.ts'

/**
 * Pedido de outro bot ainda na fila desta conversa: só o aviso DO DESTINATÁRIO (o do remetente fica na
 * conversa dele). O Hub grava `delegationId`/`botMessage`/`sourceChat`/`sourceSession` na admissão
 * (host/src/chat/team.rs); o ator do destinatário executa na conversa principal, um por vez.
 */
export const pedidoNaFila = (e: Entry): boolean =>
  e.kind === 'notice' && e.data.status === 'queued' && !!e.data.delegationId && e.data.botMessage?.targetBotId === e.botId

/** "De ‹bot›" do pedido na fila: nome do bot e, quando a lane dele não é a principal, a sessão ou a conversa. */
export function origemDoPedido(
  e: Entry,
  rotulos: {
    bot: (botId: string) => string | undefined
    sessao: (chatId: string, sessionId: string) => string | undefined
    conversa: (chatId: string) => string | undefined
  },
): string {
  const d = e.data
  const partes = [d.sourceBotId ? rotulos.bot(d.sourceBotId) ?? d.sourceBotId : '']
  if (d.sourceSession && d.sourceChat) {
    const titulo = rotulos.sessao(d.sourceChat, d.sourceSession)
    if (titulo) partes.push(titulo)
  } else if (d.sourceChat && d.sourceBotId && d.sourceChat !== d.sourceBotId) {
    const titulo = rotulos.conversa(d.sourceChat)
    if (titulo) partes.push(titulo)
  }
  return partes.filter(Boolean).join(' · ')
}

/**
 * A bandeja desta conversa: as mensagens do dono (filaDaConversa) mais os pedidos de bots pendentes, na ordem
 * de chegada. Pedido de bot não tem `queueOrder` nem alça de arraste: a posição exibida é a de admissão.
 */
export function filaComPedidos(entries: readonly Entry[], chatId: string, sessionId: string | null): Entry[] {
  const sessao = sessionId ?? chatId
  const pedidos = entries.filter((e) => pedidoNaFila(e) && (e.sessionId ?? e.botId) === sessao)
  if (!pedidos.length) return filaDaConversa(entries, chatId, sessionId)
  return [...filaDaConversa(entries, chatId, sessionId), ...pedidos].sort(
    (a, b) => (a.data.queueOrder ?? a.seq) - (b.data.queueOrder ?? b.seq) || a.seq - b.seq,
  )
}

/**
 * A posição que o Hub espera no `queuedMove`: o índice ENTRE AS MENSAGENS DO DONO (kind=user), porque o Hub não
 * conta pedidos de bots. `para` é o índice visual de destino (como em `mover`); pedido nunca conta nem se move.
 * `null` quando a mensagem não muda de lugar entre as do dono (ex.: só passou por cima de um pedido).
 */
export function posicaoEntreMensagens(itens: readonly Entry[], entry: Entry, para: number): number | null {
  const antes = itens.filter((e) => !pedidoNaFila(e))
  const de = antes.indexOf(entry)
  if (de < 0) return null
  const depois = mover(itens, itens.indexOf(entry), para)
  const alvo = depois.slice(0, depois.indexOf(entry)).filter((e) => !pedidoNaFila(e)).length
  return alvo === de ? null : alvo
}

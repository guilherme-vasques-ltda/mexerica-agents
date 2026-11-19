// MB-1b (D326): pergunta explícita do bot ao dono (`ask_owner`). O Hub grava o cartão como entrada `notice` com
// `data.card = "ownerQuestion"` e é a única fonte do estado; o app só lê e mostra. Responder é escrever na conversa:
// a próxima mensagem humana ali (pela caixa da conversa ou pelo Mission Board, o mesmo `send`) responde.

export type OwnerQuestionStatus = 'pending' | 'answered' | 'withdrawn'

/** Pergunta pendente que o Hub manda no bot (`Bot.pendingQuestions`), já filtrada pela audiência do Space. */
export interface PendingQuestion {
  entryId: string
  chatId: string
  /** Sessão adicional do catálogo; ausente na principal da conversa. */
  sessionId?: string
  text: string
  askedAt: number
}

/** O cartão no transcript. */
export interface OwnerQuestionCard {
  status: OwnerQuestionStatus
  chatId: string
  sessionId: string
  askedAt: number
  /** Mensagem humana que respondeu; só em `answered`. */
  answeredBy: string | null
  closedAt: number | null
}

const MAX_PENDING = 5
const STATUSES: ReadonlySet<string> = new Set<OwnerQuestionStatus>(['pending', 'answered', 'withdrawn'])

const str = (v: unknown): v is string => typeof v === 'string' && v.length > 0
const num = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)

/** Hub antigo (sem o campo) ou dado fora do contrato: nenhuma pergunta. Nunca mais que 5. */
export function readPendingQuestions(raw: unknown): PendingQuestion[] {
  if (!Array.isArray(raw)) return []
  const out: PendingQuestion[] = []
  for (const q of raw) {
    if (!q || typeof q !== 'object') continue
    const { entryId, chatId, sessionId, text, askedAt } = q as Record<string, unknown>
    if (!str(entryId) || !str(chatId) || !str(text) || !num(askedAt)) continue
    out.push({ entryId, chatId, ...(str(sessionId) ? { sessionId } : {}), text, askedAt })
    if (out.length === MAX_PENDING) break
  }
  return out
}

/** O cartão de uma entrada, ou `null` se ela não é um cartão de pergunta válido. */
export function ownerQuestionOf(data: { card?: unknown; ownerQuestion?: unknown } | null | undefined): OwnerQuestionCard | null {
  if (!data || data.card !== 'ownerQuestion' || !data.ownerQuestion || typeof data.ownerQuestion !== 'object') return null
  const q = data.ownerQuestion as Record<string, unknown>
  if (typeof q.status !== 'string' || !STATUSES.has(q.status) || !str(q.chatId)) return null
  return {
    status: q.status as OwnerQuestionStatus,
    chatId: q.chatId,
    sessionId: str(q.sessionId) ? q.sessionId : q.chatId,
    askedAt: num(q.askedAt) ? q.askedAt : 0,
    answeredBy: q.status === 'answered' && str(q.answeredBy) ? q.answeredBy : null,
    closedAt: num(q.closedAt) ? q.closedAt : null,
  }
}

/** Pílula do estado (ícone + texto, nunca só cor) e a frase do rodapé. */
export function ownerQuestionView(status: OwnerQuestionStatus, botName: string): { label: string; icon: string; note: string } {
  switch (status) {
    case 'pending':
      return { label: 'Aguardando sua resposta', icon: 'text.bubble', note: `Responda pela caixa de mensagem. Sua próxima mensagem aqui vai para ${botName} como resposta.` }
    case 'answered':
      return { label: 'Respondida', icon: 'checkmark', note: 'Respondida pela sua mensagem logo abaixo.' }
    case 'withdrawn':
      return { label: 'Retirada', icon: 'xmark.circle', note: 'Sem efeito: o bot fez outra pergunta ou a conversa foi reiniciada, limpa ou encerrada.' }
  }
}

/** MB-1b: Precisa de você só por pergunta, sem turno em andamento. Não é trabalho: sem Parar nem fila. */
export function onlyQuestionsPending(bot: { status: string; workingChat?: string | null; pendingQuestions?: readonly unknown[] }): boolean {
  return bot.status === 'needsInput' && !bot.workingChat && (bot.pendingQuestions?.length ?? 0) > 0
}

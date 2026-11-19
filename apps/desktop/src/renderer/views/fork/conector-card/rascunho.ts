/** Dado privado de rascunho. Protocolo de decisão/permit não existe neste formato. */
export interface RascunhoWhatsapp {
  text: string
  account: string
  replyTo: { messageId: string; sender: string; excerpt: string; origin: 'external' }
}

export function lerRascunho(raw: unknown, draftId: string, agora: number): RascunhoWhatsapp | null {
  if (!raw || typeof raw !== 'object') return null
  const value = raw as Record<string, unknown>
  const receipt = value.receipt as Record<string, unknown> | undefined
  const capability = value.capability as Record<string, unknown> | undefined
  if (receipt?.draftId !== draftId || receipt.state !== 'draft' || typeof receipt.deadline !== 'number' ||
    !Number.isFinite(receipt.deadline) || receipt.deadline <= agora || capability?.approve !== false || capability.send !== false) return null
  const p = value.presentation as Record<string, unknown> | undefined
  const reply = p?.replyTo as Record<string, unknown> | undefined
  if (typeof p?.text !== 'string' || typeof p.account !== 'string' || !p.account ||
    typeof reply?.messageId !== 'string' || !reply.messageId || typeof reply.sender !== 'string' ||
    typeof reply.excerpt !== 'string' || reply.origin !== 'external') return null
  return { text: p.text, account: p.account, replyTo: { messageId: reply.messageId, sender: reply.sender, excerpt: reply.excerpt, origin: 'external' } }
}

/** Prazo local só oculta conteúdo; apenas o recibo do Hub confirma fechamento. */
export function telaRascunho(
  draft: { state: 'draft' | 'closed'; deadline: number }, failed: boolean, agora: number,
  presentation: RascunhoWhatsapp | null,
) {
  const closed = draft.state === 'closed'
  return {
    closed,
    visible: !closed && !failed && agora < draft.deadline ? presentation : null,
    failed: !closed && failed,
  }
}

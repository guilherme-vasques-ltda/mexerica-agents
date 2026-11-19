// UI generativa no chat, G1b (D273): lê do fio a entrada `notice` com o cartão de escolha que o Hub
// publicou (`data.genuiChoice`; contrato em docs/features/ui-generativa-chat.md). O Hub grava só
// `{text, genuiChoice}`, sem `data.card` (D-1(b) do contrato sessionChoiceAnswer v1): monta pela forma
// fechada + `parseChoice`, nunca em entrada que não seja `notice` (a `user` é o que o dono ou o `send`
// gravam). Puro: sem React, rede ou store. Forma fora do contrato devolve null e a linha continua o texto
// literal da entrada (`data.text`), o mesmo fallback do app antigo. Nada aqui prova autoridade: só o Hub decide.

import { parseChoice, type ChoiceHubState } from './escolha-model.ts'
import type { Tradutor } from '../../../lib/fork-i18n/index.ts'

/** O cartão como o Hub o registrou, com onde a resposta volta (sessão extra, ou Space e conversa da própria entrada). */
export interface ChoiceEntry {
  questionId: string
  /** JSON `choice` v1 canônico do Hub, já aceito por `parseChoice`; o ChoiceCard o valida de novo. */
  snapshot: string
  hub: ChoiceHubState
  /** Space da entrada; ausente no histórico legado, que é só do Pessoal (quem usa resolve). */
  spaceId: string | null
  /** A conversa da entrada: `conversationId` do fio, senão o `botId` (no fio é o id do chat; ver entryIn). */
  conversationId: string
  /** `botId` e `sessionId` BRUTOS da entrada (sem bot lógico nem conversa derivada): `sessionId` ≠ `botId` = extra UI01. */
  botId: string
  sessionId: string | null
}

const KEYS = new Set(['version', 'questionId', 'revision', 'state', 'snapshot', 'answer', 'answeredAt', 'at'])
const isTime = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)

/** A projeção `genuiChoice` da entrada, ou null (forma desconhecida ou versão nova). */
export function genuiChoiceOf(entry: { kind: string; botId: string; spaceId?: string; conversationId?: string; sessionId?: string; data: object }): ChoiceEntry | null {
  const data = entry.data as { card?: unknown; genuiChoice?: unknown }
  // Só `notice` (lista permitida, nunca `user`); `card` ausente (Hub atual) ou o legado `genuiChoice`, nunca outro cartão.
  if (entry.kind !== 'notice' || (data.card !== undefined && data.card !== 'genuiChoice')) return null
  const g = data.genuiChoice
  if (typeof g !== 'object' || g === null || Array.isArray(g) || Object.keys(g).some((k) => !KEYS.has(k))) return null
  const { version, questionId, revision, state, snapshot, answer, answeredAt, at } = g as Record<string, unknown>
  if (version !== 1 || typeof questionId !== 'string' || !questionId || typeof snapshot !== 'string') return null
  // O payload também precisa estar no contrato (JSON fechado, `choice` v1): senão o cartão só diria
  // "Pergunta indisponível" e esconderia o texto literal da entrada, que é o fallback legível.
  if (!parseChoice(snapshot).ok) return null
  if (typeof revision !== 'number' || !Number.isSafeInteger(revision) || revision < 1) return null
  const conversationId = typeof entry.conversationId === 'string' && entry.conversationId ? entry.conversationId : entry.botId
  if (typeof conversationId !== 'string' || !conversationId) return null
  const spaceId = typeof entry.spaceId === 'string' && entry.spaceId ? entry.spaceId : null
  const sessionId = typeof entry.sessionId === 'string' && entry.sessionId ? entry.sessionId : null
  const where = { questionId, snapshot, spaceId, conversationId, botId: entry.botId, sessionId }
  switch (state) {
    case 'pending': return { ...where, hub: { state, revision } }
    // A resposta é conferida de novo pelo cartão contra a pergunta; inválida vira "Estado indisponível".
    case 'answered': return isTime(answeredAt) && answer !== undefined ? { ...where, hub: { state, revision, answer, answeredAt } } : null
    case 'cancelled':
    case 'expired': return { ...where, hub: { state, revision, at: isTime(at) ? at : null } }
    default: return null
  }
}

/** Por que o envio não valeu, como o store o guarda (código estável, nunca texto do Hub). */
export type ChoiceSendError = 'revision' | 'final' | 'closed' | 'invalid' | 'scope' | 'unsupported' | 'forbidden' | 'link' | 'generic'

/**
 * Classifica um erro do Hub/transporte só pelo código estável do começo da mensagem (`código` ou `código: …`) e
 * pelo status; o resto nunca é mostrado. Inclui os códigos de `sessionChoiceAnswer` (contrato v1, R3 seção 3).
 */
export function choiceSendError(e: unknown): ChoiceSendError {
  const status = typeof e === 'object' && e !== null ? (e as { status?: unknown }).status : undefined
  const message = e instanceof Error ? e.message : ''
  const code = /^[a-z_]+(?=$|:)/.exec(message)?.[0] ?? ''
  if (status === 0) return 'link'
  if (status === 401 || status === 403 || /^(forbidden|control_required|unauthorized|not_allowed)$/.test(code)) return 'forbidden'
  if (/^(genui_revision_conflict|conversation_revision_conflict|session_revision_conflict|genui_cas_conflict)$/.test(code)) return 'revision'
  if (/^(genui_final_state|genui_deadline_elapsed)$/.test(code)) return 'final'
  if (/^(conversation_closed|genui_context_reset|conversation_not_found|session_closed)$/.test(code)) return 'closed'
  if (/^(genui_invalid_answer|genui_unknown_option|genui_invalid_text|invalid_field|genui_question_required)$/.test(code)) return 'invalid'
  if (message === 'unknown bot' || /^(genui_question_scope_mismatch|genui_unknown_question|unknown_session|genui_session_mismatch|genui_entry_session_mismatch|genui_scope_mismatch|bot_deleted)$|^(cross_|stale_|hub_context_mismatch)/.test(code)) return 'scope'
  if (/^(genui_conversation_required|unknown_conversation_method|space_context_unavailable|session_chat_unsupported)$/.test(code)) return 'unsupported'
  // `engine_unavailable`/`session_selection_unavailable` (conta ou modelo da extra indisponível) e o resto: genérico.
  return 'generic'
}

/**
 * O texto da linha de estado depois de um envio que não valeu (conversa.escolha.erro.*, seis idiomas).
 * Nunca ecoa resposta, código ou mensagem do Hub.
 */
export function choiceErrorText(t: Tradutor, error: ChoiceSendError): string {
  return t(`conversa.escolha.erro.${error}`)
}

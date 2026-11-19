// Estado das sessões (Barra05, passos 13 e 16). Módulo do fork (D71), puro para testar sem DOM.
// Adaptador trocável: enquanto o Hub não manda o estado por sessão, ele é derivado aqui do que já chega
// (status/workingChat do bot e aprovações pendentes por `sessionId`). PROVISÓRIO: quando o Hub mandar `activity`,
// `sessionState` usa o campo dele e a derivação vira só reserva para Hubs antigos.
import type { Bot, Entry, Session } from '../../../../shared/models.ts'
import { onlyQuestionsPending } from '../../../../shared/fork-owner-question.ts'
import { genuiChoiceOf } from '../genui/choice-entry.ts'
import type { ChaveTexto } from '../../../lib/fork-i18n/index.ts'
import type { Tradutor } from '../../../lib/fork-i18n/estado.ts'

/** Rodando, pede você, livre ou arquivada: o ponto `.ldot` desenha cada um. */
export type SessionState = 'running' | 'needsYou' | 'idle' | 'archived'

/** A sessão como o app a recebe; `activity` é o campo que o Hub ainda não manda (opcional). */
export type HubSession = Session & { activity?: 'running' | 'needsYou' | 'idle' }

/** A chave do estado no catálogo (DON-903); a view traduz com `t(SESSION_LABEL[estado])`. */
export const SESSION_LABEL = {
  running: 'sessions.state.running',
  needsYou: 'sessions.state.needsYou',
  idle: 'sessions.state.idle',
  archived: 'sessions.state.archived',
} as const satisfies Record<SessionState, ChaveTexto>

/** O nome da sessão na aba e no painel; antes da 1ª mensagem (UI-01), ela ainda não tem título. O título é do dono. */
export const sessionTitle = (s: Pick<HubSession, 'title'>, t: Tradutor) => s.title.trim() || t('sessions.untitled')

/** A sessão de uma entrada; Hubs anteriores não mandam `sessionId` e tudo é da principal. */
export const sessionOf = (entry: Entry, chatId: string) => entry.sessionId ?? chatId

/**
 * Uma pendência que espera o dono numa sessão. Hoje, as aprovações pendentes; a pergunta pendente da Torre
 * (MB-1b) entra depois como mais uma fonte desta lista, sem mudar a derivação.
 */
export interface PendingAsk { botId: string; sessionId: string }

/** As aprovações pendentes da conversa, como pendências por sessão. */
export function pendingApprovals(chatId: string, entries: readonly Entry[]): PendingAsk[] {
  return entries.filter((e) => e.kind === 'permission' && e.data.status === 'pending').map((e) => ({ botId: e.data.author ?? e.botId, sessionId: sessionOf(e, chatId) }))
}

/** Os cartões de escolha pendentes (`genuiChoice`, contrato em docs/features/ui-generativa-chat.md) também pedem você
 * (dono, 07/11): contam no Agora, no ponto do bot e na linha da sessão. */
export function pendingChoices(chatId: string, entries: readonly Entry[]): PendingAsk[] {
  return entries.filter((e) => genuiChoiceOf(e)?.hub.state === 'pending').map((e) => ({ botId: e.data.author ?? e.botId, sessionId: sessionOf(e, chatId) }))
}

/** As perguntas pendentes ao dono (MB-1b, `Bot.pendingQuestions`) nesta conversa; sem `sessionId`, valem para a
 * sessão principal dela. */
export function pendingQuestions(chatId: string, bots: readonly Bot[]): PendingAsk[] {
  return bots.flatMap((b) => b.pendingQuestions.filter((q) => q.chatId === chatId).map((q) => ({ botId: b.id, sessionId: q.sessionId ?? chatId })))
}

/** O que a derivação lê da conversa: a sessão em uso agora e as que têm pendência com você. */
export interface ChatActivity {
  /** Sessão do último pedido seu nesta conversa (onde o bot trabalha). */
  current: string
  /** Sessões com alguma pendência (aprovação ou, depois, pergunta da Torre). */
  waiting: ReadonlySet<string>
  /** Algum bot trabalha nesta conversa (o bot dela, ou um membro do canal). */
  working: boolean
  /** O bot (ou um membro do canal) espera você e nenhuma pendência diz em que sessão: vale a atual. */
  needsInput: boolean
}

/** `pending`: todas as fontes de "pede você", juntas; por padrão, só as aprovações. */
export function chatActivity(chatId: string, bots: readonly Bot[], entries: readonly Entry[], pending: readonly PendingAsk[] = pendingApprovals(chatId, entries)): ChatActivity {
  let current = chatId
  // R1-01 (#509): mensagem ainda na fila (`queued`) não leva o trabalho para a sessão dela.
  for (const e of entries) if (e.kind === 'user' && e.data.status !== 'queued') current = sessionOf(e, chatId)
  const here = bots.filter((b) => (b.workingChat ?? b.id) === chatId)
  return {
    current,
    waiting: new Set(pending.map((p) => p.sessionId)),
    working: here.some((b) => b.status === 'working'),
    // O status "precisa de você" só por pergunta já está nas pendências (com a sessão); não vale de novo para a atual.
    needsInput: !pending.length && here.some((b) => b.status === 'needsInput' && !onlyQuestionsPending(b)),
  }
}

export function sessionState(session: HubSession, activity: ChatActivity): SessionState {
  if (session.state === 'closed') return 'archived'
  if (session.activity) return session.activity
  if (activity.waiting.has(session.id) || (activity.needsInput && activity.current === session.id)) return 'needsYou'
  if (activity.working && activity.current === session.id) return 'running'
  return 'idle'
}

/** O estado do bot soma as sessões: alguma pedindo → pede você; alguma rodando → rodando; senão livre. */
export function combinedState(states: Iterable<SessionState>): Exclude<SessionState, 'archived'> {
  let running = false
  for (const s of states) {
    if (s === 'needsYou') return 'needsYou'
    if (s === 'running') running = true
  }
  return running ? 'running' : 'idle'
}

/** As sessões em andamento primeiro pela última vez usada; a principal fica fora (é a aba do bot). */
export function splitSessions(sessions: readonly HubSession[]) {
  const extra = sessions.filter((s) => !s.principal).sort((a, b) => b.lastUsedAt - a.lastUsedAt)
  return { open: extra.filter((s) => s.state === 'open'), archived: extra.filter((s) => s.state === 'closed') }
}

/** Resumo do Agora recolhido: "2 rodando · 1 pede você"; vazio, "Livre". */
export function nowSummary(states: readonly SessionState[], t: Tradutor) {
  const running = states.filter((s) => s === 'running').length
  const needs = states.filter((s) => s === 'needsYou').length
  const parts = [running ? t.plural('sessions.now.running', running) : '', needs ? t.plural('sessions.now.needsYou', needs) : '']
  return { running, needs, text: parts.filter(Boolean).join(' · ') || t('sessions.now.idle') }
}

/** As entradas de uma sessão; a principal leva também as sem sessão conhecida (Hub antigo, sessão sumida). */
export function entriesOfSession<T extends Entry>(entries: T[], chatId: string, sessionId: string | null, known: ReadonlySet<string>): T[] {
  if (!known.size && sessionId === null) return entries
  return entries.filter((e) => {
    const s = sessionOf(e, chatId)
    return sessionId === null ? s === chatId || !known.has(s) : s === sessionId
  })
}

/** O estado de um bot só pelo status (membros do canal): trabalhando → rodando; esperando você → pede você. */
export const botState = (bot: Pick<Bot, 'status'>): Exclude<SessionState, 'archived'> =>
  bot.status === 'needsInput' ? 'needsYou' : bot.status === 'working' ? 'running' : 'idle'

/** Como o rosto e o cabeçalho dizem o estado do bot (soma das sessões). */
export const BOT_STATE_LABEL = { running: 'sessions.bot.running', needsYou: 'sessions.bot.needsYou', idle: 'sessions.bot.idle' } as const satisfies Record<Exclude<SessionState, 'archived'>, ChaveTexto>

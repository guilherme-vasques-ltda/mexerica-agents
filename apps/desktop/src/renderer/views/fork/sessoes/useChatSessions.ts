import { spaceAllows } from '../../../lib/fork-space-capabilities'
import { useEffect, useMemo } from 'react'
import { isGroup, type Bot } from '@shared/models'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import { chatActivity, combinedState, pendingApprovals, pendingChoices, pendingQuestions, sessionState, splitSessions, type HubSession, type SessionState } from './estado'

export interface ChatSessions {
  /** Sessões em andamento (sem a principal), da usada por último para a mais antiga. */
  open: HubSession[]
  archived: HubSession[]
  /** Ids das sessões além da principal: as entradas delas saem da conversa principal. */
  known: ReadonlySet<string>
  state: (id: string) => SessionState
  /** O estado da conversa principal e o do bot (soma de todas). */
  main: SessionState
  /** A principal como o Hub a manda (título e última vez usada); null antes da lista chegar. */
  principal: HubSession | null
  combined: Exclude<SessionState, 'archived'>
}

/** As sessões de uma conversa (bot ou canal) com o estado de cada uma, para as abas e o painel. */
export function useChatSessions(store: BotStore, chat: Bot | null): ChatSessions {
  const model = useModel(store.sessions)
  const chatId = chat?.id ?? ''
  const list = model.of(chatId)
  useEffect(() => {
    if (spaceAllows(store.spaceId, 'sessions') && chatId && !model.of(chatId)) void model.load(chatId)
  }, [model, chatId])
  const entries = store.allEntries(chatId)
  const members = chat ? (isGroup(chat) ? store.members(chat) : [chat]) : []
  const runtime = members.map((b) => `${b.id}:${b.status}:${b.workingChat ?? ''}:${b.pendingQuestions.map((q) => `${q.chatId}/${q.sessionId ?? ''}`).join(',')}`).join('|')
  return useMemo(() => {
    // As fontes de "pede você": aprovações, cartões de escolha (dono, 07/11) e perguntas ao dono (MB-1b) pendentes.
    const activity = chatActivity(chatId, members, entries, [...pendingApprovals(chatId, entries), ...pendingChoices(chatId, entries), ...pendingQuestions(chatId, members)])
    const all = list ?? []
    const states = new Map<string, SessionState>(all.map((s) => [s.id, sessionState(s, activity)]))
    const principal = all.find((s) => s.principal)
    const main = principal ? states.get(principal.id)! : sessionState({ id: chatId, state: 'open' } as HubSession, activity)
    const { open, archived } = splitSessions(all)
    return {
      open,
      archived,
      known: new Set(all.filter((s) => !s.principal).map((s) => s.id)),
      state: (id: string) => states.get(id) ?? (id === chatId ? main : 'idle'),
      main,
      principal: principal ?? null,
      combined: combinedState([main, ...states.values()]),
    }
    // `members` muda de identidade a cada render; `runtime` resume o que importa dele.
  }, [chatId, list, entries, runtime])
}

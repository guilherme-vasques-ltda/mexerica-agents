import type { Space, SpacesSnapshot, SpaceConversation } from '../../shared/fork-spaces.ts'
import { isChat, type Entry } from '../../shared/models.ts'
import { cru, frase, plural, type Frase } from './fork-i18n/frase.ts'

// D169 (components/SpaceView; docs/reference/spaces.md): pure rules of an open space's view over what the Hub sends.
// Nothing here calls the Hub or decides authority; store/fork-space-context.ts holds the context and the calls.
// D454 (fatia 9c) e D507: o que vai à tela volta como `Frase` (chave e parâmetros; nome e título do Hub como dado).

/** The open conversations in the Hub's order (each bot's fixed one first), then the closed ones. */
export function conversationGroups(list: readonly SpaceConversation[]) {
  return { open: list.filter((c) => !c.closed), closed: list.filter((c) => c.closed) }
}

/** Without the principal's fixed conversation, a newly opened space shows the first open one, as the Hub ordered them; none if all are closed. */
export const firstConversation = (list: readonly SpaceConversation[]) => list.find((c) => !c.closed)?.id ?? null

/** Abertura no Berga (03/11/2026): a conversa fixa e aberta da principal deste space, onde quer que o Hub a ponha. */
export const principalConversation = (list: readonly SpaceConversation[], principalId: string | null | undefined) =>
  principalId ? list.find((c) => c.primary && !c.closed && c.botId === principalId)?.id ?? null : null

/** "Conversa fixa com {bot}" for each bot's fixed conversation, the owner's title for the others. */
export function conversationLabel(c: SpaceConversation, botName: Frase): Frase {
  return c.primary ? frase('spaces.conversa.fixa', { bot: botName }) : cru(c.title)
}

/** A bot's name in this space: the principal from the Hub's presentation, others from the space's own bots.
 *  `foreign` liga o botId ao rótulo do Space dono quando a conversa mistura Spaces (só fora do Pessoal). */
export function botName(botId: string, principal: { id: string; name: string } | null, bots: ReadonlyMap<string, string>, foreign?: { personalId: string | null; spaces: ReadonlyMap<string, string> }): Frase {
  if (principal && principal.id === botId) return cru(principal.name)
  if (foreign) {
    if (botId === foreign.personalId) return frase('spaces.conversa.bergadoPessoal')
    const space = foreign.spaces.get(botId)
    if (space !== undefined) return frase('spaces.conversa.bergaDoSpace', { space })
  }
  const name = bots.get(botId)
  return name !== undefined ? cru(name) : frase('spaces.conversa.botDesteSpace')
}

/** An entry of this space's conversation, and only of it: a late or foreign one never reaches the screen. */
export function entryIn(e: Entry, spaceId: string, conversationId: string) {
  return e.spaceId === spaceId && (e.conversationId ?? e.botId) === conversationId
}

/** What the history shows: the chat's own lines, in seq order, one per id (a newer revision of an entry wins). */
export function mergeHistory(current: readonly Entry[], incoming: readonly Entry[]): Entry[] {
  const byId = new Map(current.map((e) => [e.id, e]))
  for (const e of incoming) {
    const known = byId.get(e.id)
    if (!known || e.rev >= known.rev) byId.set(e.id, e)
  }
  return [...byId.values()].filter(isChat).sort((a, b) => a.seq - b.seq)
}

/** How the conversation's context started, from the Hub's own continuity record; null when the Hub sent none. */
export function continuityText(c: SpaceConversation): Frase | null {
  if (!c.continuity) return null
  if (c.continuity.mode === 'fresh') return frase('spaces.continuidade.zero')
  const n = c.continuity.coverage?.messages
  return n ? plural('spaces.continuidade.ultimas', n) : frase('spaces.continuidade.visiveis')
}

/** Why a read of this space failed, as a phrase translated on screen; never the raw code and never an empty list. */
export function readErrorText(e: unknown): { kind: 'offline' | 'gone' | 'unavailable' | 'refused'; text: Frase } {
  const status = typeof e === 'object' && e !== null ? (e as { status?: unknown }).status : undefined
  const message = e instanceof Error ? e.message : String(e)
  if (status === 0) return { kind: 'offline', text: frase('spaces.leitura.semConexao') }
  if (/space_context_unavailable|unknown_conversation_method|spaces_unavailable/.test(message)) return { kind: 'unavailable', text: frase('spaces.leitura.indisponivel') }
  if (/space_not_found|invalid_space/.test(message)) return { kind: 'gone', text: frase('spaces.leitura.spaceSumiu') }
  if (/conversation_not_found|conversation_bot_unavailable|invalid_conversation_cursor/.test(message)) return { kind: 'gone', text: frase('spaces.leitura.conversaSumiu') }
  if (/hub_context_mismatch|cross_hub|cross_space|cross_conversation|stale_/.test(message)) return { kind: 'refused', text: frase('spaces.leitura.outroHub') }
  return { kind: 'refused', text: frase('spaces.leitura.recusada') }
}

/** The open view belongs to this exact store and Hub, including while its connection is offline. */
export interface OpenSpaceOrigin<S> { store: S; hostId: string; spaceId: string }
export function retainedSpace<S extends { hello: { hostId: string; spacesContract?: number } | null; spaces: SpacesSnapshot | null }>(
  visible: readonly S[], origin: OpenSpaceOrigin<S> | null,
): Space | null {
  if (!origin || !visible.includes(origin.store) || origin.store.hello?.hostId !== origin.hostId || origin.store.hello.spacesContract !== 1) return null
  return origin.store.spaces?.spaces.find((s) => s.id === origin.spaceId && s.kind === 'space') ?? null
}

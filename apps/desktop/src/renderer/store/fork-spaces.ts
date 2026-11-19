import { SpacePrincipalIntent } from '../../shared/fork-space-birth.ts'
import type { SpaceTile, SpacesSnapshot, SpaceDeleteResult } from '../../shared/fork-spaces.ts'
import type { BotEditor, BotEditorPatch } from '../../shared/fork-bot-editor.ts'
import { SpaceClient } from '../../shared/fork-spaces-client.ts'
import type { HostClient } from '../client/host-client.ts'
import { failureOf, movedOrder, originRefusal, spaceErrorText } from '../lib/fork-spaces-rail.ts'
import type { BotStore } from './bot-store.ts'
import { ErroFrase, frase } from '../lib/fork-i18n/frase.ts'

// D169 (docs/reference/spaces.md; components/SpaceRail, SpaceIconPicker): the three changes the rail asks of a Hub
// (create a space, edit its name and tile, move it), through `SpaceClient` with the Hub's id from its hello. Each
// starts from an origin captured when its form opened (or the move was clicked) and kept for its whole life: the
// Hub, link, generation and list it came from. Before the Hub is called that origin is checked again, and the
// revision sent is the origin's, so another Hub in the same store or a newer list (an event, a re-read) sends
// nothing. The Hub validates, keeps and answers the whole list, which replaces the store's only while the same Hub,
// link and generation asked (`BotStore.acceptSpaces`). A stale revision, a gone space or no answer reads the list
// again and never repeats the change. Errors come back as `ErroFrase` (D507): the picker or the rail translates them
// when it draws.

/** Where a form (when it opened) or a move (when clicked) started; fixed for its whole life. */
export interface SpacesOrigin { store: BotStore; hostId: string; client: HostClient; epoch: number; snapshot: SpacesSnapshot }

/** The store's origin now, or null without a Hub that has spaces on a live link. */
export function spacesOrigin(store: BotStore): SpacesOrigin | null {
  const hostId = store.hello?.hostId
  const client = store.connectedClient
  const snapshot = store.spaces
  if (!hostId || !client || !snapshot || store.hello?.spacesContract !== 1) return null
  return { store, hostId, client, epoch: store.principalEpoch, snapshot }
}

const sameLink = ({ store, hostId, client, epoch }: SpacesOrigin) =>
  store.hello?.hostId === hostId && store.connectedClient === client && store.principalEpoch === epoch

/** The list as the store keeps it, without the `hubId` the Hub echoes on these answers. */
const listOf = (answer: SpacesSnapshot): SpacesSnapshot => ({ version: answer.version, personalSpaceId: answer.personalSpaceId, revision: answer.revision, spaces: answer.spaces })

const MOVED = frase('spaces.erro.respostaOutraConexao')

/** Reads the list again on the same link, read only; true when it was read there. */
async function reread(origin: SpacesOrigin): Promise<boolean> {
  if (!sameLink(origin)) return false
  const client = new SpaceClient(origin.hostId, origin.snapshot.personalSpaceId, origin.client)
  try {
    origin.store.acceptSpaces(listOf(await client.spaces()), origin)
    return sameLink(origin)
  } catch {
    return false
  } finally {
    client.dispose()
  }
}

/** One change on the Hub, scoped to `target` (a space) or to Pessoal for the list itself. */
async function change(origin: SpacesOrigin, target: string | null, act: (client: SpaceClient, list: SpacesSnapshot) => Promise<SpacesSnapshot>) {
  const { store } = origin
  // Before the Hub is called: the same Hub, link, generation and list the change was prepared on (Sol R1-F1/F2).
  const now = { hostId: store.hello?.hostId ?? null, client: store.connectedClient, epoch: store.principalEpoch, revision: store.spaces?.revision ?? null }
  const refusal = originRefusal(now, { hostId: origin.hostId, client: origin.client, epoch: origin.epoch, revision: origin.snapshot.revision })
  if (refusal) throw new ErroFrase(refusal)
  const client = new SpaceClient(origin.hostId, target ?? origin.snapshot.personalSpaceId, origin.client)
  let answer: SpacesSnapshot
  try {
    answer = listOf(await act(client, origin.snapshot))
  } catch (e) {
    const failure = failureOf(e)
    throw new ErroFrase(spaceErrorText(e, failure === 'refused' ? null : await reread(origin)))
  } finally {
    client.dispose()
  }
  if (!store.acceptSpaces(answer, origin) && !sameLink(origin)) throw new ErroFrase(MOVED)
}

export function createSpace(origin: SpacesOrigin, draft: { name: string; tile: SpaceTile }) {
  return change(origin, null, (client, list) => client.create({ name: draft.name, tile: draft.tile, expectedRevision: list.revision }))
}

export function updateSpace(origin: SpacesOrigin, spaceId: string, draft: { name: string; tile: SpaceTile }) {
  return change(origin, spaceId, (client, list) => client.update({ expectedRevision: list.revision, patch: { name: draft.name, tile: draft.tile } }))
}

export async function moveSpace(origin: SpacesOrigin, spaceId: string, delta: -1 | 1) {
  // The menu only offers a move that exists in the list it was opened on.
  const ids = movedOrder(origin.snapshot, spaceId, delta)
  if (!ids) return
  await change(origin, null, (client, list) => client.reorder({ expectedRevision: list.revision, ids }))
}

/** D543 E1: ativa o Berga num Space legado, pelo cliente do próprio Space alvo. */
export function principalIntent(origin: SpacesOrigin, spaceId: string) {
  return new SpacePrincipalIntent(origin.store, origin.hostId, origin.client, origin.epoch, spaceId, origin.snapshot.revision)
}

export async function activateSpacePrincipal(origin: SpacesOrigin, spaceId: string,
  principal: { clientNonce: string; name?: string; backend: import('../../shared/fork-engine-accounts.ts').Engine; engineAccountId: string; appearance?: import('@shared/fork-spaces').SpacePrincipalAppearance },
  intent = principalIntent(origin, spaceId)) {
  if (!intent.fixed) {
    const { store } = origin
    const refusal = originRefusal({ hostId: store.hello?.hostId ?? null, client: store.connectedClient,
      epoch: store.principalEpoch, revision: store.spaces?.revision ?? null },
    { hostId: origin.hostId, client: origin.client, epoch: origin.epoch, revision: origin.snapshot.revision })
    if (refusal) throw new ErroFrase(refusal)
  }
  // A intenção conserva revisão/nonce/conteúdo após resposta perdida. Só a
  // primeira tentativa exige a revisão atual; replay nunca rebaseia o corpo.
  try {
    const { clientNonce, ...draft } = principal
    const answer = await intent.send({ clientNonce, principal: draft })
    if (!origin.store.acceptSpaces(listOf(answer), origin) && !sameLink(origin)) throw new ErroFrase(MOVED)
  } catch (e) {
    const failure = failureOf(e)
    throw new ErroFrase(spaceErrorText(e, failure === 'refused' ? null : await reread(origin)))
  }
}

/** D543 E1: exclusão definitiva de Space (só computador). `{busy}` volta com `{botId, name}`; a lista nova é aceita como nas outras mudanças. */
export async function deleteSpace(origin: SpacesOrigin, spaceId: string, confirmName: string): Promise<SpaceDeleteResult> {
  const { store } = origin
  if (store.hello?.spaceLifecycleContract !== 1) throw new ErroFrase(frase('spaces.delete.atualize'))
  const now = { hostId: store.hello?.hostId ?? null, client: store.connectedClient, epoch: store.principalEpoch, revision: store.spaces?.revision ?? null }
  const refusal = originRefusal(now, { hostId: origin.hostId, client: origin.client, epoch: origin.epoch, revision: origin.snapshot.revision })
  if (refusal) throw new ErroFrase(refusal)
  const client = new SpaceClient(origin.hostId, spaceId, origin.client)
  let answer: SpaceDeleteResult
  try {
    answer = await client.remove({ expectedRevision: origin.snapshot.revision, confirmName })
  } catch (e) {
    const failure = failureOf(e)
    throw new ErroFrase(spaceErrorText(e, failure === 'refused' ? null : await reread(origin)))
  } finally {
    client.dispose()
  }
  if ('busy' in answer) return answer
  if (!store.acceptSpaces(listOf(answer), origin) && !sameLink(origin)) throw new ErroFrase(MOVED)
  return answer
}

/** D543 E1: aparência/nome do Berga do Space pela intenção do editor de bot, com a origem conferida antes de cada chamada. */
async function botEditorCall<T>(origin: SpacesOrigin, spaceId: string, act: (client: SpaceClient) => Promise<T>): Promise<T> {
  const { store } = origin
  const now = { hostId: store.hello?.hostId ?? null, client: store.connectedClient, epoch: store.principalEpoch, revision: store.spaces?.revision ?? null }
  const refusal = originRefusal(now, { hostId: origin.hostId, client: origin.client, epoch: origin.epoch, revision: origin.snapshot.revision })
  if (refusal) throw new ErroFrase(refusal)
  const client = new SpaceClient(origin.hostId, spaceId, origin.client)
  try { return await act(client) } finally { client.dispose() }
}

export function readSpaceBotEditor(origin: SpacesOrigin, spaceId: string, botId: string): Promise<BotEditor> {
  return botEditorCall(origin, spaceId, (client) => client.botEditor(botId))
}

export function saveSpaceBotEditor(origin: SpacesOrigin, spaceId: string, botId: string, expectedVersion: string, patch: BotEditorPatch): Promise<BotEditor> {
  return botEditorCall(origin, spaceId, (client) => client.botEditorSave({ botId, expectedVersion, patch }))
}

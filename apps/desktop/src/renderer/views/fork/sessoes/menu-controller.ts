// D545 R2: o hook e os testes usam a mesma captura, sem promover cache a contexto de escrita.
import type { Bot } from '../../../../shared/models'
import type { BotStore } from '../../../store/bot-store'
import { sessionCopyCurrent, sessionMenuCurrent, sessionMenuRow, type SessionMenuScope } from './menu'

export type SessionMenuOrigin = SessionMenuScope & { id: string; title: string; revision: number | null }

/** Sem `spaceId` no constructor: o destino vem do hello do link atual, nunca do mirror. */
export function captureSessionMenuScope(store: BotStore, chatId: string): SessionMenuScope {
  const hello = store.sessionMenuHello
  const snapshot = hello?.spaces
  const space = hello?.spaceId
  const known = hello?.spacesContract === 1 && snapshot?.version === 1 && typeof space === 'string' && !!space.trim()
    && Array.isArray(snapshot.spaces) && snapshot.spaces.some((s) => s.id === space)
    && (!store.spaceId || store.spaceId === space)
  return { store, chatId, client: store.connectedClient, epoch: store.sessions.epoch, localSpaceId: store.spaceId,
    spaceId: known ? space : undefined, hubId: known ? hello?.hostId : undefined }
}

/** `read` é vivo: respostas e confirmações não usam o store capturado por uma renderização antiga. */
export function createSessionMenuController(read: () => { store: BotStore; chat: Bot | null }) {
  const scope = () => { const { store, chat } = read(); return captureSessionMenuScope(store, chat?.id ?? '') }
  const current = (origin: SessionMenuOrigin | null, mutation = false) =>
    mutation ? sessionMenuCurrent(origin, scope()) : sessionCopyCurrent(origin, scope())
  const row = (id: string) => { const { store, chat } = read(); return sessionMenuRow(store.sessions.of(chat?.id ?? ''), chat, id) }
  const canArchive = (origin: SessionMenuOrigin | null) => {
    if (!origin || !current(origin, true)) return false
    const { store, chat } = read(), target = row(origin.id)
    const personal = store.sessionMenuHello?.spaces?.personalSpaceId
    return !!chat && chat.kind !== 'group' && origin.spaceId === personal && !!target && !target.principal
      && target.state === 'open' && store.sessions.can(chat.id, 'close') && !store.sessions.controlBusy(target.id)
  }
  const canRemove = (origin: SessionMenuOrigin | null) => {
    if (!origin || !current(origin, true) || !origin.revision || !Number.isSafeInteger(origin.revision)) return false
    const { store, chat } = read(), target = row(origin.id)
    return !!chat && chat.kind !== 'group' && origin.spaceId === store.sessionMenuHello?.spaces?.personalSpaceId
      && !!target && !target.principal && store.sessions.can(chat.id, 'delete') && !store.sessions.controlBusy(target.id)
  }
  return {
    current, row, canArchive, canRemove,
    capture(id: string, title: string): SessionMenuOrigin | null {
      const target = row(id)
      return target ? { ...scope(), id, title, revision: 'revision' in target ? target.revision : null } : null
    },
    async copy(origin: SessionMenuOrigin, name: boolean, write: (text: string) => Promise<void>) {
      if (!current(origin) || !row(origin.id)) return null
      try {
        await write(name ? origin.title : origin.id)
        return current(origin) ? 'copied' as const : null
      } catch { return current(origin) ? 'copyFailed' as const : null }
    },
    async remove(origin: SessionMenuOrigin) {
      if (!canRemove(origin)) return null
      const { store } = read()
      const result = await store.sessions.remove(origin.chatId, origin.id, origin.spaceId!, origin.revision!, {
        hubId: origin.hubId!, client: origin.client!, epoch: origin.epoch,
      })
      return current(origin, true) ? result : null
    },
    async archive(origin: SessionMenuOrigin) {
      if (!canArchive(origin)) return null
      const { store } = read()
      const result = await store.sessions.control(origin.chatId, origin.id, 'close', {
        hubId: origin.hubId!, spaceId: origin.spaceId!, client: origin.client!, epoch: origin.epoch,
      })
      return current(origin, true) ? result : null
    },
  }
}

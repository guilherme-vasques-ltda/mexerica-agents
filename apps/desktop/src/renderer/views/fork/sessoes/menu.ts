import type { HubSession } from './estado'

/** A origem da intenção de UI; nunca contém autoridade nova nem dados persistidos. */
export interface SessionMenuScope {
  store: object
  chatId: string
  localSpaceId?: string
  hubId?: string
  spaceId: string | undefined
  client: object | null
  epoch: number
}

/** Copiar é local: a ausência de Spaces não impede ler o ID/nome já exibido. */
export function sessionCopyCurrent(origin: SessionMenuScope | null, current: SessionMenuScope): boolean {
  return !!origin && origin.store === current.store && origin.chatId === current.chatId
    && origin.localSpaceId === current.localSpaceId && origin.client === current.client && origin.epoch === current.epoch
}

/** Mutação: além da identidade local, exige Hub e Space identificados no link atual. */
export function sessionMenuCurrent(origin: SessionMenuScope | null, current: SessionMenuScope): boolean {
  return sessionCopyCurrent(origin, current) && !!origin?.spaceId?.trim() && !!current.spaceId?.trim()
    && !!origin.hubId?.trim() && origin.hubId === current.hubId && origin.spaceId === current.spaceId
}

/** Evita que atalhos da aba interpretem a tecla de menu como outra ação. */
export function sessionMenuKey(e: { key: string; shiftKey: boolean; preventDefault: () => void }, open: () => void) {
  if (e.key !== 'ContextMenu' && !(e.shiftKey && e.key === 'F10')) return false
  e.preventDefault()
  open()
  return true
}


/** A principal conhecida pode ser copiada antes do catálogo; nunca inventa uma extra. */
export function sessionMenuRow(rows: readonly HubSession[] | null, chat: { id: string; name: string } | null, id: string) {
  return rows?.find((s) => s.id === id)
    ?? (chat && id === chat.id ? { id, principal: true, state: 'open' as const, title: chat.name } : null)
}

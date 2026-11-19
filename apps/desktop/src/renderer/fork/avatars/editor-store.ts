import { AvatarSaveQueue } from '../../../shared/fork-avatar-save.ts'
import type { BotAvatarConfig } from '../../../shared/fork-avatar-config.ts'
import type { BotStore } from '../../store/bot-store'

interface Editors { bots: Map<string, AvatarSaveQueue>; unsubscribe: () => void }
export type AvatarEditorStore = Pick<BotStore, 'bots' | 'subscribe' | 'connection' | 'saveAvatar'>
const editors = new WeakMap<AvatarEditorStore, Editors>()

/** Uma única fila por Hub/bot atende perfil, inspector e reabertura do editor. */
export function avatarEditorFor(store: AvatarEditorStore, id: string, initial: BotAvatarConfig): AvatarSaveQueue {
  let owned = editors.get(store)
  if (!owned) {
    const bots = new Map<string, AvatarSaveQueue>()
    owned = { bots, unsubscribe: store.subscribe(() => {
      for (const [id, queue] of bots) {
        if (!store.bots.has(id)) { queue.dispose(); bots.delete(id) }
        else queue.setOnline(store.connection.kind === 'online')
      }
    }) }
    editors.set(store, owned)
  }
  let queue = owned.bots.get(id)
  if (!queue) {
    queue = new AvatarSaveQueue(initial, (config) => store.saveAvatar(id, config), store.connection.kind === 'online')
    owned.bots.set(id, queue)
  }
  return queue
}

export function retireAvatarEditors(store: AvatarEditorStore) {
  const owned = editors.get(store)
  if (!owned) return
  owned.unsubscribe()
  for (const queue of owned.bots.values()) queue.dispose()
  editors.delete(store)
}

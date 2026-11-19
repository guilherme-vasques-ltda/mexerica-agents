import { setSpaceAppearance } from '../../../lib/appearance'
import { useEffect, useState } from 'react'
import { useModel } from '../../../lib/observable'
import type { BotStore } from '../../../store/bot-store'
import type { AppModel } from '../../../store/app-model'
import { SpaceContext, openSpaceAt } from '../../../store/fork-space-context'

/** E2: a loja selecionada troca o app; SpaceContext serve só ao histórico adicional legado. */
export function useSpaceView(app: AppModel, rail: BotStore | null) {
  const [spaceView] = useState(() => new SpaceContext())
  useModel(spaceView)
  const active = app.spaceMirrors.active
  const fallback = rail ?? (app.selection ? app.spaceMirrors.root(app.selection.computerId) : null) ?? (app.stores.size === 1 ? [...app.stores.values()][0]! : null)
  const spacesStore = active ? app.spaceMirrors.root(active.computerId) : fallback ? app.spaceMirrors.root(fallback.computer.id) : null
  const openSpaceRecord = active ? spacesStore?.spaces?.spaces.find(s => s.id === active.spaceId) ?? active.record : null
  useEffect(() => {
    setSpaceAppearance(openSpaceRecord?.appearanceConfigured ? openSpaceRecord.appearance : null)
    return () => setSpaceAppearance(null)
  }, [openSpaceRecord?.id, openSpaceRecord?.revision])
  const closeSpace = () => {
    const root = spacesStore
    app.spaceMirrors.activate(root?.computer.id ?? '')
    const id = root?.principal.id
    app.select(app.selection ?? (root && id ? { computerId: root.computer.id, botId: id } : null))
  }
  const openSpace = (store: BotStore, id: string, conversationId?: string) => {
    const root = app.spaceMirrors.root(store.computer.id) ?? store
    if (id === root.spaces?.personalSpaceId) return closeSpace()
    app.spaceMirrors.activate(root.computer.id, id)
    const principal = root.spaces?.spaces.find(s => s.id === id)?.principalBotId
    const native = conversationId && app.spaceMirrors.store(root.computer.id, id)?.bots.has(conversationId) ? conversationId : principal
    app.select(conversationId && native ? { computerId: root.computer.id, spaceId: id, botId: native } : app.selection ?? (native ? { computerId: root.computer.id, spaceId: id, botId: native } : null))
    if (conversationId && conversationId !== principal) spaceView.select(conversationId)
  }
  const origin = `${spacesStore?.hello?.hostId ?? ''}:${active?.computerId ?? ''}:${active?.spaceId ?? ''}`
  useEffect(() => {
    if (active && spacesStore && active.hostId === spacesStore.hello?.hostId) openSpaceAt(spaceView, spacesStore, active.spaceId)
    else spaceView.close()
  }, [origin, spaceView])
  useEffect(() => () => spaceView.close(), [spaceView])
  return { spaceView, spacesStore, openSpaceRecord, openSpace, closeSpace }
}

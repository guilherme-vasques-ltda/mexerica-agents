import { useEffect, useRef, useState } from 'react'
import { isGroup } from '@shared/models'
import { useModels } from '../../../lib/observable'
import { refKey, sameRef, type AppModel, type BotRef, type RosterItem } from '../../../store/app-model'
import type { BotStore } from '../../../store/bot-store'
import { defaultSelection, emptyWait, initialTrack, trackSelection, type DefaultInput, type DefaultSelection, type SelectionTrack } from './default-selection'

/** O `selectionSeq` da última escolha automática, por janela: sobrevive a uma nova montagem da barra. */
const autoSeqs = new WeakMap<AppModel, number>()

export interface StartupLists {
  /** Os bots visíveis recebidos dos computadores mostrados (`useComputerRoster(...).items`, de `store.roster`). */
  items: readonly RosterItem[]
  /**
   * A paleta da tela (`useRosterFilter(...).items`), que reinclui a principal oculta (D121). Só protege uma escolha da
   * pessoa fora da lista; nunca conta como bot visível.
   */
  palette: readonly RosterItem[]
  rows: readonly RosterItem[]
  hub: BotStore | null
  /** Os computadores mostrados na barra: confirmam juntos a lista vazia. */
  stores: readonly BotStore[]
  connecting: boolean
}

type StartupApp = Pick<AppModel, 'selection' | 'selectionSeq' | 'roster'>

/** O que a tela entrega à regra pura: a mesma montagem no gancho e no teste de integração. */
export function startupInput(app: StartupApp, lists: StartupLists, track: SelectionTrack, now: number): DefaultInput {
  const { items, palette, rows, hub, stores, connecting } = lists
  const row = (item: RosterItem) => ({ ref: item.ref, group: isGroup(item.bot) })
  const kind = hub?.connection.kind
  const exists = (ref: BotRef | null) => !!ref && (app.roster.some((item) => sameRef(item.ref, ref)) || palette.some((item) => sameRef(item.ref, ref)))
  return {
    selection: app.selection,
    items: items.map(row),
    rows: rows.map(row),
    hub: hub ? {
      computerId: hub.computer.id,
      ...(hub.spaceId ? { spaceId: hub.spaceId } : {}),
      principalId: hub.principal.protectedId,
      phase: hub.principal.phase,
      reachable: !hub.mismatch && (kind === 'online' || kind === 'connecting'),
    } : null,
    connecting,
    settled: emptyWait(stores, now) === 0,
    manual: track.manual,
    pinned: track.outside && exists(app.selection),
  }
}

/** Onde a seleção está agora, para `trackSelection`/`initialTrack`. */
export function selectionNow(app: StartupApp, items: readonly RosterItem[]) {
  return {
    seq: app.selectionSeq,
    key: app.selection ? refKey(app.selection) : null,
    listed: !!app.selection && items.some((item) => sameRef(item.ref, app.selection)),
  }
}

/**
 * Abre o Berga quando não há conversa válida (default-selection.ts). Espera enquanto um space, o editor, a criação,
 * Rotinas ou a paleta estão abertos: a escolha da pessoa nunca é trocada, e mudar a seleção fecharia o space.
 */
export function useDefaultSelection(app: AppModel, lists: StartupLists & { paused: boolean }): DefaultSelection {
  const { items, stores, paused } = lists
  // MX-4 (C9): a loja inteira a tela já assina pela fatia (conexão, bots, hello); aqui sobra o
  // sub-modelo principal, que não propaga para o `changed()` dela.
  useModels(stores.map((store) => store.principal))
  // Os 5 s da lista vazia passam sem evento: um render no fim deles reavalia.
  const [, setClock] = useState(0)
  const now = Date.now()
  const at = { ...selectionNow(app, items), autoSeq: autoSeqs.get(app) ?? -1 }
  // A seleção anterior à primeira montagem (uma notificação antes da barra) nasce manual e, fora da lista, fixada.
  const track = useRef<SelectionTrack | null>(null)
  track.current = track.current ? trackSelection(track.current, at) : initialTrack(at)
  const decision = defaultSelection(startupInput(app, lists, track.current, now))
  const target = decision.kind === 'select' ? decision.ref : null
  const wait = emptyWait(stores, now)
  const waitingEmpty = !items.length && wait !== null && wait > 0
  useEffect(() => {
    if (!waitingEmpty) return
    const timer = setTimeout(() => setClock((n) => n + 1), wait! + 50)
    return () => clearTimeout(timer)
  }, [waitingEmpty, wait])
  useEffect(() => {
    if (paused || !target || sameRef(target, app.selection)) return
    autoSeqs.set(app, app.selectionSeq + 1)
    app.select(target)
  }, [paused, target?.computerId, target?.botId, at.key])
  return decision
}

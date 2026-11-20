// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
import { createContext, useContext } from 'react'
import { listEqual, useModel, useSelector } from '../lib/observable'
import type { AppModel } from './app-model'
import type { BotStore } from './bot-store'
import type { Entry } from '@shared/models'

export const AppContext = createContext<AppModel | null>(null)

/** The window's model, re-rendering on its changes. */
function useRequiredApp(): AppModel {
  const app = useContext(AppContext)
  if (!app) throw new Error('AppContext missing')
  return app
}

export function useApp(): AppModel {
  const app = useRequiredApp()
  return useModel(app)
}

/**
 * O app para a moldura da janela (MX-4 C9). `useApp` renderiza a cada `changed()` — e o
 * SpaceMirrors propaga todo evento de loja para o app. Aqui a assinatura é uma fatia: host,
 * computadores, space ativo, seleção, aprovações e a lista de lojas. Conteúdo de loja (bots,
 * conexões) entra pela assinatura por loja da própria tela.
 */
export function useAppShell(): AppModel {
  const app = useRequiredApp()
  useSelector(app, appShellSig, listEqual)
  return app
}

function appShellSig(app: AppModel): readonly unknown[] {
  const approval = app.currentApproval
  return [
    app.host, app.computers, app.spaceMirrors.active, app.selection, app.selectionSeq,
    app.lastError, app.errorMessage, app.confirmation, app.screenNeedsApproval, app.screenError,
    app.approvals.length, approval?.id ?? null, approval?.request ?? null,
    ...app.stores.values(),
  ]
}

/** The store of the computer a view works on (kit's `@Environment(BotStore.self)`). */
export const StoreContext = createContext<BotStore | null>(null)

export function useStore(): BotStore {
  const store = useContext(StoreContext)
  if (!store) throw new Error('StoreContext missing')
  return useModel(store)
}

/** The store without following its changes: for rows that get their data as props. */
export function useStoreRef(): BotStore {
  const store = useContext(StoreContext)
  if (!store) throw new Error('StoreContext missing')
  return store
}

const NO_ENTRIES: Entry[] = []

/**
 * A lista da conversa para as assinaturas por fatia (MX-4 C9): `entries.get` é estável (a loja
 * troca a lista inteira a cada evento); num fake mínimo de teste, sem o mapa, cai em `allEntries`.
 */
export function chatEntriesOf(store: BotStore, chatId: string): Entry[] {
  const list = store.entries?.get(chatId)
  return list ?? (store.entries ? NO_ENTRIES : store.allEntries(chatId))
}

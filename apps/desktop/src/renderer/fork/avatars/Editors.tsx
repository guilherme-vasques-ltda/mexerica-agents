import { useEffect, useMemo, useSyncExternalStore } from 'react'
import { avatarDoBot, avatarMexeriquinhaHubNotice, normalizarAvatarDoBot } from '@shared/fork-avatar-config'
import type { BotDraft } from '@shared/models'
import type { BotStore } from '../../store/bot-store'
import { SeletorMexeriquinha } from '../../views/fork/mexeriquinha/SeletorMexeriquinha'
import { comAvatar } from '../../views/fork/mexeriquinha/seletor'
import { avatarEditorFor } from './editor-store'
import './avatars.css'

/**
 * O componente observa a fila compartilhada; desmontar não cria outra ordem de escrita.
 * Fase 2a: as Mexeriquinhas (dois estilos) no lugar do editor de robô; um Hub sem o formato v2 só mostra o aviso.
 */
export function ProfileAvatarEditor({ store, draft, onChange }: {
  store: BotStore; draft: BotDraft; onChange: (update: (d: BotDraft) => BotDraft) => void
}) {
  const notice = avatarMexeriquinhaHubNotice(store.hello)
  if (notice) return <div role="status">{notice}</div>
  return <SupportedProfileAvatarEditor store={store} draft={draft} onChange={onChange} />
}

function SupportedProfileAvatarEditor({ store, draft, onChange }: {
  store: BotStore; draft: BotDraft; onChange: (update: (d: BotDraft) => BotDraft) => void
}) {
  const atual = () => avatarDoBot(draft) ?? normalizarAvatarDoBot(undefined, draft.name, draft.description)
  const queue = useMemo(() => avatarEditorFor(store, draft.id!, atual()), [store, draft.id])
  const state = useSyncExternalStore(queue.subscribe, queue.getSnapshot)
  const saved = JSON.stringify([draft.avatarConfig, draft.mexeriquinha])
  useEffect(() => { queue.receive(atual()) }, [queue, saved])
  useEffect(() => () => { void queue.flush(true) }, [queue])
  const online = store.connection.kind === 'online'
  const status = state.error ?? (state.saving ? 'Salvando…' : state.dirty ? online ? 'Alteração pendente…' : 'Sem conexão. O avatar será salvo ao reconectar.' : 'Avatar salvo')
  return <div className="donnabot-avatar-profile">
    <SeletorMexeriquinha value={normalizarAvatarDoBot(state.value)} previa onChange={(avatarConfig) => {
      queue.change(avatarConfig)
      onChange((d) => comAvatar(d, avatarConfig))
    }} />
    <div role="status" className="donnabot-avatar-save">{status}</div>
    {state.error ? <button type="button" onClick={() => { void queue.retry() }}>Tentar salvar novamente</button> : null}
  </div>
}

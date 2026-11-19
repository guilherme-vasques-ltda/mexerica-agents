import type { NativeLink } from '../../../../shared/fork-native-catalog.ts'
import type { LibraryOp } from './biblioteca-session.ts'

// Biblioteca (BN-03): o que cada cartão e cada prévia oferecem, e para onde a navegação leva. Puro, sem React, para
// que a composição recibo → ação → navegação seja testada inteira. O destino de uma ação vem SEMPRE da mesma fonte que
// a mensagem mostrada: um recibo do Hub leva ao bot do recibo; o vínculo atual só aparece depois de “Ver estado atual”.

export type LibraryAction =
  /** Pessoal: o Bot por dentro normal. */
  | { kind: 'editor'; botId: string }
  /** Pessoal: a conversa do bot. Outro space: a conversa exata dele, no space. */
  | { kind: 'conversation'; botId: string }
  /** Cartão → prévia. */
  | { kind: 'show'; again: boolean }
  | { kind: 'add' }
  | { kind: 'readd' }
  /** Mesma intenção sem resposta (mesma chave e corpo). */
  | { kind: 'retry' }
  /** Sai do recibo histórico Removido para o estado atual do space; não abre nada. */
  | { kind: 'current' }

/**
 * As ações de um perfil, da secundária para a principal. `preview` distingue a prévia do cartão.
 * Prioridade: intenção sem resposta → recibo do Hub para este perfil → vínculo atual lido na prévia.
 */
export function libraryActions(catalogKey: string, s: { personal: boolean; link: NativeLink | null; op: LibraryOp; preview: boolean }): LibraryAction[] {
  const { personal, link, op, preview } = s
  if (op.kind === 'unknown' && op.intent.body.catalogKey === catalogKey) return [{ kind: 'retry' }]
  if (op.kind === 'done' && op.added.catalogKey === catalogKey) {
    // Recibo histórico: o bot dele foi removido. Nenhum destino, nem o sucessor que a releitura mostra.
    if (op.added.state === 'removed') return [{ kind: 'current' }]
    return opened(op.added.botId, personal, preview)
  }
  if (link?.state === 'active' && link.botId) return opened(link.botId, personal, preview)
  if (link?.state === 'removed') return preview ? [{ kind: 'readd' }] : [{ kind: 'show', again: true }]
  return preview ? [{ kind: 'add' }] : [{ kind: 'show', again: false }]
}

function opened(botId: string, personal: boolean, preview: boolean): LibraryAction[] {
  if (!personal) return [{ kind: 'conversation', botId }]
  return preview ? [{ kind: 'conversation', botId }, { kind: 'editor', botId }] : [{ kind: 'editor', botId }]
}

/** O recibo mostrado agora para este perfil, se for o histórico Removido. */
export const removedReceipt = (catalogKey: string, op: LibraryOp) =>
  op.kind === 'done' && op.added.catalogKey === catalogKey && op.added.state === 'removed' ? op.added : null

/** O que a navegação da janela oferece à folha (ChatWindow): sem editor fora do Pessoal (bot-dentro-13). */
export interface LibraryNavigationDeps {
  /** Fecha a folha. */
  close(): void
  /** Bot por dentro do bot no store Pessoal; false se ele ainda não está no app (nada é aberto). */
  openEditor(botId: string): boolean
  /** Conversa do bot no Pessoal. */
  selectPersonal(botId: string): void
  closeSpace(): void
  /** O space aberto exatamente nessa conversa (use-space-view → SpaceContext). */
  openSpace(spaceId: string, conversationId: string): void
}

export function libraryNavigation(target: { personal: boolean; spaceId: string }, deps: LibraryNavigationDeps) {
  return {
    onEditor(botId: string) {
      if (!target.personal) return
      if (deps.openEditor(botId)) deps.close()
    },
    onConversation(botId: string) {
      deps.close()
      if (target.personal) {
        deps.closeSpace()
        deps.selectPersonal(botId)
      } else deps.openSpace(target.spaceId, botId)
    },
  }
}

// MX-478: permissão pedida por um turno delegado, vista em Pontas soltas (Mission Board).
// Contrato mínimo `delegatedPermissions` do snapshot: só quem pediu a quem, o cartão de permissão original que
// espera o dono e quando. Nenhum texto do pedido, comando ou argumento. O app só mostra Abrir: aprovar continua
// no cartão original, e nada é retomado daqui. Campo ausente, malformado ou incoerente com o resto do snapshot vira
// indisponível ou é ignorado (fail-closed), nunca outro estado.
import type { ConversationRef, TowerBot, TowerPending } from './fork-torre.ts'

export type DelegatedMode = 'wait' | 'async'

export interface DelegatedPermissionItem {
  /** ID opaco do pedido, só para a key da lista. */
  key: string
  requesterBotId: string
  targetBotId: string
  /** O cartão de permissão original, o mesmo de `pending` do bot que recebeu o pedido. */
  entryId: string
  conversation: ConversationRef
  mode: DelegatedMode
  requestedAt: number
}

export interface DelegatedPermissions { items: DelegatedPermissionItem[]; omitted: number }

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const str = (v: unknown) => (typeof v === 'string' && v ? v : null)

function item(v: unknown): DelegatedPermissionItem | null {
  if (!isObj(v) || !isObj(v.conversation)) return null
  const key = str(v.key)
  const requesterBotId = str(v.requesterBotId)
  const targetBotId = str(v.targetBotId)
  const entryId = str(v.entryId)
  const conversationId = str(v.conversation.conversationId)
  const revision = v.conversation.revision
  const requestedAt = v.requestedAt
  if (!key || !requesterBotId || !targetBotId || !entryId || !conversationId) return null
  if (typeof revision !== 'number' || !Number.isFinite(revision)) return null
  if (typeof requestedAt !== 'number' || !Number.isFinite(requestedAt)) return null
  if (requesterBotId === targetBotId) return null
  if (v.mode !== 'wait' && v.mode !== 'async') return null
  return { key, requesterBotId, targetBotId, entryId, conversation: { conversationId, revision }, mode: v.mode, requestedAt }
}

/** Lista inválida vira indisponível; item desconhecido é ignorado. */
export function parseDelegatedPermissions(v: unknown): DelegatedPermissions | null {
  if (!isObj(v) || !Array.isArray(v.items)) return null
  const omitted = v.omitted
  if (typeof omitted !== 'number' || !Number.isInteger(omitted) || omitted < 0) return null
  return { items: v.items.map(item).filter((i): i is DelegatedPermissionItem => i !== null), omitted }
}

export interface DelegatedRow { item: DelegatedPermissionItem; from: TowerBot; to: TowerBot; card: TowerPending }

/**
 * Só vira linha o que a própria fotografia confirma: os dois bots estão nela e o cartão ainda está pendente, na
 * mesma conversa e revisão, como permissão pendente, entre as que o Hub autorizou para o bot que recebeu. Cartão já
 * respondido, de outra conversa, de outra revisão, de outro tipo ou de outro bot some. `revision` aqui é a revisão da
 * CONVERSA (`donna_space_conversations.revision`) nos dois lados, lida pelo Hub na mesma transação da fotografia; a
 * revisão do próprio cartão (`entries.rev`) nunca chega ao app e não entra na comparação. Nunca se abre um destino que o Hub não mostrou.
 */
export function delegatedRows(items: readonly DelegatedPermissionItem[], bots: readonly TowerBot[]): DelegatedRow[] {
  const byId = new Map(bots.map((b) => [b.botId, b]))
  return items.flatMap((it) => {
    const from = byId.get(it.requesterBotId)
    const to = byId.get(it.targetBotId)
    if (!from || !to || to.pending.availability !== 'available') return []
    const card = to.pending.value.find((p) => p.entryId === it.entryId && p.kind === 'permission' && p.status === 'pending'
      && p.conversation.conversationId === it.conversation.conversationId && p.conversation.revision === it.conversation.revision)
    return card ? [{ item: it, from, to, card }] : []
  })
}

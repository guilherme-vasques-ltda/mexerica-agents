// Abertura no Berga (docs/features/janela-do-mac.md#ao-abrir): quando a lista vazia pode virar a tela "Novo bot".
// Regra do coordenador (03/11/2026), igual para Hub com ou sem o contrato de spaces: depois de 5 s online sem nenhum
// bot visível chegar, ou antes, se o `hello` do stream disser que o space não tem bot nenhum. Cada bot vivo tem a sua
// conversa fixa com `id === botId` (host: `save_bot` → `register_primary`, o único caminho que cria bot), então um
// `hello` com `conversations` e nenhuma fixa é esse zero explícito. Sem `conversations` (Hub antigo), só os 5 s valem.

export const EMPTY_GRACE_MS = 5000

/** O que este módulo lê de uma conversa do `hello`. */
export interface AnnouncedConversation { id: string; botId: string; primary: boolean }

export class EmptyRosterWatch {
  /** Desde quando o computador está online neste link; null fora do ar. */
  onlineAt: number | null = null
  /** O último `hello` deste link disse que o space não tem bot nenhum. */
  zero = false

  connection(online: boolean, now: number) {
    this.onlineAt = online ? (this.onlineAt ?? now) : null
    if (!online) this.zero = false
  }

  hello(conversations: readonly AnnouncedConversation[] | undefined) {
    this.zero = Array.isArray(conversations) && !conversations.some((c) => c.primary && c.id === c.botId)
  }

  /** Quanto falta para a lista vazia valer como ausência de bots; null fora do ar. */
  remaining(now: number): number | null {
    if (this.onlineAt === null) return null
    return this.zero ? 0 : Math.max(0, this.onlineAt + EMPTY_GRACE_MS - now)
  }
}

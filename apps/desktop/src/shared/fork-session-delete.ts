/** D545: tombstone sem conteúdo. Metadados estritos antes de remover qualquer dado do mirror. */
export interface SessionDeleted {
  type: 'sessionDeleted'
  spaceId: string
  chatId: string
  sessionId: string
  rev: number
}
export function sessionDeletedOf(raw: unknown): SessionDeleted | null {
  const v = raw as Partial<SessionDeleted> | null
  return v?.type === 'sessionDeleted' && typeof v.spaceId === 'string' && !!v.spaceId
    && typeof v.chatId === 'string' && !!v.chatId && typeof v.sessionId === 'string' && !!v.sessionId
    && v.sessionId !== v.chatId && Number.isSafeInteger(v.rev) && (v.rev ?? 0) > 0 ? v as SessionDeleted : null
}

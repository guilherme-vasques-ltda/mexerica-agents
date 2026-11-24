export const geminiExtendedThinking = (model: string) => model === 'gemini-3.8-live-extended-thinking'

export function geminiInteractionState(extended: boolean, status: unknown, turnComplete: unknown) {
  if (extended) {
    if (status === 'IN_PROGRESS') return 'busy'
    if (status === 'IDLE') return 'idle'
    return
  }
  if (turnComplete === true) return 'idle'
}

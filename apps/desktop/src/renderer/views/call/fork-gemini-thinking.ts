const extendedThinking = 'gemini-3.8-live-extended-thinking'

export const geminiExtendedThinking = (model: string) => model === extendedThinking || model.startsWith(`${extendedThinking}-`)

export function geminiInteractionState(extended: boolean, status: unknown, turnComplete: unknown) {
  if (extended) {
    if (status === 'IN_PROGRESS') return 'busy'
    if (status === 'IDLE') return 'idle'
    return
  }
  if (turnComplete === true) return 'idle'
}

/** Bot-originated data is delivered only against a real, outstanding tool call. */
export class VoiceUpdates {
  private callId: string | null = null
  private messages: string[] = []

  push(text: string) { this.messages.push(text) }

  /** Rearm with the newest call, returning the superseded ID to close silently. */
  wait(id: string): string | null {
    const previous = this.callId
    this.callId = id
    return previous === id ? null : previous
  }

  get needsWait() { return this.messages.length > 0 && !this.callId }

  cancel(ids?: string[]) {
    if (!ids || (this.callId && ids.includes(this.callId))) this.callId = null
  }

  take() {
    if (!this.callId || !this.messages.length) return
    const result = { id: this.callId, output: JSON.stringify({ messages: this.messages }) }
    this.callId = null
    this.messages = []
    return result
  }
}

export const openAIToolOutput = (id: string, output: string) => ({
  type: 'conversation.item.create', item: { type: 'function_call_output', call_id: id, output },
})

export const geminiToolOutput = (id: string, name: string, output: string, scheduling?: 'SILENT') => ({
  id, name, response: { result: JSON.parse(output) as unknown, ...(scheduling ? { scheduling } : {}) },
})

/** Start generation from session instructions, with no fabricated user turn. */
export const openAIStartUpdates = () => ({
  type: 'response.create', response: { tool_choice: { type: 'function', name: 'bot_updates' }, metadata: { vozOrigem: 'ferramenta' } },
})

export const geminiStartUpdates = () => ({ clientContent: { turnComplete: true } })

import type { AccountId } from './fork-engine-accounts'

// Respostas transitórias protegidas: nunca persistir, enviar ao chat ou ao mirror.
export type SetupPhase = 'preparing' | 'awaiting_authorization' | 'verifying'
  | 'connected' | 'cancelled' | 'expired' | 'failed'

export interface SetupOperation {
  operationId: string
  engineAccountId: AccountId
  generation: number
  phase: SetupPhase
  revision: number
  expiresAt: number
  errorCode: string | null
  challenge: null | {
    kind: 'device_code' | 'paste_code' | 'link'
    url: string
    displayDomain: string
    code?: string
  }
  terminal: null | { available: boolean; cursor: number; output: string }
}

export interface StartSetup {
  engineAccountId: AccountId
  expectedRevision: number
  action: 'install' | 'login'
  clientRequestId: string
}

export type SetupAction = { operationId: string; generation: number } & (
  | { action: 'read'; afterCursor?: number }
  | { action: 'cancel' | 'open_terminal' }
  | { action: 'submit_code'; code: string }
  | { action: 'terminal_input'; data: string }
  | { action: 'terminal_resize'; cols: number; rows: number }
)

/** Contrato interno Main; não é um canal IPC nem uma prova de autoridade. */
export type LocalCloudIntent =
  | { kind: 'hello' }
  | { kind: 'cloudStatus' }
  | { kind: 'accessRequests' }
  | { kind: 'claimSign'; claimId: string; nonce: string; userId: string }
  | { kind: 'decideAccessRequest'; requestId: string; expect: { deviceKey: string; code: string; transcript: string } }

export interface CapturedSSHLocal {
  readonly profileId: string
  readonly computerId: string
  readonly signKey: string
  current(): boolean
  /** Only the owning capture can identify a failure proven to precede sending. */
  wasNotSent(error: unknown): boolean
  execute(intent: LocalCloudIntent): Promise<unknown>
}

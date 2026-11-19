/** E2: tabela única de capacidades; cada entrega futura altera uma linha. */
export const SPACE_CAPABILITIES = {
  chat: true, attachments: true, approvals: true, channels: true,
  teams: false, routines: false, sessions: false, connectors: false, vault: false, projects: false, skills: false,
} as const
export type SpaceCapability = keyof typeof SPACE_CAPABILITIES
export function spaceAllows(spaceId: string | null | undefined, capability: SpaceCapability) {
  return !spaceId || SPACE_CAPABILITIES[capability]
}

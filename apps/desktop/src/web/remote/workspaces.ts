export type HubWorkspaceGrant = { id: string; kind: 'personal' }
  | { id: string; kind: 'team'; organizationId: string }

export interface ClerkOrganization {
  id: string
  name: string
}

export type Workspace = { id: string; kind: 'personal' }
  | { id: string; kind: 'team'; organizationId: string; name: string }

export function filterAuthorizedWorkspaces(
  grants: readonly HubWorkspaceGrant[],
  organizations: readonly ClerkOrganization[],
): Workspace[] {
  const byId = new Map<string, HubWorkspaceGrant>()
  const byOrganization = new Map<string, string>()
  let personal: Workspace | undefined
  const teams: Workspace[] = []
  const names = new Map(organizations.map(({ id, name }) => [id, name]))
  for (const grant of grants) {
    if (!grant.id?.trim() || (grant.kind !== 'personal' && grant.kind !== 'team')
      || (grant.kind === 'team' && !grant.organizationId?.trim())) throw new Error('invalid_workspace_binding')
    const previous = byId.get(grant.id)
    if (previous) {
      if (previous.kind !== grant.kind
        || (previous.kind === 'team' && grant.kind === 'team' && previous.organizationId !== grant.organizationId)) {
        throw new Error('ambiguous_workspace_binding')
      }
      continue
    }
    byId.set(grant.id, grant)
    if (grant.kind === 'personal') {
      if (personal) throw new Error('ambiguous_workspace_binding')
      personal = grant
      continue
    }
    if (byOrganization.has(grant.organizationId)) throw new Error('ambiguous_workspace_binding')
    byOrganization.set(grant.organizationId, grant.id)
    const name = names.get(grant.organizationId)
    if (name?.trim()) teams.push({ ...grant, name })
  }
  return personal ? [personal, ...teams] : teams
}

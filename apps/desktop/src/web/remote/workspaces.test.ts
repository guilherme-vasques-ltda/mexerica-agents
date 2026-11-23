import assert from 'node:assert/strict'
import test from 'node:test'
import { filterAuthorizedWorkspaces } from './workspaces.ts'
import type { ClerkOrganization, HubWorkspaceGrant, Workspace } from './workspaces.ts'

const orgs: ClerkOrganization[] = [
  { id: 'org-a', name: 'Equipe A' },
  { id: 'org-b', name: 'Equipe B' },
  { id: 'org-ungranted', name: 'Sem acesso' },
]

test('retorna apenas grants confirmados pelo Hub e preserva ordem, com Personal primeiro', () => {
  const grants: HubWorkspaceGrant[] = [
    { id: 'team-b', kind: 'team', organizationId: 'org-b' },
    { id: 'personal', kind: 'personal' },
    { id: 'team-a', kind: 'team', organizationId: 'org-a' },
  ]

  assert.deepEqual(filterAuthorizedWorkspaces(grants, orgs), [
    { id: 'personal', kind: 'personal' },
    { id: 'team-b', kind: 'team', organizationId: 'org-b', name: 'Equipe B' },
    { id: 'team-a', kind: 'team', organizationId: 'org-a', name: 'Equipe A' },
  ] satisfies Workspace[])
})

test('organizações sem grant não recebem acesso e Personal nunca é fabricado', () => {
  assert.deepEqual(filterAuthorizedWorkspaces([], orgs), [])
  assert.deepEqual(filterAuthorizedWorkspaces(
    [{ id: 'team-a', kind: 'team', organizationId: 'org-a' }], orgs,
  ), [{ id: 'team-a', kind: 'team', organizationId: 'org-a', name: 'Equipe A' }])
})

test('grant sem organização Clerk correspondente é omitido', () => {
  assert.deepEqual(filterAuthorizedWorkspaces([
    { id: 'team-missing', kind: 'team', organizationId: 'org-missing' },
  ], orgs), [])
})

test('deduplica entradas idênticas sem alterar a ordem', () => {
  const team: HubWorkspaceGrant = { id: 'team-a', kind: 'team', organizationId: 'org-a' }
  const personal: HubWorkspaceGrant = { id: 'personal', kind: 'personal' }

  assert.deepEqual(filterAuthorizedWorkspaces([team, team, personal, personal], orgs), [
    { id: 'personal', kind: 'personal' },
    { id: 'team-a', kind: 'team', organizationId: 'org-a', name: 'Equipe A' },
  ])
})

test('falha fechada para IDs conflitantes, Personal duplicado e mais de um grant por organização', () => {
  const conflicts: HubWorkspaceGrant[][] = [
    [
      { id: 'same', kind: 'personal' },
      { id: 'same', kind: 'team', organizationId: 'org-a' },
    ],
    [
      { id: 'team-a', kind: 'team', organizationId: 'org-a' },
      { id: 'team-b', kind: 'team', organizationId: 'org-a' },
    ],
    [
      { id: 'personal-a', kind: 'personal' },
      { id: 'personal-b', kind: 'personal' },
    ],
  ]

  for (const grants of conflicts) assert.throws(() => filterAuthorizedWorkspaces(grants, orgs))
})

test('falha fechada para grants malformados em runtime', () => {
  const malformed: unknown[] = [
    { id: '', kind: 'personal' },
    { id: 'team', kind: 'team', organizationId: '  ' },
    { id: 'unknown', kind: 'other' },
  ]

  for (const grant of malformed) {
    assert.throws(() => filterAuthorizedWorkspaces([grant as HubWorkspaceGrant], orgs))
  }
})

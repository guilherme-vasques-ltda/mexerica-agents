package com.codync.android.core.fork

import com.codync.android.core.WireJson
import org.junit.Assert.*
import org.junit.Test

class ForkWorkspaceContextTest {
    private val team = TeamMembership("org_a", "Team A", "org:member")

    @Test fun personalOnly() {
        val choices = ForkWorkspaceContext.choices("user_a", true, emptyList())
        assertEquals("personal:user_a", choices.single().workspaceId)
    }

    @Test fun personalBeforeTeams() {
        val choices = ForkWorkspaceContext.choices("user_a", true, listOf(team))
        assertEquals(listOf("personal:user_a", "org:org_a"), choices.map { it.workspaceId })
    }

    @Test fun teamOnlyAndUnknownRolesAreDenied() {
        val choices = ForkWorkspaceContext.choices("user_a", false, listOf(team, team.copy(orgId = "org_b", role = "unknown")))
        assertEquals(WorkspaceKind.Team, choices.single().kind)
        assertEquals("org:org_a", choices.single().workspaceId)
        assertTrue(ForkWorkspaceContext.choices("user_a", false, listOf(team, team)).isEmpty())
    }

    @Test fun scopeIncludesAccountWorkspaceHubEpochAndSpace() {
        val choices = ForkWorkspaceContext.choices("user_a", true, listOf(team))
        val choice = choices.last()
        assertNotEquals(choice.contextScopeId("user_a"), choice.contextScopeId("user_b"))
        assertNotEquals(choice.contextScopeId("user_a"), choices.first().contextScopeId("user_a"))
        assertNotEquals(choice.contextScopeId("user_a"), choice.copy(role = WorkspaceRole.Admin).contextScopeId("user_a"))
        assertEquals(choice.contextScopeId("user_a"), choice.copy(name = "Renamed").contextScopeId("user_a"))
        val original = WorkspaceContext(1, "hub_a", WorkspaceKind.Team, choice.workspaceId, "user_a", WorkspaceRole.Member, 7, listOf("chat"))
        val scope = original.contextScopeId("space_a")
        assertNotEquals(scope, original.copy(hubId = "hub_b").contextScopeId("space_a"))
        assertNotEquals(scope, original.copy(workspaceEpoch = 8).contextScopeId("space_a"))
        assertNotEquals(scope, original.contextScopeId("space_b"))
        val payload = WireJson.encodeToString(original)
        assertEquals(original, WireJson.decodeFromString<WorkspaceContext>(payload))
        assertTrue(runCatching { WireJson.decodeFromString<WorkspaceContext>(payload.replace("\"member\"", "\"other\"")) }.isFailure)
    }

    @Test fun delayedEventsFailAfterSwitchAndRevocation() {
        val guard = WorkspaceGeneration()
        val beforeSwitch = guard.capture()
        guard.retire()
        assertFalse(guard.accepts(beforeSwitch))
        val beforeRevocation = guard.capture()
        assertTrue(guard.accepts(beforeRevocation))
        assertTrue(WorkspaceFailure.WorkspaceRevoked.invalidatesScope)
        guard.retire()
        assertFalse(guard.accepts(beforeRevocation))
    }

    @Test fun offlineDoesNotConfirmAnEmptyConversation() {
        assertTrue(RemoteHubState.Available.canConfirmEmpty)
        assertFalse(RemoteHubState.Offline.canConfirmEmpty)
        assertFalse(RemoteHubState.Connecting.canConfirmEmpty)
        assertNull(WorkspaceFailure.fromCode("hub_offline"))
    }

    @Test fun rejectAndRequestErrorKeepDistinctSemantics() {
        assertEquals(WorkspaceFailure.DeviceUnpaired, WorkspaceFailure.channelReject("device_unpaired", "unauthorized", 4001))
        assertEquals(WorkspaceFailure.DeviceRevoked, WorkspaceFailure.channelReject("device_revoked", "revoked", 4003))
        assertEquals(WorkspaceFailure.WorkspaceRevoked, WorkspaceFailure.channelReject("workspace_revoked", "revoked", 4003))
        assertEquals(WorkspaceFailure.RoleChanged, WorkspaceFailure.channelReject("role_changed", "revoked", 4003))
        assertEquals(WorkspaceFailure.LeaseExpired, WorkspaceFailure.channelReject("lease_expired", "unauthorized", 4001))
        assertEquals(WorkspaceFailure.LeaseExpired, WorkspaceFailure.channelReject(null, "leaseExpired", 4001))
        assertFalse(WorkspaceFailure.LeaseExpired.invalidatesScope)
        assertTrue(WorkspaceFailure.requestError(409, "workspace_epoch_mismatch")!!.invalidatesScope)
        assertNull(WorkspaceFailure.requestError(403, "workspace_epoch_mismatch"))
        assertFalse(WorkspaceFailure.requestError(403, "cross_workspace_reference")!!.invalidatesScope)
        assertFalse(WorkspaceFailure.requestError(403, "role_insufficient")!!.invalidatesScope)
    }
}

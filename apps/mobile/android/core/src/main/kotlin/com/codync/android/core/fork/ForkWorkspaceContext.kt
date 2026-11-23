package com.codync.android.core.fork

import java.security.MessageDigest
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
enum class WorkspaceKind { @SerialName("personal") Personal, @SerialName("team") Team }

@Serializable
enum class WorkspaceRole { @SerialName("owner") Owner, @SerialName("admin") Admin, @SerialName("member") Member }

@Serializable
data class WorkspaceContext(
    val schema: Int,
    val hubId: String,
    val kind: WorkspaceKind,
    val workspaceId: String,
    val userId: String,
    val role: WorkspaceRole,
    val workspaceEpoch: Long,
    val capabilities: List<String>,
) {
    init {
        require(schema == 1 && hubId.isNotBlank() && userId.isNotBlank() && workspaceEpoch >= 0)
        require(when (kind) {
            WorkspaceKind.Personal -> workspaceId == "personal:$userId" && role == WorkspaceRole.Owner
            WorkspaceKind.Team -> workspaceId.startsWith("org:") && workspaceId.removePrefix("org:").isNotBlank()
        })
    }

    fun contextScopeId(spaceId: String): String {
        require(spaceId.isNotBlank())
        return scopeId("hub-v1", hubId, workspaceId, userId, workspaceEpoch.toString(), spaceId)
    }
}

data class WorkspaceChoice(val workspaceId: String, val name: String, val kind: WorkspaceKind, val role: WorkspaceRole) {
    fun contextScopeId(userId: String): String = ForkWorkspaceContext.accountScopePrefix(userId) + scopeId(workspaceId, role.name)
}

data class TeamMembership(val orgId: String, val name: String, val role: String)

object ForkWorkspaceContext {
    fun accountScopePrefix(userId: String): String = "workspace-${scopeId(userId)}-"

    fun choices(userId: String, hasPersonal: Boolean, teams: List<TeamMembership>): List<WorkspaceChoice> = buildList {
        require(userId.isNotBlank())
        if (hasPersonal) add(WorkspaceChoice("personal:$userId", "Pessoal", WorkspaceKind.Personal, WorkspaceRole.Owner))
        addAll(teams.groupBy(TeamMembership::orgId).values.mapNotNull { memberships ->
            val team = memberships.singleOrNull()?.takeIf { it.orgId.isNotBlank() && it.name.isNotBlank() } ?: return@mapNotNull null
            val role = when (team.role) {
                "org:admin" -> WorkspaceRole.Admin
                "org:member" -> WorkspaceRole.Member
                else -> return@mapNotNull null
            }
            WorkspaceChoice("org:${team.orgId}", team.name, WorkspaceKind.Team, role)
        }.sortedBy(WorkspaceChoice::workspaceId))
    }
}

class WorkspaceGeneration {
    @Volatile
    private var generation = 0L
    fun capture(): Long = generation
    fun retire() { generation = Math.incrementExact(generation) }
    fun accepts(captured: Long): Boolean = captured == generation
}

enum class WorkspaceFailure(val code: String, val invalidatesScope: Boolean = false) {
    AuthInvalid("auth_invalid"), DeviceUnpaired("device_unpaired", true), DeviceRevoked("device_revoked", true),
    LeaseExpired("lease_expired"),
    WorkspaceForbidden("workspace_forbidden", true), RoleInsufficient("role_insufficient"),
    CrossWorkspaceReference("cross_workspace_reference"), EpochMismatch("workspace_epoch_mismatch", true),
    WorkspaceRevoked("workspace_revoked", true), RoleChanged("role_changed", true),
    HubOffline("hub_offline");

    companion object {
        fun fromCode(code: String): WorkspaceFailure? = entries.firstOrNull { it.code == code && it != HubOffline }

        fun channelReject(stableCode: String?, legacyCode: String?, closeCode: Int): WorkspaceFailure? {
            if (closeCode !in setOf(4001, 4003)) return null
            if (stableCode != null) return fromCode(stableCode)?.takeIf {
                it in setOf(DeviceRevoked, DeviceUnpaired, WorkspaceRevoked, RoleChanged, LeaseExpired, AuthInvalid)
            }
            return when (legacyCode) {
                "leaseExpired" -> LeaseExpired
                "revoked" -> DeviceRevoked
                "unauthorized", "badSignature" -> DeviceUnpaired
                else -> null
            }
        }

        fun requestError(status: Int, code: String): WorkspaceFailure? =
            if (code == EpochMismatch.code && status != 409) null else fromCode(code)
    }
}

enum class RemoteHubState {
    Unpaired, Connecting, Available, Offline, Denied;

    val canConfirmEmpty: Boolean get() = this == Available
}

private fun scopeId(vararg fields: String): String = MessageDigest.getInstance("SHA-256")
    .digest(fields.joinToString("") { "${it.length}:$it" }.toByteArray(Charsets.UTF_8))
    .joinToString("") { "%02x".format(it) }

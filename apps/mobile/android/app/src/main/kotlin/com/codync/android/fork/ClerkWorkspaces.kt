package com.codync.android.fork

import com.clerk.api.Clerk
import com.clerk.api.network.serialization.ClerkResult
import com.clerk.api.session.Session
import com.clerk.api.user.getOrganizationMemberships
import com.codync.android.core.fork.ForkWorkspaceContext
import com.codync.android.core.fork.TeamMembership
import com.codync.android.core.fork.WorkspaceChoice
import kotlinx.serialization.json.JsonPrimitive

internal suspend fun clerkWorkspaces(session: Session): List<WorkspaceChoice>? {
    val user = session.user ?: return null
    fun sameSession() = Clerk.activeSession?.let {
        it.id == session.id && it.user?.id == user.id && it.expireAt > System.currentTimeMillis()
    } == true
    if (!sameSession()) return null
    val hasPersonal = !Clerk.organizationSelectionIsForced && if (Clerk.organizationIsEnabled) {
        user.publicMetadata?.get("mexericaPersonalWorkspace") == JsonPrimitive(true)
    } else true
    val teams = mutableListOf<TeamMembership>()
    if (Clerk.organizationIsEnabled) {
        do {
            if (!sameSession() || teams.size >= 1000) return null
            val result = user.getOrganizationMemberships(limit = 50, offset = teams.size)
            if (result !is ClerkResult.Success) return null
            val page = result.value
            if (page.totalCount < 0 || page.data.isEmpty() && teams.size < page.totalCount) return null
            teams += page.data.map { TeamMembership(it.organization.id, it.organization.name, it.role) }
        } while (teams.size < page.totalCount)
    }
    return ForkWorkspaceContext.choices(user.id, hasPersonal, teams).takeIf { sameSession() }
}

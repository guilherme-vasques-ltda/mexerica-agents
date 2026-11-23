// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
package com.codync.android

import android.app.Activity
import com.clerk.api.Clerk
import com.clerk.api.ClerkConfigurationOptions
import com.clerk.api.hostedauth.HostedAuthCancellationException
import com.clerk.api.network.serialization.ClerkResult
import com.clerk.api.network.serialization.errorMessage
import com.clerk.api.session.Session
import com.clerk.api.sso.OAuthProvider
import com.clerk.api.sso.SSOCancellationException
import com.codync.android.core.fork.AccountDeployment
import com.codync.android.core.fork.WorkspaceChoice
import com.codync.android.core.fork.WorkspaceKind
import com.codync.android.fork.clerkWorkspaces
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import java.io.IOException

/** A sign-in that ended without an account, and why. */
internal class SignInError(val reason: SignInFailure, message: String, cause: Throwable? = null) : IOException(message, cause)

data class SavedAccount(val userId: String, val sessionId: String, val email: String, val avatar: String?, val organizationId: String? = null)
data class AccountState(val ready: Boolean = false, val configured: Boolean = NativeAccounts.configured,
    val userId: String? = null, val accounts: List<SavedAccount> = emptyList(),
    val multiple: Boolean = false, val error: String? = null, val workspaces: List<WorkspaceChoice>? = null)

/** Clerk owns browser authentication, encrypted credentials and session restoration. */
object NativeAccounts {
    internal val deployment = AccountDeployment.parse(BuildConfig.CLOUD_URL, BuildConfig.CLERK_PUBLISHABLE_KEY, BuildConfig.CLERK_TOKEN_ORIGIN)
    val configured = deployment != null

    fun initialize(activity: Activity) {
        val config = deployment ?: return
        Clerk.initialize(activity, config.publishableKey, ClerkConfigurationOptions(enableDebugMode = false, telemetryEnabled = false)
            .withCustomHeaders(mapOf("Origin" to config.tokenOrigin)))
    }

    fun observe(scope: CoroutineScope, update: suspend (AccountState) -> Unit) = scope.launch {
        if (!configured) { update(AccountState(ready = true)); return@launch }
        var previousUserId: String? = null
        combine(Clerk.isInitialized, Clerk.sessionFlow, Clerk.sessionsFlow,
            Clerk.multiSessionModeIsEnabledFlow, Clerk.initializationError) { ready, selected, sessions, multiple, error ->
            val accounts = sessions.filter { it.status == Session.SessionStatus.ACTIVE && it.expireAt > System.currentTimeMillis() }
                .mapNotNull { session -> session.user?.let { user -> SavedAccount(user.id, session.id,
                    user.primaryEmailAddress?.emailAddress ?: "Mexerica account", user.imageUrl.takeIf { it.startsWith("https://") },
                    session.lastActiveOrganizationId) } }
                .distinctBy(SavedAccount::userId)
            selected to AccountState(ready, true, selected?.usableUserId(), accounts, multiple,
                if (error != null && !ready) "Couldn't restore sign-in. Check your connection and retry." else null)
        }.collectLatest { (selected, account) ->
            if (account.userId != previousUserId && account.userId != null) update(account)
            previousUserId = account.userId
            if (!account.ready || selected == null || account.userId == null) {
                update(account)
            } else {
                while (isActive) {
                    val workspaces = try { withTimeoutOrNull(20_000) { clerkWorkspaces(selected) } }
                    catch (error: CancellationException) { throw error }
                    catch (_: Exception) { null }
                    update(account.copy(workspaces = workspaces,
                        error = if (workspaces == null) "Couldn't verify workspaces. Check your connection." else null))
                    delay(60_000)
                    if (activeUserId() != account.userId) {
                        update(account.copy(userId = null)); break
                    }
                }
            }
        }
    }

    /** The signed-in account right now; `observe` reports the same once its flows catch up. */
    fun activeUserId(): String? = if (configured) Clerk.activeSession?.usableUserId() else null

    private fun Session.usableUserId(): String? = takeIf { it.status == Session.SessionStatus.ACTIVE && it.expireAt > System.currentTimeMillis() }?.user?.id

    suspend fun signIn(apple: Boolean) {
        require(Clerk.isInitialized.value) { "Wait for sign-in to finish loading." }
        val result = Clerk.auth.signInWithOAuth(if (apple) OAuthProvider.APPLE else OAuthProvider.GOOGLE)
        when (result) {
            is ClerkResult.Failure -> throw signInError(result)
            is ClerkResult.Success -> {
                val sessionId = result.value.signIn?.createdSessionId ?: result.value.signUp?.createdSessionId
                    ?: throw SignInError(SignInFailure.Verification, "Sign-in needs additional verification. Continue using the other sign-in options.")
                val session = Clerk.auth.sessions.firstOrNull { it.id == sessionId }
                if (Clerk.activeSession?.id == sessionId) return
                val activated = Clerk.auth.setActive(sessionId, session?.lastActiveOrganizationId)
                requireSuccess(activated)
                if (Clerk.activeSession?.id != sessionId) throw SignInError(SignInFailure.Verification,
                    "Choose your organization using the hosted sign-in option.")
            }
        }
    }

    suspend fun hostedSignIn() {
        val result = Clerk.auth.startHostedAuth()
        if (result is ClerkResult.Failure) throw signInError(result)
    }

    private fun signInError(failure: ClerkResult.Failure<com.clerk.api.network.model.error.ClerkErrorResponse>): SignInError {
        val cause = failure.throwable
        val reason = when {
            cause is SSOCancellationException || cause is HostedAuthCancellationException -> SignInFailure.Cancelled
            cause?.isTimeout == true -> SignInFailure.Timeout
            else -> SignInFailure.Error
        }
        return SignInError(reason, failure.errorMessage, cause)
    }

    suspend fun switch(account: SavedAccount) {
        val result = Clerk.auth.setActive(account.sessionId, account.organizationId)
        requireSuccess(result)
        require(Clerk.activeSession?.id == account.sessionId) { "This account needs an active organization. Sign in again." }
    }

    internal suspend fun selectPersonal(userId: String) {
        val session = requireNotNull(Clerk.activeSession)
        require(session.usableUserId() == userId && !Clerk.organizationSelectionIsForced)
        if (session.lastActiveOrganizationId == null) return
        requireSuccess(Clerk.auth.setActive(session.id, organizationId = null))
        require(Clerk.activeSession?.let { it.id == session.id && it.lastActiveOrganizationId == null } == true)
    }

    suspend fun signOut(sessionId: String) {
        // An explicit session keeps local credentials intact if the server rejects logout.
        val result = Clerk.auth.signOut(sessionId)
        requireSuccess(result)
    }

    suspend fun reset() {
        if (!configured) return
        // The all-session API clears the SDK's local credentials even when the network is down.
        val result = Clerk.auth.signOut()
        if (result is ClerkResult.Failure) {
            val error = result.throwable
            if (error is CancellationException) throw error
        }
    }

    fun retry() { if (configured) Clerk.reinitialize() }

    suspend fun token(expectedUser: String): String {
        val session = requireNotNull(Clerk.activeSession) { "Sign in again to reach account computers." }
        require(session.usableUserId() == expectedUser && session.lastActiveOrganizationId == null) { "Select the personal Clerk session to reach account computers." }
        val result = Clerk.auth.getToken()
        require(Clerk.activeSession?.let { it.id == session.id && it.usableUserId() == expectedUser && it.lastActiveOrganizationId == null } == true) { "The active account changed." }
        return when (result) {
            is ClerkResult.Success -> result.value
            is ClerkResult.Failure -> throw IOException("Couldn't refresh sign-in. Check your connection and try again.")
        }
    }

    private fun requireSuccess(result: ClerkResult<*, com.clerk.api.network.model.error.ClerkErrorResponse>) {
        if (result is ClerkResult.Failure) throw IOException(result.errorMessage)
    }
}

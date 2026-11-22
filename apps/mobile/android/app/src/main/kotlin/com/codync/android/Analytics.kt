// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
package com.codync.android

import android.content.Context
import android.os.Build
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.platform.UriHandler
import java.security.MessageDigest
import java.util.UUID
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/** Reports a screen, sheet or onboarding step each time it appears. */
@Composable internal fun AnalyticsOnAppear(event: AnalyticsEvent) = LaunchedEffect(event) { Analytics.capture(event) }

/** Opens an outbound link, counted by what it leads to (desktop `openLink`). */
internal fun UriHandler.open(url: String, link: OutboundLink) {
    openUri(url)
    Analytics.capture(AnalyticsEvent.LinkOpened(link))
}

/** Event catalog retained for compatibility; analytics and crash upload are disabled. */
@Suppress("UNUSED_PARAMETER")
internal object Analytics {
    val isEnabled: Boolean get() = false
    val enabled: Boolean get() = isEnabled
    var userId: () -> String? = { null }
    fun start(context: Context) = Unit
    fun setEnabled(on: Boolean) = Unit
    fun capture(event: AnalyticsEvent) = Unit
    fun record(name: String, extra: Map<String, JsonElement>) = Unit
    fun completeOnboarding(via: OnboardingVia) = Unit
    fun forgetOnboarding() = Unit
    fun becameActive(context: Context) = Unit
    fun enteredBackground() = Unit
    fun flush() = Unit
    fun sessionId(): String = ""

    /** One event in PostHog's `/batch/` body, with the properties every event carries. */
    internal fun event(name: String, distinctId: String, timestamp: String, extra: Map<String, JsonElement>): JsonObject = buildJsonObject {
        put("uuid", UUID.randomUUID().toString()); put("event", name); put("distinct_id", distinctId); put("timestamp", timestamp)
        put("properties", JsonObject(common + extra + ("\$process_person_profile" to JsonPrimitive(true))))
    }

    private val common: Map<String, JsonElement> by lazy {
        mapOf("\$lib" to JsonPrimitive("codync-android"), "app_version" to JsonPrimitive(BuildConfig.VERSION_NAME),
            // Codync Dev reports as dev, whatever the build type.
            "env" to JsonPrimitive(if (BuildConfig.APPLICATION_ID.endsWith(".dev")) "dev" else "main"),
            "\$os" to JsonPrimitive("Android"), "\$os_version" to JsonPrimitive(Build.VERSION.RELEASE.orEmpty()))
    }

    /** A computer's analytics key, the same hash its host sends: joins `computer_paired` with that computer's events. */
    fun computerTag(id: String): String =
        MessageDigest.getInstance("SHA-256").digest("codync-analytics:$id".toByteArray()).take(8).joinToString("") { "%02x".format(it) }
}

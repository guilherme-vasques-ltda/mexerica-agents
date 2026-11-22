// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
package com.codync.android.core

/**
 * Which side must update before this app and a host can work together
 * (docs/reference/compatibility.md): each names the oldest version of the other it works with.
 */
sealed interface VersionMismatch {
    /** This app is older than the host's `minApp`. */
    data class UpdateApp(val minimum: String) : VersionMismatch
    /** The host is older than `MIN_HOST`. */
    data class UpdateHost(val version: String, val minimum: String) : VersionMismatch

    companion object {
        /** The oldest host this app works with; raise it only when a core flow needs something older hosts lack. */
        const val MIN_HOST = "2.20.1"

        /** An unknown version is never a reason to lock someone out. */
        fun check(app: String, hostVersion: String?, minApp: String?, minHost: String = MIN_HOST): VersionMismatch? = when {
            hostVersion == null -> null
            minApp != null && AppVersion.isBelow(app, minApp) -> UpdateApp(minApp)
            AppVersion.isBelow(hostVersion, minHost) -> UpdateHost(hostVersion, minHost)
            else -> null
        }
    }
}

object AppVersion {
    /** Whether `version` is below `minimum` in release order; an unreadable version never is. */
    fun isBelow(version: String, minimum: String): Boolean {
        val have = parse(version)?.let(::rank) ?: return false
        val need = parse(minimum)?.let(::rank) ?: return false
        for (i in have.indices) {
            if (have[i] != need[i]) return have[i] < need[i]
        }
        return false
    }

    /**
     * Release order across lines: major 0 is the Mexerica line, which comes after every inherited
     * Codync version (2.x), so `0.1.0` is newer than `2.20.3`. Within a line the usual numeric
     * order applies. Leaving major 0 needs this rule revisited (mirrors `rank` in host/src/compat.rs).
     */
    private fun rank(v: List<Long>) = listOf(if (v[0] == 0L) 1L else 0L) + v

    /** Shared parse contract with iOS and the Hub (`host/src/compat.rs`):
     *  `major.minor.patch`, trimming spaces AND line breaks, ignoring ONE leading `v`
     *  (`vv2.18.0` stays unreadable here — the host strips every `v`; like the desktop we strip
     *  one, choosing the side that never blocks) and any pre-release or build suffix; 1–3
     *  ASCII-digit parts read as 64-bit (the host's u64 accepts a few values above Long.MAX_VALUE,
     *  which stay unreadable here — again never blocking); missing parts count as 0.
     *  `null` for anything else, and an unreadable version never blocks anyone. */
    private fun parse(version: String): List<Long>? {
        val core = version.trim().removePrefix("v").substringBefore('-').substringBefore('+')
        val parts = core.split('.')
        if (parts.size !in 1..3) return null
        val out = parts.map { part ->
            if (part.isEmpty() || !part.all { it in '0'..'9' }) return null
            part.toLongOrNull() ?: return null
        }
        return out + List(3 - out.size) { 0L }
    }
}

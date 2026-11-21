// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
package com.codync.android.core

import org.junit.Assert.*
import org.junit.Test

class CompatibilityTest {
    @Test fun `each side names the oldest version of the other`() {
        assertNull(VersionMismatch.check("2.17.5-dev", null, "9.0.0"))
        assertEquals(VersionMismatch.UpdateApp("2.18.0"), VersionMismatch.check("2.17.5-dev", "2.18.0", "2.18.0"))
        assertEquals(VersionMismatch.UpdateHost("2.20.0", "2.20.1"), VersionMismatch.check("2.20.1", "2.20.0", "2.3.0"))
        assertNull(VersionMismatch.check("2.20.1", "2.20.1", "2.3.0"))
        assertTrue(AppVersion.isBelow("2.9.3", "2.14.1"))
        assertFalse(AppVersion.isBelow("garbled", "2.14.1"))
    }

    @Test fun `shared version vectors match the host contract`() {
        // Shared vectors with CodyncKitTests.swift (iOS) — same cases, same expectations.
        val vectors = listOf(
            // The 0.x line (Mexerica) comes after every 2.x (inherited Codync).
            Triple("0.1.0", "2.20.1", false),
            Triple("0.1.0", "2.20.3", false),
            Triple("2.20.3", "0.1.0", true),
            Triple("0.1.0", "0.1.0", false),
            Triple("0.1.0", "0.2.0", true),
            Triple("0.9.0", "0.10.0", true),
            Triple("0.10.0", "0.9.0", false),
            // 1.x vs 2.x and the usual numeric order inside a line.
            Triple("1.9.9", "2.0.0", true),
            Triple("2.0.0", "1.9.9", false),
            Triple("2.9.9", "2.10.0", true),
            Triple("2.19.0", "2.20.1", true),
            Triple("2.4.0", "2.4.0", false),
            Triple("2.4.0-dev", "2.4.0", false),
            // Whitespace and line breaks are trimmed; one leading v drops.
            Triple(" v2.3.9 ", "2.4.0", true),
            Triple("\n2.18.0\n", "2.20.1", true),
            Triple("v2.3.9", "2.4.0", true),
            // Pre-release and build suffixes are ignored.
            Triple("2.4.0-beta.1+77", "2.4.1", true),
            Triple("2.4.0+77", "2.4.1", true),
            // Missing parts count as 0.
            Triple("2.4", "2.4.1", true),
            Triple("3", "2.20.1", false),
            // Unreadable versions never block, in either position.
            Triple("", "2.4.0", false),
            Triple("2.0.0", "garbage", false),
            Triple("garbled", "0.1.0", false),
            Triple("0.1.0", "garbled", false),
            Triple("2..0", "2.4.1", false),
            Triple("2.4.0.1", "2.4.1", false),
            Triple("+1.0.0", "2.0.0", false),
            Triple("-1.0.0", "2.0.0", false),
            Triple("２.4.0", "2.5.0", false),
            // Documented decision: `vv` stays unreadable on mobile (the host strips every `v`;
            // like the desktop we strip one — the side that never blocks).
            Triple("vv2.18.0", "2.20.1", false),
            // 64-bit components: beyond Int32 still parses; beyond Int64 stays unreadable.
            Triple("2.2147483648.0", "3.0.0", true),
            Triple("2.9223372036854775807.0", "3.0.0", true),
            Triple("2.9223372036854775808.0", "0.1.0", false),
            Triple("0.1.0", "2.9223372036854775808.0", false),
        )
        for ((version, minimum, expected) in vectors) {
            assertEquals(expected, AppVersion.isBelow(version, minimum))
        }
    }

    @Test fun `check vectors match the shared table`() {
        // Shared vectors with CodyncKitTests.swift (iOS) — same cases, same expectations.
        data class Row(val app: String, val host: String?, val minApp: String?, val expected: VersionMismatch?)
        val rows = listOf(
            // Same-release pair and mixed pairs within the floors connect; older 2.x is still refused.
            Row("0.1.0", "0.1.0", "2.20.1", null),
            Row("0.1.0", "2.20.3", "2.20.1", null),
            Row("2.20.3", "0.1.0", "2.20.1", null),
            Row("2.19.0", "0.1.0", "2.20.1", VersionMismatch.UpdateApp("2.20.1")),
            Row("0.1.0", "2.18.0", null, VersionMismatch.UpdateHost("2.18.0", "2.20.1")),
            // Line-break-padded versions are read (trimmed) and judged like any other.
            Row("\n2.19.0\n", "0.1.0", "2.20.1", VersionMismatch.UpdateApp("2.20.1")),
            // Unreadable versions never block: beyond Int64, `vv`, empty.
            Row("2.9223372036854775808.0", "0.1.0", "2.20.1", null),
            Row("vv2.19.0", "0.1.0", "2.20.1", null),
            Row("", "0.1.0", "2.20.1", null),
            Row("0.1.0", null, "2.20.1", null),
        )
        for (row in rows) {
            assertEquals(row.expected, VersionMismatch.check(row.app, row.host, row.minApp))
        }
    }
}

// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
package com.codync.android.core

import org.junit.Assert.*
import org.junit.Test

class UsageDestinationTest {
    private val query = "scope=local&computer=AAAAAAAAAAAAAAAAAAAAAA"
    @Test fun usageLinksRetainTheirAccountAndComputerScope() {
        val destination = UsageDestination.parse("donnabot://usage?$query", "donnabot")
        assertEquals("local", destination.scope)
        assertEquals("AAAAAAAAAAAAAAAAAAAAAA", destination.computerId)
    }
    @Test fun incompleteAmbiguousOrForeignLinksAreRejected() {
        for (value in listOf("donnabot://usage", "donnabot://usage/path?$query", "donnabot://usage?$query&scope=local",
            "donnabot://usage?$query#other", "https://usage?$query")) {
            assertTrue(value, runCatching { UsageDestination.parse(value, "donnabot") }.isFailure)
        }
    }
}

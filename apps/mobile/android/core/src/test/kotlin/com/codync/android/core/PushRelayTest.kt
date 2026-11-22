package com.codync.android.core

import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Test

class PushRelayTest {
    @Test fun `empty relay is rejected before a push client or request is created`() = runBlocking {
        for (relay in listOf("", "   ")) {
            val error = assertFails<IllegalArgumentException> {
                PushRelay.ticket(relay, "fixture-token-long-enough", "local", "fixture-computer")
            }
            assertEquals("Invalid push relay address", error.message)
        }
    }
}

package com.codync.android

import com.codync.android.core.Entry
import com.codync.android.core.MirrorEvent
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Test

class SessionTest {
    private fun message(id: String, bot: String = "bot", sessionId: String? = null, kind: String = "user") =
        MirrorEvent.EntryChanged(Entry(id, 1, bot, 1, kind, sessionId = sessionId, data = buildJsonObject { put("text", "hi") }))

    @Test fun `the main chat keeps its own and legacy entries and leaves extra sessions out`() {
        val entries = listOf(message("main"), message("legacy", sessionId = "bot"), message("extra", sessionId = "extra"),
            message("other-bot", bot = "other"), message("other-extra", bot = "other", sessionId = "extra"))
        assertEquals(listOf("main", "legacy"), entries.map { it.entry }.mainChat("bot").map { it.id })
        assertEquals(listOf("other-bot"), entries.map { it.entry }.mainChat("other").map { it.id })
    }

    @Test fun `history pages over the main session alone so extra sessions never mix in`() {
        val entries = listOf(message("main", sessionId = "bot"), message("extra", sessionId = "extra"))
        assertEquals(1L, entries.map { it.entry }.mainChat("bot").minOfOrNull(Entry::seq))
    }

    @Test fun `a voice call hears only its bot's main session and keeps other bots untouched`() {
        val call = "bot"
        val entries = listOf(message("main"), message("extra", sessionId = "extra"), message("other", bot = "other", sessionId = "extra"))
        assertEquals(listOf("main", "other"), entries.filter { it.entry.botId != call || it.entry.isMainSession }.map { it.entry.id })
    }
}

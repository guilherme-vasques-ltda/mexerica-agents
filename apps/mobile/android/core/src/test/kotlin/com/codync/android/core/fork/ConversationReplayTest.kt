package com.codync.android.core.fork

import com.codync.android.core.Entry
import com.codync.android.core.PendingSend
import com.codync.android.core.WireJson
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.*
import org.junit.Test

class ConversationReplayTest {
    private val send = PendingSend("nonce-a", "bot-a", "hello", 1)
    private val echo = Entry("entry-a", 1, "bot-a", 2, "user", data = buildJsonObject { put("clientNonce", send.nonce) })

    @Test fun onlyUserEchoInTheSameLaneConfirmsOptimisticSend() {
        assertTrue(ConversationReplay.confirms(send, echo))
        assertTrue(ConversationReplay.confirms(send, echo.copy(sessionId = send.botId)))
        assertFalse(ConversationReplay.confirms(send, echo.copy(sessionId = "session-b")))
        assertFalse(ConversationReplay.confirms(send, echo.copy(botId = "bot-b")))
        assertFalse(ConversationReplay.confirms(send, echo.copy(kind = "agent")))
        assertFalse(ConversationReplay.confirms(send.copy(nonce = "nonce-b"), echo))
        assertTrue(ConversationReplay.confirms(send.copy(sessionId = "session-b"), echo.copy(sessionId = "session-b")))
    }

    @Test fun replayUpdatesEachEntryOnlyAtAHigherRevisionInTheSameLane() {
        val reply = echo.copy(id = "reply-a", seq = 2, kind = "agent")
        assertFalse(ConversationReplay.replaces(reply, reply))
        assertFalse(ConversationReplay.replaces(reply, reply.copy(rev = 1)))
        assertFalse(ConversationReplay.replaces(reply, reply.copy(seq = 3, rev = 3)))
        assertFalse(ConversationReplay.replaces(reply, reply.copy(sessionId = "session-b", rev = 3)))
        assertFalse(ConversationReplay.replaces(reply, echo))
        assertTrue(ConversationReplay.replaces(reply, reply.copy(rev = 3)))
    }

    @Test fun retryAfterAnUncertainResponseKeepsNonceAndUploadIds() {
        val first = send.copy(uploadedIds = listOf("upload-a"))
        val restored = WireJson.decodeFromString<PendingSend>(WireJson.encodeToString(first))
        assertEquals(first.body(first.uploadedIds!!), restored.body(restored.uploadedIds!!))
        assertTrue(ConversationReplay.confirms(restored, echo))
    }
}

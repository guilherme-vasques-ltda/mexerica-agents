package com.codync.android.core.fork

import com.codync.android.core.Entry
import com.codync.android.core.PendingSend

data class ConversationLane(val botId: String, val sessionId: String)

val Entry.lane: ConversationLane get() = ConversationLane(botId, sessionId ?: botId)
val PendingSend.lane: ConversationLane get() = ConversationLane(botId, sessionId ?: botId)

object ConversationReplay {
    fun confirms(send: PendingSend, entry: Entry): Boolean =
        entry.kind == "user" && entry.lane == send.lane && !entry.clientNonce.isNullOrEmpty() && entry.clientNonce == send.nonce

    fun replaces(current: Entry, incoming: Entry): Boolean = current.id == incoming.id &&
        current.seq == incoming.seq && current.lane == incoming.lane && incoming.rev > current.rev
}

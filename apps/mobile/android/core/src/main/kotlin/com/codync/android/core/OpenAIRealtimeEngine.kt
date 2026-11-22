// Modified from Codync v2.20.3 (Apache-2.0, github.com/leepokai/Codync); see MODIFICACOES.md.
package com.codync.android.core

import java.io.IOException
import java.util.UUID
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.update
import kotlinx.serialization.json.*
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

/** The WebRTC side of an OpenAI Realtime call: the microphone track, the model's voice and the `oai-events` channel. */
interface RealtimePeer {
    enum class Ice { Connected, Disconnected, Failed }
    interface Events { fun opened(); fun received(text: String); fun ice(state: Ice) }
    /** Creates the connection and returns its SDP offer. */
    suspend fun offer(events: Events): String
    suspend fun answer(sdp: String)
    val isOpen: Boolean
    fun send(text: String)
    fun mic(enabled: Boolean)
    suspend fun close()
}

/**
 * OpenAI Realtime over WebRTC, phone ↔ OpenAI directly (iOS `OpenAIRealtimeEngine`): the microphone is
 * a WebRTC audio track, the model's voice plays from the remote track, and events (tools, replies) go
 * over `oai-events`. The user's key stays on the computer, which mints the client secret.
 */
class OpenAIRealtimeEngine(parent: CoroutineScope, settings: VoiceSettings, private val credential: suspend () -> String,
    private val operator: CallOperator, private val peer: RealtimePeer,
    private val exchange: suspend (secret: String, offer: String) -> String,
) : ProviderEngine(parent, VoiceProvider.OpenAI, settings, "OpenAI Realtime"), RealtimePeer.Events {
    /** A response is being generated; new replies wait for it to finish. */
    private var responding = false
    /** Replies to hand over, and tool results as empty entries (a response is asked for once free). */
    private val pending = mutableListOf<String>()
    private data class Delivery(val eventId: String, val itemType: String)
    private val unconfirmed = mutableMapOf<String, Delivery>()
    private var iceAvailable = true
    private var dropTimer: Job? = null

    init {
        hop {
            val secret = credential()
            if (ended) return@hop
            val offer = peer.offer(this)
            peer.mic(!muted)
            val answer = exchange(secret, offer)
            if (ended) return@hop
            peer.answer(answer)
            drop(15_000, "Couldn't reach OpenAI.") { phase == VoicePhase.Starting }
        }
    }

    override fun speak(markdown: String): Boolean {
        if (ended) return false
        hop { pending += operator.reply(markdown); flush() }
        return true
    }
    override fun announce(notice: String) = hop { pending += operator.notice(notice); flush() }
    override fun mute(value: Boolean) = hop {
        operator.muteChanged(value)
        mutable.update { it.copy(muted = value, level = if (value) 0f else it.level) }
        if (phase != VoicePhase.Interrupted) peer.mic(iceAvailable && !value)
    }
    override fun interrupt() = hop { if (phase == VoicePhase.Speaking) { cancelResponse(); phase(VoicePhase.Listening) } }
    override fun focus(active: Boolean) = hop {
        if (!active && this.active) {
            operator.stopListening()
            peer.mic(false)
            if (phase == VoicePhase.Speaking) cancelResponse()
            level(0f); phase(VoicePhase.Interrupted)
        } else if (active && phase == VoicePhase.Interrupted) {
            phase(VoicePhase.Listening)
            resumeListening(); flush()
        }
    }

    override fun opened() = hop {
        if (iceAvailable) dropTimer?.cancel()
        send(buildJsonObject {
            put("type", "session.update")
            putJsonObject("session") {
                put("type", "realtime")
                put("instructions", operator.instructions)
                put("tools", JsonArray(CallOperator.tools.map(CallTool::openAI)))
                putJsonObject("audio") { putJsonObject("input") { putJsonObject("turn_detection") { put("type", "semantic_vad") } } }
            }
        })
        if (phase == VoicePhase.Starting) phase(VoicePhase.Listening)
        resumeListening()
        flush()
    }

    override fun received(text: String) = hop {
        val message = runCatching { WireJson.parseToJsonElement(text).jsonObject }.getOrNull() ?: return@hop
        when (message.stringAt("type")) {
            "input_audio_buffer.speech_started" -> {
                // The provider heard the user speaking: this turn may act. Audio alone isn't speech.
                // While the model talks, what the microphone picks up may be its own voice coming back.
                if (phase == VoicePhase.Listening && iceAvailable) {
                    operator.speechDetected()
                    level(if (muted) 0f else .7f)
                }
            }
            "input_audio_buffer.speech_stopped" -> level(0f)
            // The API confirms a created item with `conversation.item.added` (GA) and finalizes it with
            // `conversation.item.done`; the beta name is still accepted.
            "conversation.item.added", "conversation.item.done", "conversation.item.created" -> {
                val item = message["item"] as? JsonObject ?: return@hop
                val id = item.stringAt("id") ?: return@hop
                val delivery = unconfirmed[id] ?: return@hop
                if (item.stringAt("type") != delivery.itemType) return@hop
                unconfirmed.remove(id)
                operator.confirmed(id)
            }
            "error" -> {
                val id = (message["error"] as? JsonObject)?.stringAt("event_id") ?: return@hop
                // A rejection isn't an ACK: fail this link before teardown discards its pending IDs.
                if (unconfirmed.values.any { it.eventId == id }) fail("OpenAI rejected an item sent during the call.")
            }
            "output_audio_buffer.started" -> if (phase == VoicePhase.Listening) phase(VoicePhase.Speaking)
            "output_audio_buffer.stopped", "output_audio_buffer.cleared" -> if (phase == VoicePhase.Speaking) phase(VoicePhase.Listening)
            "response.created" -> responding = true
            "response.done" -> { responding = false; flush() }
            "response.function_call_arguments.done" -> {
                val id = message.text("call_id") ?: return@hop
                val name = message.text("name") ?: return@hop
                val arguments = message.text("arguments")?.let { runCatching { WireJson.parseToJsonElement(it).jsonObject }.getOrNull() }
                // Our own id for the result: speech the provider reports before confirming it may predate it.
                val item = itemId()
                val output = operator.call(name, arguments ?: JsonObject(emptyMap()), item)
                createItem(item, "function_call_output") {
                    put("call_id", id); put("output", output.toString())
                }
                // An empty entry asks for a response once this one is done, so the model speaks the result.
                pending += ""
            }
        }
    }

    override fun ice(state: RealtimePeer.Ice) = hop {
        when (state) {
            RealtimePeer.Ice.Connected -> {
                if (!iceAvailable) {
                    dropTimer?.cancel()
                    if (phase == VoicePhase.Starting) drop(15_000, "Couldn't reach OpenAI.") { phase == VoicePhase.Starting }
                }
                iceAvailable = true
                resumeListening(); flush()
            }
            RealtimePeer.Ice.Disconnected -> {
                iceAvailable = false
                operator.stopListening(); peer.mic(false); level(0f)
                drop(5_000, "Lost the connection to OpenAI.") { !iceAvailable }
            }
            RealtimePeer.Ice.Failed -> fail("Lost the connection to OpenAI.")
        }
    }

    private fun resumeListening() {
        if (!active || !iceAvailable || !peer.isOpen) return
        peer.mic(!muted)
        operator.audioStarted()
    }

    /** Fails the call after `millis` unless something cancels the timer first. */
    private fun drop(millis: Long, message: String, still: () -> Boolean) {
        dropTimer?.cancel()
        dropTimer = scope.launch { delay(millis); hop { if (still()) fail(message) } }
    }

    private fun cancelResponse() {
        send(buildJsonObject { put("type", "response.cancel") })
        send(buildJsonObject { put("type", "output_audio_buffer.clear") })
    }

    /** Hands waiting replies (and tool results, as empty entries) to the model once it's free. */
    private fun flush() {
        if (responding || !active || !iceAvailable || !peer.isOpen || pending.isEmpty()) return
        for (text in pending) if (text.isNotEmpty()) {
            // A bot's reply is the computer's data, never the user's speech: once it reaches the
            // model, only new speech can ask to send or approve.
            val item = itemId()
            operator.dataDelivered(item)
            createItem(item, "message") {
                put("role", "user")
                putJsonArray("content") { addJsonObject { put("type", "input_text"); put("text", text) } }
            }
        }
        pending.clear()
        responding = true
        send(buildJsonObject { put("type", "response.create") })
    }

    private fun createItem(id: String, type: String, content: JsonObjectBuilder.() -> Unit) {
        val eventId = "evt_" + UUID.randomUUID().toString()
        if (operator.awaitsConfirmation(id)) unconfirmed[id] = Delivery(eventId, type)
        send(buildJsonObject {
            put("type", "conversation.item.create"); put("event_id", eventId)
            putJsonObject("item") { put("id", id); put("type", type); content() }
        })
    }

    private fun JsonObject.stringAt(key: String): String? = (this[key] as? JsonPrimitive)?.takeIf { it.isString }?.content

    private fun send(event: JsonObject) { if (peer.isOpen) peer.send(event.toString()) }

    /** A client-chosen item id (up to 32 characters), echoed back in `conversation.item.created`. */
    private fun itemId() = "mx_" + UUID.randomUUID().toString().replace("-", "").take(29)

    override suspend fun release() {
        operator.resetLink(); unconfirmed.clear()
        dropTimer?.cancel(); peer.close()
    }


    companion object {
        const val CALLS = "https://api.openai.com/v1/realtime/calls"

        /** Posts the offer with the call's client secret and returns OpenAI's SDP answer. */
        suspend fun answer(http: OkHttpClient, secret: String, offer: String, url: String = CALLS): String = withContext(Dispatchers.IO) {
            val request = Request.Builder().url(url).header("Authorization", "Bearer $secret")
                .post(offer.toByteArray().toRequestBody("application/sdp".toMediaType())).build()
            http.newBuilder().callTimeout(15, TimeUnit.SECONDS).build().newCall(request).execute().use { response ->
                val body = response.body?.string().orEmpty()
                if (response.isSuccessful) return@use body
                val message = runCatching { WireJson.parseToJsonElement(body).jsonObject["error"]?.jsonObject?.text("message") }.getOrNull()
                throw IOException(message ?: "OpenAI answered ${response.code}.")
            }
        }
    }
}

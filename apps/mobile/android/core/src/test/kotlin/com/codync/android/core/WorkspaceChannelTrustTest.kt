package com.codync.android.core

import java.io.IOException
import kotlinx.coroutines.async
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.*
import org.junit.Assert.*
import org.junit.Test

class WorkspaceChannelTrustTest {
    private class HubWire : ChannelSocket {
        val incoming = Channel<String>(8)
        val requests = Channel<JsonObject>(8)
        private val seed = ByteArray(32) { 1 }
        private val signingKey = RelayCrypto.signingPublic(seed)
        private val computer = Computer(RelayCrypto.computerId(signingKey), "Fixture", signingKey.base64Url())
        val channel = EncryptedChannel(this, HostRoute.Direct, computer,
            DeviceIdentity(ByteArray(32) { 2 }, ByteArray(32) { 3 }), false)
        private lateinit var opener: FrameOpener
        private lateinit var sealer: FrameSealer

        override suspend fun send(text: String) {
            val frame = WireJson.parseToJsonElement(text).jsonObject
            if (frame.text("t") == "hello") {
                val devicePublic = frame.getValue("ek").jsonPrimitive.content.decodeBase64Url(32)
                val ephemeral = ByteArray(32) { 4 }
                val hostPublic = RelayCrypto.agreementPublic(ephemeral)
                val transcript = RelayCrypto.transcriptHash(computer.id.decodeBase64Url(16),
                    frame.getValue("dk").jsonPrimitive.content.decodeBase64Url(32), devicePublic,
                    frame.getValue("n").jsonPrimitive.content.decodeBase64Url(32), hostPublic)
                val keys = RelayCrypto.channelKeys(RelayCrypto.sharedSecret(ephemeral, devicePublic), transcript)
                opener = FrameOpener(keys.d2h, RelayCrypto.MAX_OUTBOUND)
                sealer = FrameSealer(keys.h2d)
                incoming.send(buildJsonObject {
                    put("t", "welcome"); put("ek", hostPublic.base64Url())
                    put("sig", RelayCrypto.sign(seed, transcript).base64Url())
                }.toString())
            } else if (frame.text("t") == "f") {
                val bytes = opener.open(Frame(frame.getValue("c").jsonPrimitive.long,
                    frame.getValue("d").jsonPrimitive.content.decodeBase64Url())) ?: return
                requests.send(WireJson.parseToJsonElement(bytes.toString(Charsets.UTF_8)).jsonObject)
            }
        }

        suspend fun reply(response: JsonObject) {
            for (frame in sealer.seal(response.toString().toByteArray())) {
                incoming.send(buildJsonObject {
                    put("t", "f"); put("c", frame.counter); put("d", frame.data.base64Url())
                }.toString())
            }
        }

        override suspend fun receive(): String = incoming.receive()
        override fun close(code: Int) { incoming.close(); requests.close() }
    }

    @Test fun `plaintext workspace and device rejects after a signed welcome are not authenticated denials`() = runBlocking {
        for (code in listOf("workspace_revoked", "role_changed", "device_revoked")) withTimeout(3_000) {
            val wire = HubWire()
            wire.channel.start().use { channel ->
                val pending = async { runCatching { channel.call("send") } }
                assertEquals("send", wire.requests.receive().text("m"))
                wire.incoming.send("""{"t":"reject","code":"revoked","stableCode":"$code","message":"Access removed"}""")
                val failed = channel.state.first { it is LinkState.Failed } as LinkState.Failed
                assertNull(failed.status)
                assertNotEquals("Access removed", failed.message)
                val stopped = pending.await().exceptionOrNull()
                assertTrue(stopped is IOException && stopped !is HostError)
                assertTrue(wire.requests.receiveCatching().isClosed)
            }
        }
    }

    @Test fun `a bare 4003 after a signed welcome is not an authenticated denial`() = runBlocking {
        withTimeout(3_000) {
            val wire = HubWire()
            wire.channel.start().use { channel ->
                wire.incoming.close(ChannelClosed(4003))
                val failed = channel.state.first { it is LinkState.Failed } as LinkState.Failed
                assertNull(failed.status)
                assertNotEquals(ChannelClosed(4003).message, failed.message)
            }
        }
    }

    @Test fun `authenticated RPC and subscription conflicts preserve the stable code without closing the wire`() = runBlocking {
        withTimeout(3_000) {
            val wire = HubWire()
            wire.channel.start().use { channel ->
                val stale = async { runCatching { channel.call("workspaceContext", buildJsonObject { put("workspaceEpoch", 9) }) } }
                val request = wire.requests.receive()
                wire.reply(epochConflict(request))
                assertEpochConflict(stale.await())
                val subscription = async {
                    runCatching { channel.subscribe("events", buildJsonObject { put("workspaceEpoch", 9) }).first() }
                }
                val sub = wire.requests.receive()
                assertEquals("events", sub.text("sub"))
                wire.reply(epochConflict(sub))
                assertEpochConflict(subscription.await())
                assertTrue(channel.state.value is LinkState.Ready)
                val fresh = async { channel.call("workspaceContext") }
                var refresh = wire.requests.receive()
                if (refresh["cancel"]?.jsonPrimitive?.booleanOrNull == true) {
                    assertEquals(sub.getValue("id"), refresh.getValue("id"))
                    refresh = wire.requests.receive()
                }
                assertEquals("workspaceContext", refresh.text("m"))
                wire.reply(buildJsonObject {
                    put("id", refresh.getValue("id")); put("ok", buildJsonObject { put("workspaceEpoch", 10) })
                })
                assertEquals(10, fresh.await().jsonObject.getValue("workspaceEpoch").jsonPrimitive.int)
            }
        }
    }

    private fun epochConflict(request: JsonObject) = buildJsonObject {
        put("id", request.getValue("id")); put("err", buildJsonObject {
            put("status", 409); put("code", "workspace_epoch_mismatch"); put("message", "Reload context")
        })
    }

    private fun assertEpochConflict(result: Result<JsonElement>) {
        val error = result.exceptionOrNull() as HostError
        assertEquals(409, error.status)
        assertEquals("workspace_epoch_mismatch", error.code)
    }
}

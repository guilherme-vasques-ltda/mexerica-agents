package com.codync.android.core.fork

import java.io.IOException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test

class ForkMailInboxTest {
    private class FixtureTransport : MailInboxTransport {
        val statusResponses = mutableMapOf<String, () -> MailInboxStatus>()
        val listResponses = mutableMapOf<String, () -> MailInboxListThreadsResponse>()
        val openResponses = mutableMapOf<String, () -> MailInboxOpenThreadResponse>()
        val recordedCalls = mutableListOf<String>()
        var delayMs: Long = 0L

        override suspend fun mailInboxStatus(spaceId: String): MailInboxStatus {
            recordedCalls += "status:$spaceId"
            if (delayMs > 0) delay(delayMs)
            val provider = statusResponses[spaceId] ?: throw MailInboxException(MailInboxError.ACCESS_DENIED)
            return provider()
        }

        override suspend fun mailInboxListThreads(
            spaceId: String,
            mailboxId: String,
            cursor: String?,
        ): MailInboxListThreadsResponse {
            recordedCalls += "list:$spaceId:$mailboxId:${cursor ?: "nil"}"
            if (delayMs > 0) delay(delayMs)
            val key = "$spaceId:$mailboxId:${cursor ?: "page1"}"
            val provider = listResponses[key] ?: throw MailInboxException(MailInboxError.ACCESS_DENIED)
            return provider()
        }

        override suspend fun mailInboxOpenThread(
            spaceId: String,
            mailboxId: String,
            threadId: String,
        ): MailInboxOpenThreadResponse {
            recordedCalls += "open:$spaceId:$mailboxId:$threadId"
            if (delayMs > 0) delay(delayMs)
            val key = "$spaceId:$mailboxId:$threadId"
            val provider = openResponses[key] ?: throw MailInboxException(MailInboxError.ACCESS_DENIED)
            return provider()
        }
    }

    private fun makeFixtureTransport(): FixtureTransport = FixtureTransport().apply {
        statusResponses["space-alpha"] = {
            MailInboxStatus.Ready(
                listOf(
                    MailInboxMailbox("mbox-empty", "Caixa Vazia"),
                    MailInboxMailbox("mbox-inbox", "Caixa Principal"),
                ),
            )
        }
        statusResponses["space-beta"] = {
            MailInboxStatus.Ready(listOf(MailInboxMailbox("mbox-beta", "Equipe Beta")))
        }
        listResponses["space-alpha:mbox-empty:page1"] = {
            MailInboxListThreadsResponse(emptyList(), null)
        }
        listResponses["space-alpha:mbox-inbox:page1"] = {
            MailInboxListThreadsResponse(
                threads = listOf(
                    MailInboxThreadSummary("thr-100", "Fatura mensal", "2026-11-22T14:00:00Z"),
                ),
                nextCursor = "cur-page-2",
            )
        }
        listResponses["space-alpha:mbox-inbox:cur-page-2"] = {
            MailInboxListThreadsResponse(
                threads = listOf(
                    MailInboxThreadSummary("thr-099", "Boas-vindas", "2026-11-21T09:30:00Z"),
                ),
                nextCursor = null,
            )
        }
        openResponses["space-alpha:mbox-inbox:thr-100"] = {
            MailInboxOpenThreadResponse(
                threadId = "thr-100",
                subject = "Fatura mensal",
                messages = listOf(
                    MailInboxMessage(
                        messageId = "msg-1",
                        fromDisplay = "Financeiro Lab",
                        fromDomain = "billing.example.invalid",
                        sentAt = "2026-11-22T13:50:00Z",
                        bodyText = "Confira o resumo em https://portal.example.invalid/fatura",
                    ),
                    MailInboxMessage(
                        messageId = "msg-2",
                        fromDisplay = "Contato Sem Dominio",
                        fromDomain = null,
                        sentAt = "2026-11-22T14:00:00Z",
                        bodyText = "Recebido, obrigado.",
                    ),
                ),
                truncated = true,
            )
        }
    }

    @Test fun `feature gate disabled fails closed as unavailable`() = runBlocking {
        val transport = makeFixtureTransport()
        val store = ForkMailInboxStore(transport, featureEnabled = false)

        store.loadStatus("space-alpha")
        assertEquals(MailInboxStatus.Unavailable, store.status)
        assertEquals(MailInboxError.UNAVAILABLE, store.lastError)
        assertFalse(store.isEmptyInbox)

        store.loadFirstPage("space-alpha", "mbox-inbox")
        assertEquals(MailInboxError.UNAVAILABLE, store.lastError)

        store.openThread("space-alpha", "mbox-inbox", "thr-100")
        assertEquals(MailInboxError.UNAVAILABLE, store.lastError)
        assertTrue(transport.recordedCalls.isEmpty())
    }

    @Test fun `empty spaceId fails with invalid_request without defaulting`() = runBlocking {
        val transport = makeFixtureTransport()
        val store = ForkMailInboxStore(transport, featureEnabled = true)

        store.loadStatus("   ")
        assertEquals(MailInboxError.INVALID_REQUEST, store.lastError)
        assertNull(store.activeSpaceId)
        assertTrue(transport.recordedCalls.isEmpty())
    }

    @Test fun `lists paginates and opens thread with separate domain and truncated flag`() = runBlocking {
        val transport = makeFixtureTransport()
        val store = ForkMailInboxStore(transport, featureEnabled = true)

        store.loadStatus("space-alpha")
        assertTrue(store.status is MailInboxStatus.Ready)

        store.loadFirstPage("space-alpha", "mbox-inbox")
        assertEquals(listOf("thr-100"), store.threads.map { it.threadId })
        assertEquals("cur-page-2", store.nextCursor)
        assertFalse(store.isEmptyInbox)

        store.loadNextPage()
        assertEquals(listOf("thr-100", "thr-099"), store.threads.map { it.threadId })
        assertNull(store.nextCursor)

        store.openThread("space-alpha", "mbox-inbox", "thr-100")
        val opened = store.openThread!!
        assertTrue(opened.truncated)
        assertEquals(2, opened.messages.size)
        assertEquals("Financeiro Lab", opened.messages[0].fromDisplay)
        assertEquals("billing.example.invalid", opened.messages[0].fromDomain)
        assertTrue(opened.messages[0].senderPresentation.hasDeclaredDomain)
        assertFalse(opened.messages[0].senderPresentation.isVerifiedIdentity)
        assertNull(opened.messages[1].fromDomain)
        assertFalse(opened.messages[1].senderPresentation.hasDeclaredDomain)
        assertFalse(opened.messages[1].senderPresentation.isVerifiedIdentity)
    }

    @Test fun `empty inbox only on successful first page and distinct from unavailable or denied`() = runBlocking {
        val transport = makeFixtureTransport()
        val store = ForkMailInboxStore(transport, featureEnabled = true)

        store.loadStatus("space-alpha")
        store.loadFirstPage("space-alpha", "mbox-empty")
        assertTrue(store.isEmptyInbox)

        transport.statusResponses["space-offline"] = { throw IOException("network timeout") }
        store.loadStatus("space-offline")
        assertEquals(MailInboxStatus.Unavailable, store.status)
        assertEquals(MailInboxError.UNAVAILABLE, store.lastError)
        assertFalse(store.isEmptyInbox)

        store.loadStatus("space-unknown")
        assertEquals(MailInboxError.ACCESS_DENIED, store.lastError)
        assertFalse(store.isEmptyInbox)
    }

    @Test fun `cursor_invalid restarts from page 1 without mixing previous pages`() = runBlocking {
        val transport = makeFixtureTransport()
        transport.listResponses["space-alpha:mbox-inbox:cur-page-2"] = {
            throw MailInboxException(MailInboxError.CURSOR_INVALID)
        }

        val store = ForkMailInboxStore(transport, featureEnabled = true)
        store.loadStatus("space-alpha")
        store.loadFirstPage("space-alpha", "mbox-inbox")
        assertEquals(listOf("thr-100"), store.threads.map { it.threadId })

        transport.listResponses["space-alpha:mbox-inbox:page1"] = {
            MailInboxListThreadsResponse(
                threads = listOf(
                    MailInboxThreadSummary("thr-200", "Pagina 1 reiniciada", "2026-11-23T10:00:00Z"),
                ),
                nextCursor = null,
            )
        }

        store.loadNextPage()
        assertNull(store.lastError)
        assertEquals(listOf("thr-200"), store.threads.map { it.threadId })
        assertNull(store.nextCursor)
    }

    @Test fun `sanitizes bidi and control chars while preserving rtl and newlines`() = runBlocking {
        val transport = makeFixtureTransport()
        val bidiSubject = "Aviso \u202Egpj.exe\u202C importante\u0007"
        val bidiSender = "Remetente\u2066Falso\u2069\u200E\u200F\u061C"
        val body = "Linha 1\r\nLinha 2 \u202Einversa\u202C\nשלום"

        transport.openResponses["space-alpha:mbox-inbox:thr-bidi"] = {
            MailInboxOpenThreadResponse(
                threadId = "thr-bidi",
                subject = bidiSubject,
                messages = listOf(
                    MailInboxMessage(
                        messageId = "msg-bidi",
                        fromDisplay = bidiSender,
                        fromDomain = "mail\u202E.example.invalid",
                        sentAt = "2026-11-23T08:00:00Z",
                        bodyText = body,
                    ),
                ),
                truncated = false,
            )
        }

        val store = ForkMailInboxStore(transport, featureEnabled = true)
        store.openThread("space-alpha", "mbox-inbox", "thr-bidi")

        val opened = store.openThread!!
        assertEquals("Aviso gpj.exe importante", opened.subject)
        assertEquals("RemetenteFalso", opened.messages[0].fromDisplay)
        assertEquals("mail.example.invalid", opened.messages[0].fromDomain)
        assertEquals("Linha 1\nLinha 2 inversa\nשלום", opened.messages[0].bodyText)
    }

    @Test fun `context change discards in-flight response and clears sensitive state`() = runBlocking {
        val transport = makeFixtureTransport().apply { delayMs = 60L }
        val store = ForkMailInboxStore(transport, featureEnabled = true)

        val job = launch { store.loadStatus("space-alpha") }
        delay(10L)
        store.resetForContextChange()
        job.join()

        assertEquals(1L, store.generation)
        assertNull(store.activeSpaceId)
        assertNull(store.status)
        assertTrue(store.threads.isEmpty())
        assertNull(store.openThread)
        assertFalse(store.shouldMaskInAppSwitcher)
    }

    @Test fun `app switcher mask and toString redaction protect sensitive fields`() = runBlocking {
        val transport = makeFixtureTransport()
        val store = ForkMailInboxStore(transport, featureEnabled = true)
        assertFalse(store.shouldMaskInAppSwitcher)

        store.loadStatus("space-alpha")
        store.loadFirstPage("space-alpha", "mbox-inbox")
        store.openThread("space-alpha", "mbox-inbox", "thr-100")

        assertTrue(store.shouldMaskInAppSwitcher)

        val dumped = listOf(
            store.toString(),
            store.status.toString(),
            store.threads[0].toString(),
            store.openThread.toString(),
            store.openThread!!.messages[0].toString(),
        ).joinToString("\n")

        for (secret in listOf(
            "Fatura mensal",
            "Financeiro Lab",
            "billing.example.invalid",
            "portal.example.invalid",
            "mbox-inbox",
            "thr-100",
            "msg-1",
            "cur-page-2",
        )) {
            assertFalse("Leaked $secret in $dumped", dumped.contains(secret))
        }

        store.resetForContextChange()
        assertFalse(store.shouldMaskInAppSwitcher)
    }
}

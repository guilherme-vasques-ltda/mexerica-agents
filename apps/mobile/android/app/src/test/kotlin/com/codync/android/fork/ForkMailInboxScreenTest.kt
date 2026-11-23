package com.codync.android.fork

import com.codync.android.core.fork.ForkMailInboxStore
import com.codync.android.core.fork.MailInboxError
import com.codync.android.core.fork.MailInboxException
import com.codync.android.core.fork.MailInboxListThreadsResponse
import com.codync.android.core.fork.MailInboxOpenThreadResponse
import com.codync.android.core.fork.MailInboxStatus
import com.codync.android.core.fork.MailInboxTransport
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ForkMailInboxScreenTest {

    private class CountingTransport(
        private val delegate: MailInboxTransport = ForkMailInboxFixtureTransport(),
    ) : MailInboxTransport {
        var calls = 0
            private set

        override suspend fun mailInboxStatus(spaceId: String): MailInboxStatus {
            calls += 1
            return delegate.mailInboxStatus(spaceId)
        }

        override suspend fun mailInboxListThreads(
            spaceId: String,
            mailboxId: String,
            cursor: String?,
        ): MailInboxListThreadsResponse {
            calls += 1
            return delegate.mailInboxListThreads(spaceId, mailboxId, cursor)
        }

        override suspend fun mailInboxOpenThread(
            spaceId: String,
            mailboxId: String,
            threadId: String,
        ): MailInboxOpenThreadResponse {
            calls += 1
            return delegate.mailInboxOpenThread(spaceId, mailboxId, threadId)
        }
    }

    @Test
    fun featureDisabledShowsUnavailableWithoutCallingTransport() = runBlocking {
        val transport = CountingTransport()
        val store = ForkMailInboxStore(transport = transport, featureEnabled = false)

        val initialUi = toForkMailInboxUiState(store)
        assertFalse(initialUi.featureEnabled)
        assertEquals(ForkMailInboxScreenState.Unavailable, initialUi.screenState)
        assertFalse(initialUi.showPrivacyMask)

        store.loadStatus(ForkMailInboxFixtureTransport.SPACE_ALPHA)
        store.loadFirstPage(
            ForkMailInboxFixtureTransport.SPACE_ALPHA,
            ForkMailInboxFixtureTransport.MAILBOX_MAIN,
        )
        store.openThread(
            ForkMailInboxFixtureTransport.SPACE_ALPHA,
            ForkMailInboxFixtureTransport.MAILBOX_MAIN,
            ForkMailInboxFixtureTransport.THREAD_WELCOME,
        )

        assertEquals(0, transport.calls)
        val afterCallsUi = toForkMailInboxUiState(store)
        assertEquals(ForkMailInboxScreenState.Unavailable, afterCallsUi.screenState)
    }

    @Test
    fun distinctEmptyAndConnectionStatesNeverCollapseIntoEmptyInbox() = runBlocking {
        val notConnectedStore = ForkMailInboxStore(
            transport = ForkMailInboxFixtureTransport(
                mapOf(ForkMailInboxFixtureTransport.SPACE_ALPHA to MailInboxStatus.NotConnected),
            ),
            featureEnabled = true,
        )
        notConnectedStore.loadStatus(ForkMailInboxFixtureTransport.SPACE_ALPHA)
        assertEquals(
            ForkMailInboxScreenState.NotConnected,
            toForkMailInboxUiState(notConnectedStore).screenState,
        )

        val reconnectStore = ForkMailInboxStore(
            transport = ForkMailInboxFixtureTransport(
                mapOf(ForkMailInboxFixtureTransport.SPACE_ALPHA to MailInboxStatus.ReconnectRequired),
            ),
            featureEnabled = true,
        )
        reconnectStore.loadStatus(ForkMailInboxFixtureTransport.SPACE_ALPHA)
        assertEquals(
            ForkMailInboxScreenState.ReconnectRequired,
            toForkMailInboxUiState(reconnectStore).screenState,
        )

        val unavailableStore = ForkMailInboxStore(
            transport = ForkMailInboxFixtureTransport(
                mapOf(ForkMailInboxFixtureTransport.SPACE_ALPHA to MailInboxStatus.Unavailable),
            ),
            featureEnabled = true,
        )
        unavailableStore.loadStatus(ForkMailInboxFixtureTransport.SPACE_ALPHA)
        assertEquals(
            ForkMailInboxScreenState.Unavailable,
            toForkMailInboxUiState(unavailableStore).screenState,
        )

        val deniedStore = ForkMailInboxStore(
            transport = ForkMailInboxFixtureTransport(),
            featureEnabled = true,
        )
        deniedStore.loadStatus("unknown-space-fixture")
        assertEquals(
            ForkMailInboxScreenState.AccessDenied,
            toForkMailInboxUiState(deniedStore).screenState,
        )

        val emptyInboxStore = ForkMailInboxStore(
            transport = ForkMailInboxFixtureTransport(),
            featureEnabled = true,
        )
        emptyInboxStore.loadStatus(ForkMailInboxFixtureTransport.SPACE_ALPHA)
        emptyInboxStore.loadFirstPage(
            ForkMailInboxFixtureTransport.SPACE_ALPHA,
            ForkMailInboxFixtureTransport.MAILBOX_EMPTY,
        )
        assertTrue(emptyInboxStore.isEmptyInbox)
        assertEquals(
            ForkMailInboxScreenState.EmptyInbox,
            toForkMailInboxUiState(emptyInboxStore).screenState,
        )
    }

    @Test
    fun loadingStateIsMappedWhileStatusRequestIsInFlight() = runBlocking {
        val gate = CompletableDeferred<Unit>()
        val blockingTransport = object : MailInboxTransport {
            override suspend fun mailInboxStatus(spaceId: String): MailInboxStatus {
                gate.await()
                return MailInboxStatus.Ready(emptyList())
            }

            override suspend fun mailInboxListThreads(
                spaceId: String,
                mailboxId: String,
                cursor: String?,
            ): MailInboxListThreadsResponse = MailInboxListThreadsResponse(emptyList(), null)

            override suspend fun mailInboxOpenThread(
                spaceId: String,
                mailboxId: String,
                threadId: String,
            ): MailInboxOpenThreadResponse = throw MailInboxException(MailInboxError.UNAVAILABLE)
        }
        val store = ForkMailInboxStore(transport = blockingTransport, featureEnabled = true)
        val job = launch { store.loadStatus(ForkMailInboxFixtureTransport.SPACE_ALPHA) }
        kotlinx.coroutines.yield()

        assertTrue(store.isLoadingStatus)
        assertEquals(ForkMailInboxScreenState.Loading, toForkMailInboxUiState(store).screenState)

        gate.complete(Unit)
        job.join()
    }

    @Test
    fun paginationAndThreadOpeningWithSeparatedUnverifiedDomainAndTruncatedBanner() = runBlocking {
        val store = ForkMailInboxStore(
            transport = ForkMailInboxFixtureTransport(),
            featureEnabled = true,
        )
        store.loadStatus(ForkMailInboxFixtureTransport.SPACE_ALPHA)
        store.loadFirstPage(
            ForkMailInboxFixtureTransport.SPACE_ALPHA,
            ForkMailInboxFixtureTransport.MAILBOX_MAIN,
        )

        val page1 = toForkMailInboxUiState(store).screenState as ForkMailInboxScreenState.ThreadList
        assertEquals(2, page1.mailboxes.size)
        assertEquals(2, page1.threads.size)
        assertTrue(page1.hasNextPage)
        assertEquals("Resumo semanal de operação", page1.threads[0].subject)
        assertEquals("Sem assunto", page1.threads[1].subject)

        store.loadNextPage()
        val page2 = toForkMailInboxUiState(store).screenState as ForkMailInboxScreenState.ThreadList
        assertEquals(3, page2.threads.size)
        assertFalse(page2.hasNextPage)

        store.openThread(
            ForkMailInboxFixtureTransport.SPACE_ALPHA,
            ForkMailInboxFixtureTransport.MAILBOX_MAIN,
            ForkMailInboxFixtureTransport.THREAD_WELCOME,
        )
        val detail = toForkMailInboxUiState(store).screenState as ForkMailInboxScreenState.ThreadDetail
        assertFalse(detail.truncated)
        assertEquals(2, detail.messages.size)
        assertEquals("Equipe Norte", detail.messages[0].senderDisplayName)
        assertEquals("norte.example.invalid", detail.messages[0].declaredDomain)
        assertFalse(detail.messages[0].isVerifiedIdentity)
        assertEquals("Suporte Sul", detail.messages[1].senderDisplayName)
        assertEquals("sul.example.invalid", detail.messages[1].declaredDomain)
        assertFalse(detail.messages[1].isVerifiedIdentity)

        val backToList = toForkMailInboxUiState(store, preferListOverDetail = true).screenState
        assertTrue(backToList is ForkMailInboxScreenState.ThreadList)

        store.openThread(
            ForkMailInboxFixtureTransport.SPACE_ALPHA,
            ForkMailInboxFixtureTransport.MAILBOX_MAIN,
            ForkMailInboxFixtureTransport.THREAD_TRUNCATED,
        )
        val truncatedDetail = toForkMailInboxUiState(store).screenState as ForkMailInboxScreenState.ThreadDetail
        assertTrue(truncatedDetail.truncated)
        assertEquals("Sem assunto", truncatedDetail.subject)
        assertEquals("Remetente não identificado", truncatedDetail.messages[0].senderDisplayName)
        assertNull(truncatedDetail.messages[0].declaredDomain)
        assertFalse(truncatedDetail.messages[0].isVerifiedIdentity)
    }

    @Test
    fun privacyMaskActivatesInBackgroundOnlyWhenSensitiveDataExists() = runBlocking {
        val store = ForkMailInboxStore(
            transport = ForkMailInboxFixtureTransport(),
            featureEnabled = true,
        )
        assertFalse(shouldShowPrivacyMask(store.shouldMaskInAppSwitcher, isAppInBackground = true))
        assertFalse(toForkMailInboxUiState(store, isAppInBackground = true).showPrivacyMask)

        store.loadStatus(ForkMailInboxFixtureTransport.SPACE_ALPHA)
        store.loadFirstPage(
            ForkMailInboxFixtureTransport.SPACE_ALPHA,
            ForkMailInboxFixtureTransport.MAILBOX_MAIN,
        )

        assertTrue(store.shouldMaskInAppSwitcher)
        assertFalse(toForkMailInboxUiState(store, isAppInBackground = false).showPrivacyMask)
        assertTrue(toForkMailInboxUiState(store, isAppInBackground = true).showPrivacyMask)
    }

    @Test
    fun revocationClearsSensitiveStateAndCrossWorkspaceAccessIsDenied() = runBlocking {
        var failWithReconnect = false
        val delegate = ForkMailInboxFixtureTransport()
        val revocableTransport = object : MailInboxTransport {
            override suspend fun mailInboxStatus(spaceId: String): MailInboxStatus =
                if (failWithReconnect) MailInboxStatus.ReconnectRequired else delegate.mailInboxStatus(spaceId)

            override suspend fun mailInboxListThreads(
                spaceId: String,
                mailboxId: String,
                cursor: String?,
            ): MailInboxListThreadsResponse =
                if (failWithReconnect) {
                    throw MailInboxException(MailInboxError.RECONNECT_REQUIRED)
                } else {
                    delegate.mailInboxListThreads(spaceId, mailboxId, cursor)
                }

            override suspend fun mailInboxOpenThread(
                spaceId: String,
                mailboxId: String,
                threadId: String,
            ): MailInboxOpenThreadResponse =
                if (failWithReconnect) {
                    throw MailInboxException(MailInboxError.RECONNECT_REQUIRED)
                } else {
                    delegate.mailInboxOpenThread(spaceId, mailboxId, threadId)
                }
        }

        val store = ForkMailInboxStore(transport = revocableTransport, featureEnabled = true)
        store.loadStatus(ForkMailInboxFixtureTransport.SPACE_ALPHA)
        store.loadFirstPage(
            ForkMailInboxFixtureTransport.SPACE_ALPHA,
            ForkMailInboxFixtureTransport.MAILBOX_MAIN,
        )
        store.openThread(
            ForkMailInboxFixtureTransport.SPACE_ALPHA,
            ForkMailInboxFixtureTransport.MAILBOX_MAIN,
            ForkMailInboxFixtureTransport.THREAD_WELCOME,
        )
        assertTrue(store.shouldMaskInAppSwitcher)

        failWithReconnect = true
        store.loadStatus(ForkMailInboxFixtureTransport.SPACE_ALPHA)

        assertTrue(store.threads.isEmpty())
        assertNull(store.openThread)
        assertNull(store.selectedMailboxId)
        assertFalse(store.shouldMaskInAppSwitcher)
        assertEquals(
            ForkMailInboxScreenState.ReconnectRequired,
            toForkMailInboxUiState(store, isAppInBackground = true).screenState,
        )
        assertFalse(toForkMailInboxUiState(store, isAppInBackground = true).showPrivacyMask)

        failWithReconnect = false
        store.loadStatus(ForkMailInboxFixtureTransport.SPACE_BETA)
        store.loadFirstPage(
            ForkMailInboxFixtureTransport.SPACE_BETA,
            ForkMailInboxFixtureTransport.MAILBOX_MAIN,
        )
        assertEquals(
            ForkMailInboxScreenState.AccessDenied,
            toForkMailInboxUiState(store).screenState,
        )
    }
}

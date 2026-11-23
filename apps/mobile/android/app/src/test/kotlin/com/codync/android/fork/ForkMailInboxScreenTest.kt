package com.codync.android.fork

import com.codync.android.core.fork.ForkMailInboxStore
import com.codync.android.core.fork.MailInboxError
import com.codync.android.core.fork.MailInboxException
import com.codync.android.core.fork.MailInboxListThreadsResponse
import com.codync.android.core.fork.MailInboxOpenThreadResponse
import com.codync.android.core.fork.MailInboxStatus
import com.codync.android.core.fork.MailInboxTransport
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
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
    fun presentationPublishesLoadingWhileRequestsAreSuspended() = runBlocking {
        val statusGate = CompletableDeferred<Unit>()
        val firstPageGate = CompletableDeferred<Unit>()
        val firstPageStarted = CompletableDeferred<Unit>()
        val nextPageGate = CompletableDeferred<Unit>()
        val detailGate = CompletableDeferred<Unit>()
        val fixture = ForkMailInboxFixtureTransport()
        val blockingTransport = object : MailInboxTransport by fixture {
            override suspend fun mailInboxStatus(spaceId: String): MailInboxStatus {
                statusGate.await()
                return fixture.mailInboxStatus(spaceId)
            }

            override suspend fun mailInboxListThreads(
                spaceId: String,
                mailboxId: String,
                cursor: String?,
            ): MailInboxListThreadsResponse {
                if (cursor == null) {
                    firstPageStarted.complete(Unit)
                    firstPageGate.await()
                } else {
                    nextPageGate.await()
                }
                return fixture.mailInboxListThreads(spaceId, mailboxId, cursor)
            }

            override suspend fun mailInboxOpenThread(
                spaceId: String,
                mailboxId: String,
                threadId: String,
            ): MailInboxOpenThreadResponse {
                detailGate.await()
                return fixture.mailInboxOpenThread(spaceId, mailboxId, threadId)
            }
        }
        val store = ForkMailInboxStore(transport = blockingTransport, featureEnabled = true)
        val presentation = ForkMailInboxPresentation(store, ForkMailInboxFixtureTransport.SPACE_ALPHA)
        presentation.setBackground(false)
        assertEquals(ForkMailInboxScreenState.Loading, presentation.uiState.value.screenState)
        withTimeout(5_000) {
            val initial = launch(start = CoroutineStart.UNDISPATCHED) { presentation.start() }
            assertTrue(store.isLoadingStatus)
            assertEquals(ForkMailInboxScreenState.Loading, presentation.uiState.value.screenState)
            statusGate.complete(Unit)
            firstPageStarted.await()
            assertTrue(store.isLoadingThreads)
            assertEquals(ForkMailInboxScreenState.Loading, presentation.uiState.value.screenState)
            firstPageGate.complete(Unit)
            initial.join()

            val pagination = launch { presentation.loadMore() }
            val paging = presentation.uiState.first { (it.screenState as? ForkMailInboxScreenState.ThreadList)?.isLoadingMore == true }
            assertFalse(paging.canInteract)
            presentation.selectMailbox(ForkMailInboxFixtureTransport.MAILBOX_EMPTY)
            presentation.openThread(ForkMailInboxFixtureTransport.THREAD_WELCOME)
            assertEquals(ForkMailInboxFixtureTransport.MAILBOX_MAIN, store.selectedMailboxId)
            assertFalse(store.isLoadingThreadDetail)
            assertFalse(pagination.isCompleted)
            nextPageGate.complete(Unit)
            pagination.join()
            assertTrue(presentation.uiState.value.canInteract)

            val detail = launch { presentation.openThread(ForkMailInboxFixtureTransport.THREAD_WELCOME) }
            val opening = presentation.uiState.first { (it.screenState as? ForkMailInboxScreenState.ThreadList)?.isLoadingDetail == true }
            assertFalse(opening.canInteract)
            assertFalse(detail.isCompleted)
            detailGate.complete(Unit)
            detail.join()
            assertTrue(presentation.uiState.value.screenState is ForkMailInboxScreenState.ThreadDetail)
        }
    }

    @Test
    fun failedRequestDoesNotExposePreviousRowsOrMailboxActions() = runBlocking {
        val fixture = ForkMailInboxFixtureTransport()
        val transport = object : MailInboxTransport by fixture {
            override suspend fun mailInboxOpenThread(spaceId: String, mailboxId: String, threadId: String): MailInboxOpenThreadResponse =
                throw MailInboxException(MailInboxError.INVALID_REQUEST)
        }
        val store = ForkMailInboxStore(transport, featureEnabled = true)
        val presentation = ForkMailInboxPresentation(store, ForkMailInboxFixtureTransport.SPACE_ALPHA)
        presentation.setBackground(false)
        presentation.start()
        presentation.openThread(ForkMailInboxFixtureTransport.THREAD_WELCOME)
        assertTrue(store.threads.isNotEmpty())
        val ui = presentation.uiState.value
        assertEquals(ForkMailInboxScreenState.Unavailable, ui.screenState)
        assertTrue(ui.mailboxes.isEmpty())
        assertFalse(ui.canInteract)
    }

    @Test
    fun readyWithNoMailboxesIsNeitherDisconnectedNorAnEmptyInbox() = runBlocking {
        val space = ForkMailInboxFixtureTransport.SPACE_ALPHA
        val transport = CountingTransport(ForkMailInboxFixtureTransport(mapOf(space to MailInboxStatus.Ready(emptyList()))))
        val store = ForkMailInboxStore(transport, featureEnabled = true)
        val presentation = ForkMailInboxPresentation(store, space)
        presentation.start()
        assertEquals(ForkMailInboxScreenState.NoMailboxes, presentation.uiState.value.screenState)
        assertTrue(presentation.uiState.value.mailboxes.isEmpty())
        assertFalse(store.isEmptyInbox)
        assertEquals(1, transport.calls)
    }

    @Test
    fun emptyInboxKeepsTheMailboxSelectorAndCanSwitchBackToThreads() = runBlocking {
        val store = ForkMailInboxStore(ForkMailInboxFixtureTransport(), featureEnabled = true)
        val presentation = ForkMailInboxPresentation(store, ForkMailInboxFixtureTransport.SPACE_ALPHA)
        presentation.setBackground(false)
        presentation.start()
        presentation.selectMailbox(ForkMailInboxFixtureTransport.MAILBOX_EMPTY)
        assertEquals(ForkMailInboxScreenState.EmptyInbox, presentation.uiState.value.screenState)
        assertEquals(ForkMailInboxFixtureTransport.MAILBOX_EMPTY, presentation.uiState.value.selectedMailboxId)
        val main = presentation.uiState.value.mailboxes.first { it.mailboxId == ForkMailInboxFixtureTransport.MAILBOX_MAIN }
        presentation.selectMailbox(main.mailboxId)
        assertEquals(main.mailboxId, presentation.uiState.value.selectedMailboxId)
        assertTrue((presentation.uiState.value.screenState as ForkMailInboxScreenState.ThreadList).threads.isNotEmpty())
    }

    @Test
    fun duplicateIdsAcrossPagesAndInDetailsKeepFirstOccurrenceInOrder() = runBlocking {
        val fixture = ForkMailInboxFixtureTransport()
        val transport = object : MailInboxTransport by fixture {
            override suspend fun mailInboxListThreads(spaceId: String, mailboxId: String, cursor: String?): MailInboxListThreadsResponse {
                val first = fixture.mailInboxListThreads(spaceId, mailboxId, null)
                val page = fixture.mailInboxListThreads(spaceId, mailboxId, cursor)
                if (cursor == null) return page
                return page.copy(threads = listOf(first.threads.first().copy(subject = "Repetido")) + page.threads)
            }

            override suspend fun mailInboxOpenThread(spaceId: String, mailboxId: String, threadId: String): MailInboxOpenThreadResponse {
                val detail = fixture.mailInboxOpenThread(spaceId, mailboxId, threadId)
                return detail.copy(messages = detail.messages + detail.messages.first().copy(bodyText = "Repetido"))
            }
        }
        val presentation = ForkMailInboxPresentation(ForkMailInboxStore(transport, featureEnabled = true), ForkMailInboxFixtureTransport.SPACE_ALPHA)
        presentation.setBackground(false)
        presentation.start()
        val first = presentation.uiState.value.screenState as ForkMailInboxScreenState.ThreadList
        presentation.loadMore()
        val all = presentation.uiState.value.screenState as ForkMailInboxScreenState.ThreadList
        assertEquals(first.threads, all.threads.take(first.threads.size))
        assertEquals(first.threads.map { it.threadId } + ForkMailInboxFixtureTransport.THREAD_OLDER, all.threads.map { it.threadId })
        presentation.openThread(ForkMailInboxFixtureTransport.THREAD_WELCOME)
        val detail = presentation.uiState.value.screenState as ForkMailInboxScreenState.ThreadDetail
        assertEquals(listOf("Equipe Norte", "Suporte Sul"), detail.messages.map { it.senderDisplayName })
        assertFalse(detail.messages.any { it.bodyText == "Repetido" })
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
        assertEquals(2, toForkMailInboxUiState(store).mailboxes.size)
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

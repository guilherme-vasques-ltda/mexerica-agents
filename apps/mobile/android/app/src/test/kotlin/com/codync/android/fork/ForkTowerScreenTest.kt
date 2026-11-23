package com.codync.android.fork

import com.codync.android.core.fork.AttentionActionKind
import com.codync.android.core.fork.AttentionItem
import com.codync.android.core.fork.AttentionKind
import com.codync.android.core.fork.AttentionQueue
import com.codync.android.core.fork.AttentionSeverity
import com.codync.android.core.fork.AttentionTarget
import com.codync.android.core.fork.CoverageGap
import com.codync.android.core.fork.CoverageGapCode
import com.codync.android.core.fork.DoctorActionKind
import com.codync.android.core.fork.DoctorArea
import com.codync.android.core.fork.DoctorCheck
import com.codync.android.core.fork.DoctorCheckStatus
import com.codync.android.core.fork.DoctorOverallStatus
import com.codync.android.core.fork.DoctorTransportDiagnosis
import com.codync.android.core.fork.HubDoctorReport
import com.codync.android.core.fork.Observation
import com.codync.android.core.fork.ObservedExecution
import com.codync.android.core.fork.ObservedExecutionStatus
import com.codync.android.core.fork.ResolvedAttentionItem
import com.codync.android.core.fork.TowerInvalidationEvent
import com.codync.android.core.fork.TowerInvalidationEventType
import com.codync.android.core.fork.TowerPerspective
import com.codync.android.core.fork.WorkBlocker
import com.codync.android.core.fork.WorkBlockerKind
import com.codync.android.core.fork.WorkBucket
import com.codync.android.core.fork.WorkCapabilities
import com.codync.android.core.fork.WorkCard
import com.codync.android.core.fork.WorkDelivery
import com.codync.android.core.fork.WorkOrigin
import com.codync.android.core.fork.WorkOriginKind
import com.codync.android.core.fork.WorkRequesterLabel
import com.codync.android.core.fork.TowerViewCounts
import com.codync.android.core.fork.TowerViewSnapshot
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ForkTowerScreenTest {
    @Test fun unavailableIsNotAnEmptyQueue() {
        val result = ForkAttentionPresentation.from(Observation.Unavailable)
        assertFalse(result.available)
        assertFalse(result.showEmpty)
    }

    @Test fun gapsAreShownAlongsideAnEmptyQueue() {
        val result = ForkAttentionPresentation.from(ForkTowerFixtureCatalog.attentionWithGap)
        assertEquals(listOf("doctor"), result.gaps)
        assertFalse(result.showEmpty)
    }

    @Test fun resolvedItemsStartCollapsed() {
        val result = ForkAttentionPresentation.from(Observation.Available(queue(resolved = listOf(resolved()))))
        assertTrue(result.isResolvedTodayCollapsedByDefault)
    }

    @Test fun doctorKeepsOfflineAiAndApprovalStatesDistinct() {
        val report = Observation.Available(report())
        assertEquals(DoctorActionKind.START_HUB, ForkDoctorPresentation.from(DoctorTransportDiagnosis.HubOffline, report).action)
        assertEquals(DoctorActionKind.CONNECT_AI, ForkDoctorPresentation.from(DoctorTransportDiagnosis.AiDisconnected("ai_no_active_account"), report).action)
        val waiting = ForkDoctorPresentation.from(DoctorTransportDiagnosis.WaitingUserApproval(2), report)
        assertEquals(DoctorActionKind.REVIEW_WAITING, waiting.action)
        assertTrue(waiting.message.contains("não está travado"))
    }

    @Test fun executionDoesNotInventAMachineLabel() {
        val inactive = ForkTowerPresentation.from(snapshot(card(active = false, machine = "Não divulgar"))).cards.single()
        assertNull(inactive.machineLabel)
        val active = ForkTowerPresentation.from(snapshot(card(active = true, machine = "runner.example.invalid"))).cards.single()
        assertEquals("runner.example.invalid", active.machineLabel)
        assertEquals("review", active.card.state)
    }

    @Test fun actionsAreFailClosedByCapabilities() {
        val presentation = ForkTowerPresentation.from(snapshot(card(canAccept = false, canInterrupt = false)))
        assertFalse(presentation.cards.single().canAccept)
        assertFalse(presentation.cards.single().canInterrupt)
    }

    @Test fun fixtureStoreRejectsInvalidationFromAnOldGeneration() {
        val store = ForkTowerFixtureStore()
        val old = store.capturedGeneration()
        store.changeContext(ForkTowerFixtureCatalog.tower.copy(spaceId = "new-space.example.invalid"))
        val event = TowerInvalidationEvent(TowerInvalidationEventType.DONNA_CONTROL_TOWER_CHANGED, ForkTowerFixtureCatalog.hubId, ForkTowerFixtureCatalog.spaceId)
        assertFalse(store.acceptInvalidation(event, old))
        assertNull(store.current(old))
    }

    private fun queue(gaps: List<CoverageGap> = emptyList(), resolved: List<ResolvedAttentionItem> = emptyList()) =
        AttentionQueue(1, "hub.example.invalid", "space.example.invalid", 1L, emptyList(), resolved, gaps)

    private fun resolved() = ResolvedAttentionItem("resolved.example.invalid", AttentionKind.TASK_REVIEW, "review", null, 1L,
        com.codync.android.core.fork.AttentionResolutionKind.ACCEPTED, AttentionTarget())

    private fun report() = HubDoctorReport(1, "hub.example.invalid", "space.example.invalid", 1L, DoctorOverallStatus.WAITING_USER,
        listOf(DoctorCheck("approval.example.invalid", DoctorArea.APPROVALS, DoctorCheckStatus.WARNING, "approvals_waiting", 2, DoctorActionKind.REVIEW_WAITING)))

    private fun card(active: Boolean = false, machine: String? = null, canAccept: Boolean = true, canInterrupt: Boolean = false) = WorkCard(
        "task.example.invalid", "project.example.invalid", "Projeto fictício", "space.example.invalid", title = "Tarefa fictícia", state = "review",
        bucket = WorkBucket.NEEDS_YOU, origin = WorkOrigin(WorkOriginKind.OWNER_DIRECT), requesterLabel = WorkRequesterLabel.OWNER,
        blocker = WorkBlocker(WorkBlockerKind.NONE), delivery = WorkDelivery(true, null, false), declaredUpdatedAt = 1L,
        observedExecution = ObservedExecution(active, ObservedExecutionStatus.IDLE, machineLabel = machine),
        capabilities = WorkCapabilities(canInterrupt, canAccept))

    private fun snapshot(card: WorkCard) = TowerViewSnapshot(1, "hub.example.invalid", "space.example.invalid", TowerPerspective.OVERVIEW,
        collectedAt = 1L, cards = listOf(card), counts = TowerViewCounts(1, 0, 0, 1))
}

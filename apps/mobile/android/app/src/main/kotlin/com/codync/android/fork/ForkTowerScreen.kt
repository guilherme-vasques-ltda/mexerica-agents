package com.codync.android.fork

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.codync.android.R
import com.codync.android.core.fork.AttentionActionKind
import com.codync.android.core.fork.AttentionKind
import com.codync.android.core.fork.AttentionItem
import com.codync.android.core.fork.AttentionQueue
import com.codync.android.core.fork.AttentionSeverity
import com.codync.android.core.fork.DoctorActionKind
import com.codync.android.core.fork.DoctorArea
import com.codync.android.core.fork.DoctorCheck
import com.codync.android.core.fork.DoctorCheckStatus
import com.codync.android.core.fork.DoctorOverallStatus
import com.codync.android.core.fork.DoctorTransportDiagnosis
import com.codync.android.core.fork.HubDoctorReport
import com.codync.android.core.fork.Observation
import com.codync.android.core.fork.ObservedExecutionStatus
import com.codync.android.core.fork.ResolvedAttentionItem
import com.codync.android.core.fork.TowerInvalidationEvent
import com.codync.android.core.fork.TowerInvalidationGuard
import com.codync.android.core.fork.TowerPerspective
import com.codync.android.core.fork.TowerViewSnapshot
import com.codync.android.core.fork.WorkBlockerKind
import com.codync.android.core.fork.WorkBucket
import com.codync.android.core.fork.WorkCard
import com.codync.android.core.fork.WorkOriginKind
import com.codync.android.core.fork.WorkRequesterLabel
import com.codync.android.core.fork.WorkOrigin
import com.codync.android.core.fork.WorkBlocker
import com.codync.android.core.fork.WorkDelivery
import com.codync.android.core.fork.WorkCapabilities
import com.codync.android.core.fork.ObservedExecution
import com.codync.android.core.fork.TowerViewCounts

data class ForkAttentionPresentation(
    val available: Boolean,
    val items: List<AttentionItem>,
    val gaps: List<String>,
    val resolvedToday: List<ResolvedAttentionItem>,
    val showEmpty: Boolean,
    val isResolvedTodayCollapsedByDefault: Boolean,
) {
    companion object {
        fun from(observation: Observation<AttentionQueue>): ForkAttentionPresentation = when (observation) {
            Observation.Unavailable -> ForkAttentionPresentation(false, emptyList(), emptyList(), emptyList(), false, true)
            is Observation.Available -> observation.value.let { queue ->
                ForkAttentionPresentation(
                    true,
                    queue.normalizedItems(),
                    queue.coverageGaps.map { it.source },
                    queue.resolvedToday,
                    queue.items.isEmpty() && queue.coverageGaps.isEmpty(),
                    queue.isResolvedTodayCollapsedByDefault,
                )
            }
        }
    }
}

data class ForkDoctorCheckPresentation(
    val area: Int,
    val status: Int,
    val count: Int?,
    val action: DoctorActionKind,
)

data class ForkDoctorPresentation(
    val title: Int,
    val message: Int,
    val action: DoctorActionKind,
    val checks: List<ForkDoctorCheckPresentation>,
) {
    companion object {
        fun from(diagnosis: DoctorTransportDiagnosis, observation: Observation<HubDoctorReport>): ForkDoctorPresentation {
            val (title, message) = when (diagnosis) {
                DoctorTransportDiagnosis.HubOffline -> R.string.fork_doctor_offline_title to R.string.fork_doctor_offline_message
                is DoctorTransportDiagnosis.AiDisconnected -> R.string.fork_doctor_ai_title to R.string.fork_doctor_ai_message
                is DoctorTransportDiagnosis.WaitingUserApproval -> R.string.fork_doctor_waiting_title to R.string.fork_doctor_waiting_message
                is DoctorTransportDiagnosis.Degraded -> R.string.fork_doctor_degraded_title to R.string.fork_doctor_degraded_message
                DoctorTransportDiagnosis.Healthy -> R.string.fork_doctor_healthy_title to R.string.fork_doctor_healthy_message
                DoctorTransportDiagnosis.Unavailable -> R.string.fork_doctor_unavailable_title to R.string.fork_doctor_unavailable_message
            }
            val checks = (observation as? Observation.Available)?.value?.checks.orEmpty().map {
                ForkDoctorCheckPresentation(it.area.labelKey(), it.status.labelKey(), it.count, it.actionKind)
            }
            return ForkDoctorPresentation(title, message, diagnosis.actionKind, checks)
        }
    }
}

data class ForkTowerCardPresentation(
    val card: WorkCard,
    val machineLabel: String?,
) {
    val canAccept: Boolean get() = card.capabilities.canOwnerAccept
    val canInterrupt: Boolean get() = card.capabilities.canDirectorInterrupt
}

data class ForkTowerPresentation(
    val perspective: TowerPerspective,
    val counts: com.codync.android.core.fork.TowerViewCounts,
    val cards: List<ForkTowerCardPresentation>,
) {
    companion object {
        fun from(snapshot: TowerViewSnapshot) = ForkTowerPresentation(
            snapshot.perspective,
            snapshot.counts,
            snapshot.cards.map { card -> ForkTowerCardPresentation(card, card.observedExecution.machineLabel.takeIf { card.observedExecution.active }) },
        )
    }
}

class ForkTowerFixtureStore(
    private val guard: TowerInvalidationGuard = TowerInvalidationGuard(),
) {
    private val lock = Any()
    private var snapshot: TowerViewSnapshot = ForkTowerFixtureCatalog.tower
    private var generation: Long = guard.bindContext(snapshot.hubId, snapshot.spaceId)

    fun current(capturedGeneration: Long): TowerViewSnapshot? = synchronized(lock) {
        snapshot.takeIf { capturedGeneration == generation }
    }

    fun capturedGeneration(): Long = synchronized(lock) { generation }

    fun acceptInvalidation(event: TowerInvalidationEvent, capturedGeneration: Long): Boolean =
        synchronized(lock) { guard.shouldAccept(event, capturedGeneration) }

    fun changeContext(snapshot: TowerViewSnapshot, capturedGeneration: Long): Boolean = synchronized(lock) {
        if (capturedGeneration != generation) return@synchronized false
        generation = guard.bindContext(snapshot.hubId, snapshot.spaceId)
        this.snapshot = snapshot
        true
    }
}

object ForkTowerFixtureCatalog {
    const val hubId = "hub.example.invalid"
    const val spaceId = "space.example.invalid"
    val attention = Observation.Available(AttentionQueue(1, hubId, spaceId, 100L, emptyList(), emptyList(), emptyList()))
    val doctor = Observation.Available(
        HubDoctorReport(1, hubId, spaceId, 100L, DoctorOverallStatus.WAITING_USER,
            listOf(DoctorCheck("check.example.invalid", DoctorArea.APPROVALS, DoctorCheckStatus.WARNING, "approvals_waiting", 1, DoctorActionKind.REVIEW_WAITING))),
    )
    val attentionWithGap = Observation.Available(
        AttentionQueue(1, hubId, spaceId, 100L, emptyList(), emptyList(), listOf(
            com.codync.android.core.fork.CoverageGap("doctor", com.codync.android.core.fork.CoverageGapCode.SOURCE_UNAVAILABLE),
        )),
    )
    val tower = TowerViewSnapshot(1, hubId, spaceId, TowerPerspective.OVERVIEW, collectedAt = 100L,
        cards = listOf(
            WorkCard("task.example.invalid", "project.example.invalid", "Projeto fictício", spaceId, title = "Revisar entrega", state = "review", bucket = WorkBucket.NEEDS_YOU,
                origin = WorkOrigin(WorkOriginKind.OWNER_DIRECT), requesterLabel = WorkRequesterLabel.OWNER,
                blocker = WorkBlocker(WorkBlockerKind.NONE), delivery = WorkDelivery(true, "Evidência de exemplo", false), declaredUpdatedAt = 90L,
                observedExecution = ObservedExecution(false, ObservedExecutionStatus.IDLE),
                capabilities = WorkCapabilities(false, true)),
        ), counts = TowerViewCounts(1, 0, 0, 1))
}

@Composable
fun ForkAttentionQueueSection(observation: Observation<AttentionQueue>, onAction: (AttentionItem) -> Unit, modifier: Modifier = Modifier) {
    val presentation = ForkAttentionPresentation.from(observation)
    Column(modifier.verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(stringResource(R.string.fork_attention_title), style = MaterialTheme.typography.titleLarge)
        if (!presentation.available) Text(stringResource(R.string.fork_unavailable))
        presentation.gaps.forEach { Text(stringResource(R.string.fork_attention_gap, it), color = MaterialTheme.colorScheme.error) }
        if (presentation.available && presentation.showEmpty) Text(stringResource(R.string.fork_attention_empty))
        presentation.items.forEach { item ->
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("${stringResource(item.severity.labelKey())} · ${stringResource(item.kind.labelKey())}", style = MaterialTheme.typography.labelLarge)
                    Text(item.summary.orEmpty())
                    Button(onClick = { onAction(item) }) { Text(stringResource(item.actionKind.labelKey())) }
                }
            }
        }
        if (presentation.resolvedToday.isNotEmpty()) {
            var collapsed by remember { mutableStateOf(presentation.isResolvedTodayCollapsedByDefault) }
            OutlinedButton(onClick = { collapsed = !collapsed }) { Text(stringResource(if (collapsed) R.string.fork_resolved_today else R.string.fork_hide_resolved)) }
            if (!collapsed) presentation.resolvedToday.forEach { Text(it.summary.orEmpty()) }
        }
    }
}

@Composable
fun ForkHubDoctorSection(diagnosis: DoctorTransportDiagnosis, observation: Observation<HubDoctorReport>, onAction: (DoctorActionKind) -> Unit, modifier: Modifier = Modifier) {
    val presentation = ForkDoctorPresentation.from(diagnosis, observation)
    Column(modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(stringResource(R.string.fork_doctor_title), style = MaterialTheme.typography.titleLarge)
        Text(stringResource(presentation.title), style = MaterialTheme.typography.titleMedium)
        Text(stringResource(presentation.message))
        if (presentation.action != DoctorActionKind.NONE) Button(onClick = { onAction(presentation.action) }) { Text(stringResource(presentation.action.labelKey())) }
        if (observation === Observation.Unavailable) Text(stringResource(R.string.fork_unavailable))
        presentation.checks.forEach { check ->
            val count = check.count?.let { " · $it" }.orEmpty()
            Text(stringResource(R.string.fork_doctor_check, stringResource(check.area), stringResource(check.status), count))
            if (check.action != DoctorActionKind.NONE) Text(stringResource(R.string.fork_recommended_action, stringResource(check.action.labelKey())))
        }
    }
}

@Composable
fun ForkTowerScreen(
    snapshot: TowerViewSnapshot,
    onPerspectiveSelected: (TowerPerspective) -> Unit,
    onOwnerAccept: (WorkCard) -> Unit,
    onDirectorInterrupt: (WorkCard) -> Unit,
    modifier: Modifier = Modifier,
) {
    var selected by remember(snapshot.spaceId) { mutableStateOf(snapshot.perspective) }
    val projection = snapshot.takeIf { it.perspective == selected }
    Column(modifier.verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text(stringResource(R.string.fork_tower_title), style = MaterialTheme.typography.titleLarge)
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            listOf(TowerPerspective.OVERVIEW to R.string.fork_perspective_overview, TowerPerspective.DIRECTOR to R.string.fork_perspective_director, TowerPerspective.BOT to R.string.fork_perspective_bot).forEach { (perspective, label) ->
                if (selected == perspective) Text("• ${stringResource(label)}") else OutlinedButton(onClick = {
                    selected = perspective
                    onPerspectiveSelected(perspective)
                }) { Text(stringResource(label)) }
            }
        }
        if (projection == null) {
            Text(stringResource(R.string.fork_perspective_not_loaded))
            return@Column
        }
        val presentation = ForkTowerPresentation.from(projection)
        Text(stringResource(R.string.fork_tower_counts, projection.counts.needsYou, projection.counts.observedWorking, projection.counts.waitingOrBlocked, projection.counts.totalVisible))
        presentation.cards.forEach { item ->
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(item.card.title, style = MaterialTheme.typography.titleMedium)
                    Text("${item.card.projectName} · ${stringResource(item.card.bucket.labelKey())} · ${stringResource(item.card.origin.kind.labelKey())}")
                    if (item.card.blocker.kind != WorkBlockerKind.NONE) Text(stringResource(R.string.fork_blocker, stringResource(item.card.blocker.kind.labelKey()), if (item.card.blocker.kind == WorkBlockerKind.PERMISSION_PAUSE) " · ${item.card.blocker.attentionItemId.orEmpty()}" else ""))
                    Text(stringResource(R.string.fork_delivery, stringResource(if (item.card.delivery.hasEvidence) R.string.fork_has_evidence else R.string.fork_no_evidence), stringResource(if (item.card.delivery.acceptedByOwner) R.string.fork_accepted else R.string.fork_not_accepted)))
                    Text(stringResource(R.string.fork_declared_observed, item.card.state, stringResource(item.card.observedExecution.status.labelKey())))
                    item.machineLabel?.let { Text(stringResource(R.string.fork_machine, it)) }
                    if (item.canAccept) Button(onClick = { onOwnerAccept(item.card) }) { Text(stringResource(R.string.fork_accept_delivery)) }
                    if (item.canInterrupt) OutlinedButton(onClick = { onDirectorInterrupt(item.card) }) { Text(stringResource(R.string.fork_interrupt)) }
                }
            }
        }
    }
}

private fun AttentionSeverity.labelKey() = when (this) { AttentionSeverity.BLOCKING -> R.string.fork_severity_blocking; AttentionSeverity.ACTION_REQUIRED -> R.string.fork_severity_action_required; AttentionSeverity.WARNING -> R.string.fork_severity_warning }
private fun AttentionKind.labelKey() = when (this) {
    AttentionKind.PERMISSION -> R.string.fork_attention_kind_permission
    AttentionKind.DELEGATED_PERMISSION -> R.string.fork_attention_kind_delegated_permission
    AttentionKind.OWNER_QUESTION -> R.string.fork_attention_kind_owner_question
    AttentionKind.TASK_PROPOSAL -> R.string.fork_attention_kind_task_proposal
    AttentionKind.TASK_REVIEW -> R.string.fork_attention_kind_task_review
    AttentionKind.ROUTINE_REVIEW -> R.string.fork_attention_kind_routine_review
    AttentionKind.DOCTOR_ALERT -> R.string.fork_attention_kind_doctor_alert
}
private fun AttentionActionKind.labelKey() = when (this) {
    AttentionActionKind.RESPOND_PERMISSION -> R.string.fork_attention_action_respond_permission
    AttentionActionKind.ANSWER_QUESTION -> R.string.fork_attention_action_answer_question
    AttentionActionKind.DECIDE_PROPOSAL -> R.string.fork_attention_action_decide_proposal
    AttentionActionKind.REVIEW_TASK -> R.string.fork_attention_action_review_task
    AttentionActionKind.REVIEW_ROUTINE -> R.string.fork_attention_action_review_routine
    AttentionActionKind.OPEN_CONVERSATION -> R.string.fork_attention_action_open_conversation
    AttentionActionKind.OPEN_DOCTOR -> R.string.fork_attention_action_open_doctor
}
private fun DoctorActionKind.labelKey() = when (this) {
    DoctorActionKind.NONE -> R.string.fork_action_none
    DoctorActionKind.START_HUB -> R.string.fork_action_start_hub
    DoctorActionKind.CONNECT_AI -> R.string.fork_action_connect_ai
    DoctorActionKind.REVIEW_WAITING -> R.string.fork_action_review_waiting
    DoctorActionKind.CHECK_CHANNELS -> R.string.fork_action_check_channels
    DoctorActionKind.CHECK_MCPS -> R.string.fork_action_check_mcps
    DoctorActionKind.CHECK_ROUTINES -> R.string.fork_action_check_routines
    DoctorActionKind.OPEN_TORRE -> R.string.fork_action_open_tower
}
private fun DoctorArea.labelKey() = when (this) {
    DoctorArea.HUB -> R.string.fork_area_hub
    DoctorArea.AI_ENGINE -> R.string.fork_area_ai_engine
    DoctorArea.APPROVALS -> R.string.fork_area_approvals
    DoctorArea.CHANNELS -> R.string.fork_area_channels
    DoctorArea.MCPS -> R.string.fork_area_mcps
    DoctorArea.ROUTINES -> R.string.fork_area_routines
    DoctorArea.DELEGATION -> R.string.fork_area_delegation
}
private fun DoctorCheckStatus.labelKey() = when (this) {
    DoctorCheckStatus.OK -> R.string.fork_status_ok
    DoctorCheckStatus.WARNING -> R.string.fork_status_warning
    DoctorCheckStatus.ERROR -> R.string.fork_status_error
    DoctorCheckStatus.UNAVAILABLE -> R.string.fork_status_unavailable
}
private fun WorkBucket.labelKey() = when (this) {
    WorkBucket.NEEDS_YOU -> R.string.fork_bucket_needs_you
    WorkBucket.OBSERVED_WORKING -> R.string.fork_bucket_observed_working
    WorkBucket.WAITING_OR_BLOCKED -> R.string.fork_bucket_waiting_or_blocked
    WorkBucket.BACKLOG -> R.string.fork_bucket_backlog
    WorkBucket.DONE -> R.string.fork_bucket_done
}
private fun WorkOriginKind.labelKey() = when (this) {
    WorkOriginKind.OWNER_DIRECT -> R.string.fork_origin_owner_direct
    WorkOriginKind.DELEGATED -> R.string.fork_origin_delegated
    WorkOriginKind.ROUTINE -> R.string.fork_origin_routine
    WorkOriginKind.UNKNOWN -> R.string.fork_origin_unknown
}
private fun WorkBlockerKind.labelKey() = when (this) {
    WorkBlockerKind.PERMISSION_PAUSE -> R.string.fork_blocker_permission_pause
    WorkBlockerKind.OWNER_DECISION -> R.string.fork_blocker_owner_decision
    WorkBlockerKind.DEPENDENCY -> R.string.fork_blocker_dependency
    WorkBlockerKind.MANUAL -> R.string.fork_blocker_manual
    WorkBlockerKind.NONE -> R.string.fork_blocker_none
}
private fun ObservedExecutionStatus.labelKey() = when (this) {
    ObservedExecutionStatus.IDLE -> R.string.fork_execution_idle
    ObservedExecutionStatus.WORKING -> R.string.fork_execution_working
    ObservedExecutionStatus.NEEDS_INPUT -> R.string.fork_execution_needs_input
    ObservedExecutionStatus.WAITING -> R.string.fork_execution_waiting
    ObservedExecutionStatus.ERROR -> R.string.fork_execution_error
    ObservedExecutionStatus.UNAVAILABLE -> R.string.fork_execution_unavailable
}

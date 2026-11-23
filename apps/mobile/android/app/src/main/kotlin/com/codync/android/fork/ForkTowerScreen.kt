package com.codync.android.fork

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
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
import androidx.compose.ui.unit.dp
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
    val area: String,
    val status: String,
    val count: Int?,
    val action: DoctorActionKind,
)

data class ForkDoctorPresentation(
    val title: String,
    val message: String,
    val action: DoctorActionKind,
    val checks: List<ForkDoctorCheckPresentation>,
) {
    companion object {
        fun from(diagnosis: DoctorTransportDiagnosis, observation: Observation<HubDoctorReport>): ForkDoctorPresentation {
            val (title, message) = when (diagnosis) {
                DoctorTransportDiagnosis.HubOffline -> "Hub inacessível" to "Não foi possível alcançar o Hub."
                is DoctorTransportDiagnosis.AiDisconnected -> "IA desconectada ou pausada" to "Conecte uma conta de IA para continuar."
                is DoctorTransportDiagnosis.WaitingUserApproval -> "Aguardando sua aprovação" to "O sistema não está travado; aguarda sua decisão."
                is DoctorTransportDiagnosis.Degraded -> "Hub precisa de atenção" to "Há verificações com aviso ou erro."
                DoctorTransportDiagnosis.Healthy -> "Tudo funcionando" to "O Hub informou estado saudável."
                DoctorTransportDiagnosis.Unavailable -> "Não foi possível carregar agora" to "O diagnóstico está indisponível."
            }
            val checks = (observation as? Observation.Available)?.value?.checks.orEmpty().map {
                ForkDoctorCheckPresentation(it.area.label(), it.status.label(), it.count, it.actionKind)
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
    private var snapshot: TowerViewSnapshot = ForkTowerFixtureCatalog.tower
    private var generation: Long = guard.bindContext(snapshot.hubId, snapshot.spaceId)

    fun current(capturedGeneration: Long): TowerViewSnapshot? = snapshot.takeIf { capturedGeneration == generation }

    fun capturedGeneration(): Long = generation

    fun acceptInvalidation(event: TowerInvalidationEvent, capturedGeneration: Long): Boolean =
        guard.shouldAccept(event, capturedGeneration)

    fun changeContext(snapshot: TowerViewSnapshot) {
        this.snapshot = snapshot
        generation = guard.bindContext(snapshot.hubId, snapshot.spaceId)
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
    Column(modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Esperando você", style = MaterialTheme.typography.titleLarge)
        if (!presentation.available) Text("Não foi possível carregar agora")
        presentation.gaps.forEach { Text("Não consegui ver $it", color = MaterialTheme.colorScheme.error) }
        if (presentation.available && presentation.showEmpty) Text("Nada esperando você")
        presentation.items.forEach { item ->
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text("${item.severity.label()} · ${item.kind.label()}", style = MaterialTheme.typography.labelLarge)
                    Text(item.summary.orEmpty())
                    Button(onClick = { onAction(item) }) { Text(item.actionKind.label()) }
                }
            }
        }
        if (presentation.resolvedToday.isNotEmpty()) {
            var collapsed by remember { mutableStateOf(presentation.isResolvedTodayCollapsedByDefault) }
            OutlinedButton(onClick = { collapsed = !collapsed }) { Text(if (collapsed) "Resolvidos hoje" else "Ocultar resolvidos") }
            if (!collapsed) presentation.resolvedToday.forEach { Text(it.summary.orEmpty()) }
        }
    }
}

@Composable
fun ForkHubDoctorSection(diagnosis: DoctorTransportDiagnosis, observation: Observation<HubDoctorReport>, onAction: (DoctorActionKind) -> Unit, modifier: Modifier = Modifier) {
    val presentation = ForkDoctorPresentation.from(diagnosis, observation)
    Column(modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Doctor do Hub", style = MaterialTheme.typography.titleLarge)
        Text(presentation.title, style = MaterialTheme.typography.titleMedium)
        Text(presentation.message)
        if (presentation.action != DoctorActionKind.NONE) Button(onClick = { onAction(presentation.action) }) { Text(presentation.action.label()) }
        if (observation === Observation.Unavailable) Text("Não foi possível carregar agora")
        presentation.checks.forEach { check ->
            Text("${check.area} · ${check.status}${check.count?.let { " · $it" }.orEmpty()}")
            if (check.action != DoctorActionKind.NONE) Text("Ação recomendada: ${check.action.label()}")
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
    Column(modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("Torre", style = MaterialTheme.typography.titleLarge)
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            listOf(TowerPerspective.OVERVIEW to "Dono & Berga", TowerPerspective.DIRECTOR to "Tanga", TowerPerspective.BOT to "Executor").forEach { (perspective, label) ->
                if (selected == perspective) Text("• $label") else OutlinedButton(onClick = {
                    selected = perspective
                    onPerspectiveSelected(perspective)
                }) { Text(label) }
            }
        }
        if (projection == null) {
            Text("Esta visão ainda não foi carregada.")
            return@Column
        }
        val presentation = ForkTowerPresentation.from(projection)
        Text("Precisam de você ${projection.counts.needsYou} · Em execução ${projection.counts.observedWorking} · Aguardando/bloqueados ${projection.counts.waitingOrBlocked} · Visíveis ${projection.counts.totalVisible}")
        presentation.cards.forEach { item ->
            Card(Modifier.fillMaxWidth()) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(item.card.title, style = MaterialTheme.typography.titleMedium)
                    Text("${item.card.projectName} · ${item.card.bucket.label()} · ${item.card.origin.kind.label()}")
                    if (item.card.blocker.kind != WorkBlockerKind.NONE) Text("Bloqueio: ${item.card.blocker.kind.label()}${if (item.card.blocker.kind == WorkBlockerKind.PERMISSION_PAUSE) " · ${item.card.blocker.attentionItemId.orEmpty()}" else ""}")
                    Text("Entrega: ${if (item.card.delivery.hasEvidence) "com evidência" else "sem evidência"} · ${if (item.card.delivery.acceptedByOwner) "aceita" else "não aceita"}")
                    Text("Estado declarado: ${item.card.state} · Execução observada: ${item.card.observedExecution.status.label()}")
                    item.machineLabel?.let { Text("Máquina: $it") }
                    if (item.canAccept) Button(onClick = { onOwnerAccept(item.card) }) { Text("Aceitar entrega") }
                    if (item.canInterrupt) OutlinedButton(onClick = { onDirectorInterrupt(item.card) }) { Text("Interromper") }
                }
            }
        }
    }
}

private fun AttentionSeverity.label() = when (this) { AttentionSeverity.BLOCKING -> "Bloqueante"; AttentionSeverity.ACTION_REQUIRED -> "Ação necessária"; AttentionSeverity.WARNING -> "Aviso" }
private fun AttentionKind.label() = name.lowercase().replace('_', ' ')
private fun AttentionActionKind.label() = name.lowercase().replace('_', ' ')
private fun DoctorActionKind.label() = name.lowercase().replace('_', ' ')
private fun DoctorArea.label() = name.lowercase().replace('_', ' ')
private fun DoctorCheckStatus.label() = name.lowercase()
private fun WorkBucket.label() = name.lowercase().replace('_', ' ')
private fun WorkOriginKind.label() = name.lowercase().replace('_', ' ')
private fun WorkBlockerKind.label() = name.lowercase().replace('_', ' ')
private fun ObservedExecutionStatus.label() = name.lowercase().replace('_', ' ')

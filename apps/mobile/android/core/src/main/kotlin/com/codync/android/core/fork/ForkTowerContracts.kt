package com.codync.android.core.fork

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.descriptors.buildClassSerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonDecoder
import kotlinx.serialization.json.JsonEncoder
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.put

val TowerContractsJson: Json = Json {
    ignoreUnknownKeys = true
    encodeDefaults = true
    explicitNulls = false
}

// MARK: - Fail-closed Observation<T>

@Serializable(with = ObservationSerializer::class)
sealed class Observation<out T> {
    data class Available<T>(
        val value: T,
        val source: String? = null,
        val coverage: String? = null,
    ) : Observation<T>()

    data object Unavailable : Observation<Nothing>()

    val isAvailable: Boolean
        get() = this is Available

    fun valueOrNull(): T? = (this as? Available)?.value
}

class ObservationSerializer<T>(
    private val valueSerializer: KSerializer<T>,
) : KSerializer<Observation<T>> {
    override val descriptor: SerialDescriptor = buildClassSerialDescriptor("Observation")

    override fun serialize(encoder: Encoder, value: Observation<T>) {
        val jsonEncoder = encoder as? JsonEncoder
            ?: error("ObservationSerializer only supports JSON encoding")
        val element = when (value) {
            is Observation.Available -> buildJsonObject {
                put("availability", "available")
                value.source?.let { put("source", it) }
                value.coverage?.let { put("coverage", it) }
                put("value", jsonEncoder.json.encodeToJsonElement(valueSerializer, value.value))
            }
            Observation.Unavailable -> buildJsonObject {
                put("availability", "unavailable")
            }
        }
        jsonEncoder.encodeJsonElement(element)
    }

    override fun deserialize(decoder: Decoder): Observation<T> {
        val jsonDecoder = decoder as? JsonDecoder
            ?: return Observation.Unavailable
        val element = runCatching { jsonDecoder.decodeJsonElement() }.getOrNull()
            ?: return Observation.Unavailable
        if (element is JsonNull || element !is JsonObject) {
            return Observation.Unavailable
        }
        val availability = (element["availability"] as? JsonPrimitive)?.contentOrNull
        if (availability != "available") {
            // Fail-closed: "unavailable" or unrecognized availability never fabricates data.
            return Observation.Unavailable
        }
        val rawValue = element["value"] ?: return Observation.Unavailable
        val decoded = runCatching {
            jsonDecoder.json.decodeFromJsonElement(valueSerializer, rawValue)
        }.getOrNull() ?: return Observation.Unavailable
        val source = (element["source"] as? JsonPrimitive)?.contentOrNull
        val coverage = (element["coverage"] as? JsonPrimitive)?.contentOrNull
        return Observation.Available(value = decoded, source = source, coverage = coverage)
    }
}

/**
 * Additive fields introduced on TowerSnapshot in MIRROR_SCHEMA 71+.
 * Older Hubs omit `attention` and `doctor`, which decode fail-closed as `Observation.Unavailable`.
 */
@Serializable
data class TowerSnapshotObservations(
    val attention: Observation<AttentionQueue> = Observation.Unavailable,
    val doctor: Observation<HubDoctorReport> = Observation.Unavailable,
)

// MARK: - RPC: donnaAttention

@Serializable
data class DonnaAttentionRequest(
    val spaceId: String,
    val includeResolvedToday: Boolean = true,
)

@Serializable
enum class AttentionKind {
    @SerialName("permission")
    PERMISSION,

    @SerialName("delegated_permission")
    DELEGATED_PERMISSION,

    @SerialName("owner_question")
    OWNER_QUESTION,

    @SerialName("task_proposal")
    TASK_PROPOSAL,

    @SerialName("task_review")
    TASK_REVIEW,

    @SerialName("routine_review")
    ROUTINE_REVIEW,

    @SerialName("doctor_alert")
    DOCTOR_ALERT,
}

@Serializable
enum class AttentionSeverity(val rank: Int) {
    @SerialName("warning")
    WARNING(1),

    @SerialName("action_required")
    ACTION_REQUIRED(2),

    @SerialName("blocking")
    BLOCKING(3),
}

@Serializable
enum class AttentionActionKind {
    @SerialName("respond_permission")
    RESPOND_PERMISSION,

    @SerialName("answer_question")
    ANSWER_QUESTION,

    @SerialName("decide_proposal")
    DECIDE_PROPOSAL,

    @SerialName("review_task")
    REVIEW_TASK,

    @SerialName("review_routine")
    REVIEW_ROUTINE,

    @SerialName("open_conversation")
    OPEN_CONVERSATION,

    @SerialName("open_doctor")
    OPEN_DOCTOR,
}

@Serializable
enum class AttentionApproverRole {
    @SerialName("owner")
    OWNER,
}

@Serializable
enum class AttentionResolutionKind {
    @SerialName("approved")
    APPROVED,

    @SerialName("rejected")
    REJECTED,

    @SerialName("answered")
    ANSWERED,

    @SerialName("accepted")
    ACCEPTED,

    @SerialName("reopened")
    REOPENED,

    @SerialName("kept")
    KEPT,

    @SerialName("cancelled")
    CANCELLED,

    @SerialName("recovered")
    RECOVERED,
}

@Serializable
enum class CoverageGapCode {
    @SerialName("source_unavailable")
    SOURCE_UNAVAILABLE,

    @SerialName("hub_outdated")
    HUB_OUTDATED,
}

@Serializable
data class AttentionConversationRef(
    val conversationId: String,
    val revision: Int,
)

@Serializable
data class AttentionTarget(
    val botId: String? = null,
    val requesterBotId: String? = null,
    val projectId: String? = null,
    val taskId: String? = null,
    val proposalId: String? = null,
    val routineId: String? = null,
    val entryId: String? = null,
    val conversation: AttentionConversationRef? = null,
    val sessionId: String? = null,
    val doctorCheckId: String? = null,
) {
    override fun toString(): String =
        "AttentionTarget(botId=$botId, taskId=$taskId, entryId=${if (entryId != null) "<redacted>" else "null"}, sessionId=${if (sessionId != null) "<redacted>" else "null"})"
}

@Serializable
data class AttentionItem(
    val id: String,
    val dedupeKey: String,
    val kind: AttentionKind,
    val severity: AttentionSeverity,
    val spaceId: String,
    val titleKey: String,
    val summary: String? = null,
    val actionKind: AttentionActionKind,
    val approverRole: AttentionApproverRole = AttentionApproverRole.OWNER,
    val createdAt: Long,
    val target: AttentionTarget,
) {
    override fun toString(): String =
        "AttentionItem(id=$id, kind=$kind, severity=$severity, spaceId=$spaceId, titleKey=$titleKey, summary=<redacted>)"
}

@Serializable
data class ResolvedAttentionItem(
    val id: String,
    val kind: AttentionKind,
    val titleKey: String,
    val summary: String? = null,
    val resolvedAt: Long,
    val resolutionKind: AttentionResolutionKind,
    val target: AttentionTarget,
) {
    override fun toString(): String =
        "ResolvedAttentionItem(id=$id, kind=$kind, resolutionKind=$resolutionKind, summary=<redacted>)"
}

@Serializable
data class CoverageGap(
    val source: String,
    val code: CoverageGapCode,
)

@Serializable
data class AttentionQueue(
    val version: Int,
    val hubId: String,
    val spaceId: String,
    val collectedAt: Long,
    val items: List<AttentionItem>,
    val resolvedToday: List<ResolvedAttentionItem>,
    val coverageGaps: List<CoverageGap>,
) {
    /**
     * Defensive deduplication by `dedupeKey` (`delegated_permission` suppresses duplicate `permission`)
     * and stable sort by severity (blocking > action_required > warning) then recency (createdAt desc).
     */
    fun normalizedItems(): List<AttentionItem> {
        val byKey = LinkedHashMap<String, AttentionItem>()
        for (item in items) {
            val existing = byKey[item.dedupeKey]
            if (existing == null || shouldReplace(existing, item)) {
                byKey[item.dedupeKey] = item
            }
        }
        return byKey.values.sortedWith(
            compareByDescending<AttentionItem> { it.severity.rank }
                .thenByDescending { it.createdAt }
                .thenBy { it.id },
        )
    }

    /** Count strictly reflects confirmed Hub items after defensive deduplication. */
    val pendingCount: Int
        get() = normalizedItems().size

    val hasCoverageGaps: Boolean
        get() = coverageGaps.isNotEmpty()

    /** UI contract: resolvedToday renders collapsed by default. */
    val isResolvedTodayCollapsedByDefault: Boolean
        get() = true

    override fun toString(): String =
        "AttentionQueue(version=$version, hubId=$hubId, spaceId=$spaceId, pendingCount=$pendingCount, resolvedToday=${resolvedToday.size}, coverageGaps=${coverageGaps.size})"

    private companion object {
        fun shouldReplace(existing: AttentionItem, candidate: AttentionItem): Boolean {
            if (existing.kind != candidate.kind) {
                if (candidate.kind == AttentionKind.DELEGATED_PERMISSION && existing.kind == AttentionKind.PERMISSION) {
                    return true
                }
                if (existing.kind == AttentionKind.DELEGATED_PERMISSION && candidate.kind == AttentionKind.PERMISSION) {
                    return false
                }
            }
            if (candidate.severity != existing.severity) {
                return candidate.severity.rank > existing.severity.rank
            }
            return candidate.createdAt > existing.createdAt
        }
    }
}

// MARK: - RPC: donnaHubDoctor

@Serializable
data class DonnaHubDoctorRequest(
    val spaceId: String,
)

@Serializable
enum class DoctorOverallStatus {
    @SerialName("healthy")
    HEALTHY,

    @SerialName("waiting_user")
    WAITING_USER,

    @SerialName("degraded")
    DEGRADED,

    @SerialName("error")
    ERROR,
}

@Serializable
enum class DoctorCheckStatus {
    @SerialName("ok")
    OK,

    @SerialName("warning")
    WARNING,

    @SerialName("error")
    ERROR,

    @SerialName("unavailable")
    UNAVAILABLE,
}

@Serializable
enum class DoctorArea {
    @SerialName("hub")
    HUB,

    @SerialName("ai_engine")
    AI_ENGINE,

    @SerialName("approvals")
    APPROVALS,

    @SerialName("channels")
    CHANNELS,

    @SerialName("mcps")
    MCPS,

    @SerialName("routines")
    ROUTINES,

    @SerialName("delegation")
    DELEGATION,
}

@Serializable
enum class DoctorActionKind {
    @SerialName("none")
    NONE,

    @SerialName("start_hub")
    START_HUB,

    @SerialName("connect_ai")
    CONNECT_AI,

    @SerialName("review_waiting")
    REVIEW_WAITING,

    @SerialName("check_channels")
    CHECK_CHANNELS,

    @SerialName("check_mcps")
    CHECK_MCPS,

    @SerialName("check_routines")
    CHECK_ROUTINES,

    @SerialName("open_torre")
    OPEN_TORRE,
}

@Serializable
data class DoctorCheck(
    val id: String,
    val area: DoctorArea,
    val status: DoctorCheckStatus,
    val code: String,
    val count: Int? = null,
    val actionKind: DoctorActionKind,
) {
    override fun toString(): String =
        "DoctorCheck(id=$id, area=$area, status=$status, code=$code, actionKind=$actionKind)"
}

@Serializable
data class HubDoctorReport(
    val version: Int,
    val hubId: String,
    val spaceId: String,
    val collectedAt: Long,
    val overall: DoctorOverallStatus,
    val checks: List<DoctorCheck>,
) {
    override fun toString(): String =
        "HubDoctorReport(version=$version, hubId=$hubId, spaceId=$spaceId, overall=$overall, checks=${checks.size})"
}

sealed interface DoctorTransportDiagnosis {
    val actionKind: DoctorActionKind

    data object HubOffline : DoctorTransportDiagnosis {
        override val actionKind: DoctorActionKind = DoctorActionKind.START_HUB
    }

    data class AiDisconnected(val code: String) : DoctorTransportDiagnosis {
        override val actionKind: DoctorActionKind = DoctorActionKind.CONNECT_AI
    }

    data class WaitingUserApproval(val pendingCount: Int?) : DoctorTransportDiagnosis {
        override val actionKind: DoctorActionKind = DoctorActionKind.REVIEW_WAITING
    }

    data class Degraded(val codes: List<String>) : DoctorTransportDiagnosis {
        override val actionKind: DoctorActionKind = DoctorActionKind.OPEN_TORRE
    }

    data object Healthy : DoctorTransportDiagnosis {
        override val actionKind: DoctorActionKind = DoctorActionKind.NONE
    }

    data object Unavailable : DoctorTransportDiagnosis {
        override val actionKind: DoctorActionKind = DoctorActionKind.NONE
    }

    companion object {
        fun diagnose(
            observation: Observation<HubDoctorReport>,
            hubReachable: Boolean,
        ): DoctorTransportDiagnosis {
            if (!hubReachable) return HubOffline
            val report = (observation as? Observation.Available)?.value ?: return Unavailable

            val aiCheck = report.checks.firstOrNull {
                it.area == DoctorArea.AI_ENGINE &&
                    (it.status == DoctorCheckStatus.ERROR ||
                        it.status == DoctorCheckStatus.WARNING ||
                        it.actionKind == DoctorActionKind.CONNECT_AI)
            }
            if (aiCheck != null) {
                return AiDisconnected(aiCheck.code)
            }

            if (report.overall == DoctorOverallStatus.WAITING_USER) {
                val approvalCount = report.checks.firstOrNull { it.area == DoctorArea.APPROVALS }?.count
                return WaitingUserApproval(approvalCount)
            }

            val waitingCheck = report.checks.firstOrNull {
                it.area == DoctorArea.APPROVALS &&
                    it.actionKind == DoctorActionKind.REVIEW_WAITING &&
                    (it.count ?: 0) > 0
            }
            if (waitingCheck != null) {
                return WaitingUserApproval(waitingCheck.count)
            }

            if (report.overall == DoctorOverallStatus.ERROR || report.overall == DoctorOverallStatus.DEGRADED) {
                val failingCodes = report.checks
                    .filter {
                        it.status == DoctorCheckStatus.ERROR ||
                            it.status == DoctorCheckStatus.WARNING ||
                            it.status == DoctorCheckStatus.UNAVAILABLE
                    }
                    .map { it.code }
                return Degraded(failingCodes)
            }

            return Healthy
        }
    }
}

// MARK: - RPC: donnaTowerView

@Serializable
data class DonnaTowerViewRequest(
    val spaceId: String,
    val perspective: TowerPerspective,
    val directorBotId: String? = null,
    val botId: String? = null,
)

@Serializable
enum class TowerPerspective {
    @SerialName("overview")
    OVERVIEW,

    @SerialName("director")
    DIRECTOR,

    @SerialName("bot")
    BOT,
}

@Serializable
enum class WorkOriginKind {
    @SerialName("owner_direct")
    OWNER_DIRECT,

    @SerialName("delegated")
    DELEGATED,

    @SerialName("routine")
    ROUTINE,

    @SerialName("unknown")
    UNKNOWN,
}

@Serializable
enum class WorkBucket {
    @SerialName("needs_you")
    NEEDS_YOU,

    @SerialName("observed_working")
    OBSERVED_WORKING,

    @SerialName("waiting_or_blocked")
    WAITING_OR_BLOCKED,

    @SerialName("backlog")
    BACKLOG,

    @SerialName("done")
    DONE,
}

@Serializable
enum class WorkRequesterLabel {
    @SerialName("owner")
    OWNER,

    @SerialName("bot")
    BOT,

    @SerialName("routine")
    ROUTINE,

    @SerialName("unknown")
    UNKNOWN,
}

@Serializable
enum class WorkBlockerKind {
    @SerialName("permission_pause")
    PERMISSION_PAUSE,

    @SerialName("owner_decision")
    OWNER_DECISION,

    @SerialName("dependency")
    DEPENDENCY,

    @SerialName("manual")
    MANUAL,

    @SerialName("none")
    NONE,
}

@Serializable
enum class ObservedExecutionStatus {
    @SerialName("idle")
    IDLE,

    @SerialName("working")
    WORKING,

    @SerialName("needsInput")
    NEEDS_INPUT,

    @SerialName("waiting")
    WAITING,

    @SerialName("error")
    ERROR,

    @SerialName("unavailable")
    UNAVAILABLE,
}

@Serializable
data class WorkOrigin(
    val kind: WorkOriginKind,
    val rootBotId: String? = null,
    val delegatedByBotId: String? = null,
    val routineId: String? = null,
    val admittedAt: Long? = null,
)

@Serializable
data class WorkBlocker(
    val kind: WorkBlockerKind,
    val reason: String? = null,
    val attentionItemId: String? = null,
) {
    override fun toString(): String =
        "WorkBlocker(kind=$kind, attentionItemId=$attentionItemId, reason=<redacted>)"
}

@Serializable
data class WorkDelivery(
    val hasEvidence: Boolean,
    val evidenceSummary: String? = null,
    val acceptedByOwner: Boolean,
) {
    override fun toString(): String =
        "WorkDelivery(hasEvidence=$hasEvidence, acceptedByOwner=$acceptedByOwner, evidenceSummary=<redacted>)"
}

@Serializable
data class ObservedExecution(
    val active: Boolean,
    val status: ObservedExecutionStatus,
    val observedAt: Long? = null,
    val machineLabel: String? = null,
)

@Serializable
data class WorkCapabilities(
    val canDirectorInterrupt: Boolean,
    val canOwnerAccept: Boolean,
)

@Serializable
data class WorkCard(
    val taskId: String,
    val projectId: String,
    val projectName: String,
    val spaceId: String,
    val parentTaskId: String? = null,
    val title: String,
    val state: String,
    val bucket: WorkBucket,
    val origin: WorkOrigin,
    val requesterLabel: WorkRequesterLabel,
    val requesterBotId: String? = null,
    val approverRole: AttentionApproverRole = AttentionApproverRole.OWNER,
    val directorBotId: String? = null,
    val assigneeBotId: String? = null,
    val nextAction: String? = null,
    val blocker: WorkBlocker,
    val delivery: WorkDelivery,
    val declaredUpdatedAt: Long,
    val observedExecution: ObservedExecution,
    val capabilities: WorkCapabilities,
) {
    override fun toString(): String =
        "WorkCard(taskId=$taskId, spaceId=$spaceId, state=$state, bucket=$bucket, title=<redacted>, nextAction=<redacted>)"
}

@Serializable
data class TowerViewCounts(
    val needsYou: Int,
    val observedWorking: Int,
    val waitingOrBlocked: Int,
    val totalVisible: Int,
)

@Serializable
data class TowerViewSnapshot(
    val version: Int,
    val hubId: String,
    val spaceId: String,
    val perspective: TowerPerspective,
    val directorBotId: String? = null,
    val botId: String? = null,
    val collectedAt: Long,
    val cards: List<WorkCard>,
    val counts: TowerViewCounts,
) {
    override fun toString(): String =
        "TowerViewSnapshot(version=$version, hubId=$hubId, spaceId=$spaceId, perspective=$perspective, visibleCards=${cards.size})"
}

// MARK: - Leak-free Invalidation Events & Context Guard

@Serializable
enum class TowerInvalidationEventType {
    @SerialName("donnaAttentionChanged")
    DONNA_ATTENTION_CHANGED,

    @SerialName("donnaDoctorChanged")
    DONNA_DOCTOR_CHANGED,

    @SerialName("donnaControlTowerChanged")
    DONNA_CONTROL_TOWER_CHANGED,

    @SerialName("donnaGovernanceChanged")
    DONNA_GOVERNANCE_CHANGED,
}

@Serializable
data class TowerInvalidationEvent(
    val type: TowerInvalidationEventType,
    val hubId: String,
    val spaceId: String,
)

object TowerInvalidationParser {
    fun parse(rawJson: String): TowerInvalidationEvent? {
        val event = runCatching {
            TowerContractsJson.decodeFromString<TowerInvalidationEvent>(rawJson)
        }.getOrNull() ?: return null
        if (event.hubId.isEmpty() || event.spaceId.isEmpty()) return null
        return event
    }
}

class TowerInvalidationGuard {
    var activeHubId: String? = null
        private set
    var activeSpaceId: String? = null
        private set
    var generation: Long = 0L
        private set

    fun bindContext(hubId: String, spaceId: String): Long {
        generation += 1L
        activeHubId = hubId
        activeSpaceId = spaceId
        return generation
    }

    fun resetForContextChange() {
        generation += 1L
        activeHubId = null
        activeSpaceId = null
    }

    fun shouldAccept(event: TowerInvalidationEvent, capturedGeneration: Long): Boolean {
        val hubId = activeHubId ?: return false
        val spaceId = activeSpaceId ?: return false
        if (capturedGeneration != generation) {
            return false
        }
        return event.hubId == hubId && event.spaceId == spaceId
    }
}

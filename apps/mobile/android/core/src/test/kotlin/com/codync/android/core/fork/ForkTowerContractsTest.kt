package com.codync.android.core.fork

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ForkTowerContractsTest {

    @Test
    fun decodesDonnaAttentionAndDeduplicatesByDedupeKey() {
        val json = """
            {
              "version": 1,
              "hubId": "hub-demo-1",
              "spaceId": "personal",
              "collectedAt": 1763900000000,
              "items": [
                {
                  "id": "att-perm-dup",
                  "dedupeKey": "permission:entry-42",
                  "kind": "permission",
                  "severity": "warning",
                  "spaceId": "personal",
                  "titleKey": "attention.permission.pending",
                  "summary": "Sensitive secret token check on hub.example.invalid",
                  "actionKind": "respond_permission",
                  "approverRole": "owner",
                  "createdAt": 1763900000100,
                  "target": {
                    "botId": "bot-worker",
                    "entryId": "entry-42"
                  }
                },
                {
                  "id": "att-delegated-42",
                  "dedupeKey": "permission:entry-42",
                  "kind": "delegated_permission",
                  "severity": "blocking",
                  "spaceId": "personal",
                  "titleKey": "attention.delegated_permission.pending",
                  "summary": "Delegated approval required",
                  "actionKind": "respond_permission",
                  "approverRole": "owner",
                  "createdAt": 1763900000200,
                  "target": {
                    "botId": "bot-worker",
                    "requesterBotId": "bot-director",
                    "entryId": "entry-42",
                    "sessionId": "sess-secret-99"
                  }
                },
                {
                  "id": "att-question-1",
                  "dedupeKey": "question:q-1",
                  "kind": "owner_question",
                  "severity": "action_required",
                  "spaceId": "personal",
                  "titleKey": "attention.owner_question.pending",
                  "summary": "Choose deploy window",
                  "actionKind": "answer_question",
                  "approverRole": "owner",
                  "createdAt": 1763900000500,
                  "target": {
                    "botId": "bot-lead",
                    "conversation": {
                      "conversationId": "conv-1",
                      "revision": 7
                    }
                  }
                },
                {
                  "id": "att-warning-2",
                  "dedupeKey": "doctor:ai-1",
                  "kind": "doctor_alert",
                  "severity": "warning",
                  "spaceId": "personal",
                  "titleKey": "attention.doctor.warning",
                  "summary": null,
                  "actionKind": "open_doctor",
                  "approverRole": "owner",
                  "createdAt": 1763900000900,
                  "target": {
                    "doctorCheckId": "chk-ai"
                  }
                }
              ],
              "resolvedToday": [
                {
                  "id": "res-1",
                  "kind": "task_review",
                  "titleKey": "attention.task_review.accepted",
                  "summary": "Private resolved summary",
                  "resolvedAt": 1763899000000,
                  "resolutionKind": "accepted",
                  "target": {
                    "projectId": "proj-1",
                    "taskId": "task-9"
                  }
                }
              ],
              "coverageGaps": [
                {
                  "source": "delegated_permissions",
                  "code": "source_unavailable"
                }
              ]
            }
        """.trimIndent()

        val queue = TowerContractsJson.decodeFromString<AttentionQueue>(json)
        assertEquals(1, queue.version)
        assertEquals("hub-demo-1", queue.hubId)
        assertEquals("personal", queue.spaceId)
        assertTrue(queue.isResolvedTodayCollapsedByDefault)
        assertTrue(queue.hasCoverageGaps)
        assertEquals(CoverageGapCode.SOURCE_UNAVAILABLE, queue.coverageGaps.first().code)
        assertEquals(AttentionResolutionKind.ACCEPTED, queue.resolvedToday.first().resolutionKind)

        val normalized = queue.normalizedItems()
        assertEquals(3, queue.pendingCount)
        assertEquals(listOf("att-delegated-42", "att-question-1", "att-warning-2"), normalized.map { it.id })
        assertEquals(AttentionKind.DELEGATED_PERMISSION, normalized[0].kind)
        assertEquals("bot-director", normalized[0].target.requesterBotId)
        assertEquals(7, normalized[1].target.conversation?.revision)
    }

    @Test
    fun observationDecodesAvailableUnavailableAndMissingFieldFailClosed() {
        val oldSnapshot = TowerContractsJson.decodeFromString<TowerSnapshotObservations>("{}")
        assertEquals(Observation.Unavailable, oldSnapshot.attention)
        assertEquals(Observation.Unavailable, oldSnapshot.doctor)

        val unavailableJson = """
            {
              "attention": { "availability": "unavailable" },
              "doctor": { "availability": "future_unknown_state" }
            }
        """.trimIndent()
        val unavailableSnapshot = TowerContractsJson.decodeFromString<TowerSnapshotObservations>(unavailableJson)
        assertFalse(unavailableSnapshot.attention.isAvailable)
        assertFalse(unavailableSnapshot.doctor.isAvailable)

        val availableDoctorJson = """
            {
              "doctor": {
                "availability": "available",
                "source": "hub_doctor",
                "coverage": "complete",
                "value": {
                  "version": 1,
                  "hubId": "hub-demo-1",
                  "spaceId": "personal",
                  "collectedAt": 1763900000000,
                  "overall": "healthy",
                  "checks": [
                    {
                      "id": "hub",
                      "area": "hub",
                      "status": "ok",
                      "code": "hub_ok",
                      "count": null,
                      "actionKind": "none"
                    }
                  ]
                }
              }
            }
        """.trimIndent()
        val availableSnapshot = TowerContractsJson.decodeFromString<TowerSnapshotObservations>(availableDoctorJson)
        assertEquals(Observation.Unavailable, availableSnapshot.attention)
        assertTrue(availableSnapshot.doctor.isAvailable)
        assertEquals(DoctorOverallStatus.HEALTHY, availableSnapshot.doctor.valueOrNull()?.overall)

        val malformedObservationJson = """
            {
              "attention": { "availability": "available" },
              "doctor": { "availability": "available", "value": { "broken": true } }
            }
        """.trimIndent()
        val malformedSnapshot = TowerContractsJson.decodeFromString<TowerSnapshotObservations>(malformedObservationJson)
        assertEquals(Observation.Unavailable, malformedSnapshot.attention)
        assertEquals(Observation.Unavailable, malformedSnapshot.doctor)
    }

    @Test
    fun attentionItemAndWorkCardDefaultApproverRoleWhenOmitted() {
        val itemJson = """
            {
              "id": "att-no-role",
              "dedupeKey": "question:q-2",
              "kind": "owner_question",
              "severity": "action_required",
              "spaceId": "personal",
              "titleKey": "attention.owner_question.pending",
              "summary": null,
              "actionKind": "answer_question",
              "createdAt": 1763900000500,
              "target": {}
            }
        """.trimIndent()
        val item = TowerContractsJson.decodeFromString<AttentionItem>(itemJson)
        assertEquals(AttentionApproverRole.OWNER, item.approverRole)

        val cardJson = """
            {
              "taskId": "task-no-role",
              "projectId": "proj-1",
              "projectName": "Portal",
              "spaceId": "personal",
              "title": "Check defaults",
              "state": "backlog",
              "bucket": "backlog",
              "origin": { "kind": "owner_direct" },
              "requesterLabel": "owner",
              "blocker": { "kind": "none" },
              "delivery": { "hasEvidence": false, "acceptedByOwner": false },
              "declaredUpdatedAt": 1763900000000,
              "observedExecution": { "active": false, "status": "idle" },
              "capabilities": { "canDirectorInterrupt": false, "canOwnerAccept": false }
            }
        """.trimIndent()
        val card = TowerContractsJson.decodeFromString<WorkCard>(cardJson)
        assertEquals(AttentionApproverRole.OWNER, card.approverRole)
    }

    @Test
    fun doctorReportDistinguishesHubOfflineAiDisconnectedAndWaitingApproval() {
        val aiDisconnectedJson = """
            {
              "version": 1,
              "hubId": "hub-demo-1",
              "spaceId": "personal",
              "collectedAt": 1763900000000,
              "overall": "error",
              "checks": [
                {
                  "id": "chk-hub",
                  "area": "hub",
                  "status": "ok",
                  "code": "hub_ok",
                  "count": null,
                  "actionKind": "none"
                },
                {
                  "id": "chk-ai",
                  "area": "ai_engine",
                  "status": "error",
                  "code": "ai_no_active_account",
                  "count": 0,
                  "actionKind": "connect_ai"
                }
              ]
            }
        """.trimIndent()
        val aiReport = TowerContractsJson.decodeFromString<HubDoctorReport>(aiDisconnectedJson)

        val offlineDiag = DoctorTransportDiagnosis.diagnose(Observation.Available(aiReport), hubReachable = false)
        assertEquals(DoctorTransportDiagnosis.HubOffline, offlineDiag)
        assertEquals(DoctorActionKind.START_HUB, offlineDiag.actionKind)

        val aiDiag = DoctorTransportDiagnosis.diagnose(Observation.Available(aiReport), hubReachable = true)
        assertEquals(DoctorTransportDiagnosis.AiDisconnected("ai_no_active_account"), aiDiag)
        assertEquals(DoctorActionKind.CONNECT_AI, aiDiag.actionKind)

        assertEquals(
            DoctorTransportDiagnosis.Unavailable,
            DoctorTransportDiagnosis.diagnose(Observation.Unavailable, hubReachable = true),
        )

        val waitingJson = """
            {
              "version": 1,
              "hubId": "hub-demo-1",
              "spaceId": "personal",
              "collectedAt": 1763900000000,
              "overall": "waiting_user",
              "checks": [
                {
                  "id": "chk-hub",
                  "area": "hub",
                  "status": "ok",
                  "code": "hub_ok",
                  "count": null,
                  "actionKind": "none"
                },
                {
                  "id": "chk-ai",
                  "area": "ai_engine",
                  "status": "ok",
                  "code": "ai_ready",
                  "count": 1,
                  "actionKind": "none"
                },
                {
                  "id": "chk-approvals",
                  "area": "approvals",
                  "status": "warning",
                  "code": "approvals_waiting",
                  "count": 2,
                  "actionKind": "review_waiting"
                }
              ]
            }
        """.trimIndent()
        val waitingReport = TowerContractsJson.decodeFromString<HubDoctorReport>(waitingJson)
        val waitingDiag = DoctorTransportDiagnosis.diagnose(Observation.Available(waitingReport), hubReachable = true)
        assertEquals(DoctorTransportDiagnosis.WaitingUserApproval(2), waitingDiag)
        assertEquals(DoctorActionKind.REVIEW_WAITING, waitingDiag.actionKind)
    }

    @Test
    fun decodesDonnaTowerViewPerspectivesAndWorkCards() {
        val json = """
            {
              "version": 1,
              "hubId": "hub-demo-1",
              "spaceId": "team-alpha",
              "perspective": "director",
              "directorBotId": "bot-director-1",
              "botId": null,
              "collectedAt": 1763900000000,
              "cards": [
                {
                  "taskId": "task-101",
                  "projectId": "proj-1",
                  "projectName": "Portal",
                  "spaceId": "team-alpha",
                  "parentTaskId": null,
                  "title": "Secret customer migration",
                  "state": "in_progress",
                  "bucket": "needs_you",
                  "origin": {
                    "kind": "owner_direct",
                    "rootBotId": null,
                    "delegatedByBotId": null,
                    "routineId": null,
                    "admittedAt": 1763890000000
                  },
                  "requesterLabel": "owner",
                  "requesterBotId": null,
                  "approverRole": "owner",
                  "directorBotId": "bot-director-1",
                  "assigneeBotId": "bot-exec-1",
                  "nextAction": "Approve database step",
                  "blocker": {
                    "kind": "permission_pause",
                    "reason": "Private host path /var/secret/key",
                    "attentionItemId": "att-delegated-42"
                  },
                  "delivery": {
                    "hasEvidence": true,
                    "evidenceSummary": "Logs at staging.example.invalid",
                    "acceptedByOwner": false
                  },
                  "declaredUpdatedAt": 1763899500000,
                  "observedExecution": {
                    "active": true,
                    "status": "needsInput",
                    "observedAt": 1763899900000,
                    "machineLabel": "runner-demo-1"
                  },
                  "capabilities": {
                    "canDirectorInterrupt": false,
                    "canOwnerAccept": true
                  }
                }
              ],
              "counts": {
                "needsYou": 1,
                "observedWorking": 0,
                "waitingOrBlocked": 0,
                "totalVisible": 1
              }
            }
        """.trimIndent()

        val view = TowerContractsJson.decodeFromString<TowerViewSnapshot>(json)
        assertEquals(TowerPerspective.DIRECTOR, view.perspective)
        assertEquals("bot-director-1", view.directorBotId)
        assertEquals(1, view.counts.needsYou)
        assertEquals(1, view.counts.totalVisible)

        val card = view.cards.first()
        assertEquals(WorkOriginKind.OWNER_DIRECT, card.origin.kind)
        assertEquals(WorkBlockerKind.PERMISSION_PAUSE, card.blocker.kind)
        assertEquals(ObservedExecutionStatus.NEEDS_INPUT, card.observedExecution.status)
        assertFalse(card.capabilities.canDirectorInterrupt)
        assertTrue(card.capabilities.canOwnerAccept)
    }

    @Test
    fun invalidationGuardRejectsCrossContextAndStaleGeneration() {
        val validEvent = TowerInvalidationParser.parse("""{"type":"donnaAttentionChanged","hubId":"hub-1","spaceId":"personal"}""")
        val crossSpaceEvent = TowerInvalidationParser.parse("""{"type":"donnaDoctorChanged","hubId":"hub-1","spaceId":"work"}""")
        val crossHubEvent = TowerInvalidationParser.parse("""{"type":"donnaControlTowerChanged","hubId":"hub-2","spaceId":"personal"}""")
        val unknownEvent = TowerInvalidationParser.parse("""{"type":"unknownEvent","hubId":"hub-1","spaceId":"personal"}""")

        assertNotNull(validEvent)
        assertNotNull(crossSpaceEvent)
        assertNotNull(crossHubEvent)
        assertNull(unknownEvent)

        val guard = TowerInvalidationGuard()
        assertFalse(guard.shouldAccept(validEvent!!, capturedGeneration = guard.generation))

        val gen1 = guard.bindContext(hubId = "hub-1", spaceId = "personal")
        assertTrue(guard.shouldAccept(validEvent, capturedGeneration = gen1))
        assertFalse(guard.shouldAccept(crossSpaceEvent!!, capturedGeneration = gen1))
        assertFalse(guard.shouldAccept(crossHubEvent!!, capturedGeneration = gen1))

        guard.resetForContextChange()
        assertFalse(guard.shouldAccept(validEvent, capturedGeneration = gen1))
        assertFalse(guard.shouldAccept(validEvent, capturedGeneration = guard.generation))

        val gen2 = guard.bindContext(hubId = "hub-1", spaceId = "personal")
        assertNotEquals(gen1, gen2)
        assertFalse(guard.shouldAccept(validEvent, capturedGeneration = gen1))
        assertTrue(guard.shouldAccept(validEvent, capturedGeneration = gen2))

        val rebindGuard = TowerInvalidationGuard()
        val sameCtxEvent = TowerInvalidationParser.parse("""{"type":"donnaAttentionChanged","hubId":"hub-a","spaceId":"space-1"}""")!!
        val firstGen = rebindGuard.bindContext(hubId = "hub-a", spaceId = "space-1")
        rebindGuard.resetForContextChange()
        val secondGen = rebindGuard.bindContext(hubId = "hub-a", spaceId = "space-1")
        assertFalse(rebindGuard.shouldAccept(sameCtxEvent, capturedGeneration = firstGen))
        assertTrue(rebindGuard.shouldAccept(sameCtxEvent, capturedGeneration = secondGen))
    }

    @Test
    fun toStringRedactsSensitiveFields() {
        val item = AttentionItem(
            id = "att-1",
            dedupeKey = "permission:entry-secret",
            kind = AttentionKind.PERMISSION,
            severity = AttentionSeverity.BLOCKING,
            spaceId = "personal",
            titleKey = "attention.permission",
            summary = "SECRET_TOKEN_XYZ /Users/usuario/private.pem",
            actionKind = AttentionActionKind.RESPOND_PERMISSION,
            createdAt = 100L,
            target = AttentionTarget(botId = "bot-1", entryId = "entry-secret", sessionId = "sess-secret"),
        )
        val itemStr = item.toString()
        assertFalse(itemStr.contains("SECRET_TOKEN_XYZ"))
        assertFalse(itemStr.contains("/Users/usuario/private.pem"))
        assertFalse(itemStr.contains("entry-secret"))
        assertFalse(itemStr.contains("sess-secret"))
        assertTrue(itemStr.contains("<redacted>"))

        val card = WorkCard(
            taskId = "task-1",
            projectId = "proj-1",
            projectName = "Portal",
            spaceId = "personal",
            title = "PRIVATE_TITLE_ABC",
            state = "in_progress",
            bucket = WorkBucket.NEEDS_YOU,
            origin = WorkOrigin(kind = WorkOriginKind.OWNER_DIRECT),
            requesterLabel = WorkRequesterLabel.OWNER,
            nextAction = "PRIVATE_ACTION_DEF",
            blocker = WorkBlocker(kind = WorkBlockerKind.MANUAL, reason = "PRIVATE_REASON_GHI"),
            delivery = WorkDelivery(hasEvidence = true, evidenceSummary = "PRIVATE_EVIDENCE_JKL", acceptedByOwner = false),
            declaredUpdatedAt = 100L,
            observedExecution = ObservedExecution(active = false, status = ObservedExecutionStatus.IDLE),
            capabilities = WorkCapabilities(canDirectorInterrupt = false, canOwnerAccept = true),
        )
        assertFalse(card.toString().contains("PRIVATE_TITLE_ABC"))
        assertFalse(card.toString().contains("PRIVATE_ACTION_DEF"))
        assertFalse(card.blocker.toString().contains("PRIVATE_REASON_GHI"))
        assertFalse(card.delivery.toString().contains("PRIVATE_EVIDENCE_JKL"))
    }
}

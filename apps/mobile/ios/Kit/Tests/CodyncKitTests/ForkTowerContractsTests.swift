import Foundation
import Testing
@testable import CodyncKit

@Test func decodesDonnaAttentionAndDeduplicatesByDedupeKey() throws {
    let json = #"""
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
    """#

    let queue = try JSONDecoder().decode(AttentionQueue.self, from: Data(json.utf8))
    #expect(queue.version == 1)
    #expect(queue.hubId == "hub-demo-1")
    #expect(queue.spaceId == "personal")
    #expect(queue.isResolvedTodayCollapsedByDefault)
    #expect(queue.hasCoverageGaps)
    #expect(queue.coverageGaps.first?.code == .sourceUnavailable)
    #expect(queue.resolvedToday.first?.resolutionKind == .accepted)

    let normalized = queue.normalizedItems
    #expect(queue.pendingCount == 3)
    #expect(normalized.map(\.id) == ["att-delegated-42", "att-question-1", "att-warning-2"])
    #expect(normalized[0].kind == .delegatedPermission)
    #expect(normalized[0].target.requesterBotId == "bot-director")
    #expect(normalized[1].target.conversation?.revision == 7)
}

@Test func observationDecodesAvailableUnavailableAndMissingFieldFailClosed() throws {
    let oldHubJson = #"{}"#
    let oldSnapshot = try JSONDecoder().decode(TowerSnapshotObservations.self, from: Data(oldHubJson.utf8))
    #expect(oldSnapshot.attention == .unavailable)
    #expect(oldSnapshot.doctor == .unavailable)

    let unavailableJson = #"""
    {
      "attention": { "availability": "unavailable" },
      "doctor": { "availability": "future_unknown_state" }
    }
    """#
    let unavailableSnapshot = try JSONDecoder().decode(TowerSnapshotObservations.self, from: Data(unavailableJson.utf8))
    #expect(!unavailableSnapshot.attention.isAvailable)
    #expect(!unavailableSnapshot.doctor.isAvailable)

    let availableDoctorJson = #"""
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
    """#
    let availableSnapshot = try JSONDecoder().decode(TowerSnapshotObservations.self, from: Data(availableDoctorJson.utf8))
    #expect(availableSnapshot.attention == .unavailable)
    #expect(availableSnapshot.doctor.isAvailable)
    #expect(availableSnapshot.doctor.value?.overall == .healthy)

    let malformedObservationJson = #"""
    {
      "attention": { "availability": "available" },
      "doctor": { "availability": "available", "value": { "broken": true } }
    }
    """#
    let malformedSnapshot = try JSONDecoder().decode(TowerSnapshotObservations.self, from: Data(malformedObservationJson.utf8))
    #expect(malformedSnapshot.attention == .unavailable)
    #expect(malformedSnapshot.doctor == .unavailable)
}

@Test func attentionItemAndWorkCardDefaultApproverRoleWhenOmitted() throws {
    let itemJson = #"""
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
    """#
    let item = try JSONDecoder().decode(AttentionItem.self, from: Data(itemJson.utf8))
    #expect(item.approverRole == .owner)

    let cardJson = #"""
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
    """#
    let card = try JSONDecoder().decode(WorkCard.self, from: Data(cardJson.utf8))
    #expect(card.approverRole == .owner)
}

@Test func doctorReportDistinguishesHubOfflineAiDisconnectedAndWaitingApproval() throws {
    let aiDisconnectedJson = #"""
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
    """#
    let aiReport = try JSONDecoder().decode(HubDoctorReport.self, from: Data(aiDisconnectedJson.utf8))

    // Transport offline must report hubOffline, never AI error.
    let offlineDiag = DoctorTransportDiagnosis.diagnose(.available(value: aiReport), hubReachable: false)
    #expect(offlineDiag == .hubOffline)
    #expect(offlineDiag.actionKind == .startHub)

    // Reachable Hub with disconnected AI reports aiDisconnected.
    let aiDiag = DoctorTransportDiagnosis.diagnose(.available(value: aiReport), hubReachable: true)
    #expect(aiDiag == .aiDisconnected(code: "ai_no_active_account"))
    #expect(aiDiag.actionKind == .connectAI)

    // Unavailable observation stays unavailable (fail-closed, never healthy).
    #expect(DoctorTransportDiagnosis.diagnose(.unavailable, hubReachable: true) == .unavailable)

    let waitingJson = #"""
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
    """#
    let waitingReport = try JSONDecoder().decode(HubDoctorReport.self, from: Data(waitingJson.utf8))
    let waitingDiag = DoctorTransportDiagnosis.diagnose(.available(value: waitingReport), hubReachable: true)
    #expect(waitingDiag == .waitingUserApproval(pendingCount: 2))
    #expect(waitingDiag.actionKind == .reviewWaiting)
}

@Test func decodesDonnaTowerViewPerspectivesAndWorkCards() throws {
    let json = #"""
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
    """#

    let view = try JSONDecoder().decode(TowerViewSnapshot.self, from: Data(json.utf8))
    #expect(view.perspective == .director)
    #expect(view.directorBotId == "bot-director-1")
    #expect(view.counts.needsYou == 1)
    #expect(view.counts.totalVisible == 1)

    let card = try #require(view.cards.first)
    #expect(card.origin.kind == .ownerDirect)
    #expect(card.blocker.kind == .permissionPause)
    #expect(card.observedExecution.status == .needsInput)
    #expect(card.capabilities.canDirectorInterrupt == false)
    #expect(card.capabilities.canOwnerAccept == true)
}

@Test func invalidationGuardRejectsCrossContextAndStaleGeneration() throws {
    let validData = Data(#"{"type":"donnaAttentionChanged","hubId":"hub-1","spaceId":"personal"}"#.utf8)
    let crossSpaceData = Data(#"{"type":"donnaDoctorChanged","hubId":"hub-1","spaceId":"work"}"#.utf8)
    let crossHubData = Data(#"{"type":"donnaControlTowerChanged","hubId":"hub-2","spaceId":"personal"}"#.utf8)
    let unknownTypeData = Data(#"{"type":"unknownEvent","hubId":"hub-1","spaceId":"personal"}"#.utf8)

    let validEvent = TowerInvalidationParser.parse(validData)
    let crossSpaceEvent = TowerInvalidationParser.parse(crossSpaceData)
    let crossHubEvent = TowerInvalidationParser.parse(crossHubData)
    #expect(validEvent != nil)
    #expect(crossSpaceEvent != nil)
    #expect(crossHubEvent != nil)
    #expect(TowerInvalidationParser.parse(unknownTypeData) == nil)

    var guardState = TowerInvalidationGuard()
    // Unbound context rejects everything.
    #expect(!guardState.shouldAccept(validEvent!, capturedGeneration: guardState.generation))

    let gen1 = guardState.bindContext(hubId: "hub-1", spaceId: "personal")
    #expect(guardState.shouldAccept(validEvent!, capturedGeneration: gen1))
    #expect(!guardState.shouldAccept(crossSpaceEvent!, capturedGeneration: gen1))
    #expect(!guardState.shouldAccept(crossHubEvent!, capturedGeneration: gen1))

    // Resetting for workspace switch/revocation invalidates prior generation and clears context.
    guardState.resetForContextChange()
    #expect(!guardState.shouldAccept(validEvent!, capturedGeneration: gen1))
    #expect(!guardState.shouldAccept(validEvent!, capturedGeneration: guardState.generation))

    // Rebinding increments generation; old gen1 remains rejected.
    let gen2 = guardState.bindContext(hubId: "hub-1", spaceId: "personal")
    #expect(gen2 != gen1)
    #expect(!guardState.shouldAccept(validEvent!, capturedGeneration: gen1))
    #expect(guardState.shouldAccept(validEvent!, capturedGeneration: gen2))

    // Explicit same-context rebind check after resetForContextChange().
    var rebindGuard = TowerInvalidationGuard()
    let sameCtxEvent = try #require(
        TowerInvalidationParser.parse(Data(#"{"type":"donnaAttentionChanged","hubId":"hub-a","spaceId":"space-1"}"#.utf8))
    )
    let firstGen = rebindGuard.bindContext(hubId: "hub-a", spaceId: "space-1")
    rebindGuard.resetForContextChange()
    let secondGen = rebindGuard.bindContext(hubId: "hub-a", spaceId: "space-1")
    #expect(!rebindGuard.shouldAccept(sameCtxEvent, capturedGeneration: firstGen))
    #expect(rebindGuard.shouldAccept(sameCtxEvent, capturedGeneration: secondGen))
}

@Test func descriptionsRedactSensitiveFields() {
    let item = AttentionItem(
        id: "att-1",
        dedupeKey: "permission:entry-secret",
        kind: .permission,
        severity: .blocking,
        spaceId: "personal",
        titleKey: "attention.permission",
        summary: "SECRET_TOKEN_XYZ /Users/usuario/private.pem",
        actionKind: .respondPermission,
        createdAt: 100,
        target: AttentionTarget(botId: "bot-1", entryId: "entry-secret", sessionId: "sess-secret")
    )
    #expect(!item.description.contains("SECRET_TOKEN_XYZ"))
    #expect(!item.description.contains("/Users/usuario/private.pem"))
    #expect(!item.description.contains("entry-secret"))
    #expect(!item.description.contains("sess-secret"))
    #expect(item.description.contains("<redacted>"))

    let card = WorkCard(
        taskId: "task-1",
        projectId: "proj-1",
        projectName: "Portal",
        spaceId: "personal",
        title: "PRIVATE_TITLE_ABC",
        state: "in_progress",
        bucket: .needsYou,
        origin: WorkOrigin(kind: .ownerDirect),
        requesterLabel: .owner,
        nextAction: "PRIVATE_ACTION_DEF",
        blocker: WorkBlocker(kind: .manual, reason: "PRIVATE_REASON_GHI"),
        delivery: WorkDelivery(hasEvidence: true, evidenceSummary: "PRIVATE_EVIDENCE_JKL", acceptedByOwner: false),
        declaredUpdatedAt: 100,
        observedExecution: ObservedExecution(active: false, status: .idle),
        capabilities: WorkCapabilities(canDirectorInterrupt: false, canOwnerAccept: true)
    )
    #expect(!card.description.contains("PRIVATE_TITLE_ABC"))
    #expect(!card.description.contains("PRIVATE_ACTION_DEF"))
    #expect(!card.blocker.description.contains("PRIVATE_REASON_GHI"))
    #expect(!card.delivery.description.contains("PRIVATE_EVIDENCE_JKL"))
}

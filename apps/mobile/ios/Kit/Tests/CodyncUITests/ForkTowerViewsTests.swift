import CodyncKit
import Foundation
import Testing
@testable import CodyncUI

private func attentionItem(
    id: String,
    dedupeKey: String,
    kind: AttentionKind = .ownerQuestion,
    severity: AttentionSeverity = .actionRequired,
    createdAt: Int64 = 100
) -> AttentionItem {
    AttentionItem(
        id: id,
        dedupeKey: dedupeKey,
        kind: kind,
        severity: severity,
        spaceId: ForkTowerFixtureCatalog.spaceId,
        titleKey: "attention.test",
        summary: "Resumo de exemplo",
        actionKind: .answerQuestion,
        createdAt: createdAt,
        target: AttentionTarget()
    )
}

private func queue(
    items: [AttentionItem] = [],
    gaps: [CoverageGap] = []
) -> AttentionQueue {
    AttentionQueue(
        version: 1,
        hubId: ForkTowerFixtureCatalog.hubId,
        spaceId: ForkTowerFixtureCatalog.spaceId,
        collectedAt: 1_000,
        items: items,
        resolvedToday: [],
        coverageGaps: gaps
    )
}

private func doctorReport(overall: DoctorOverallStatus, checks: [DoctorCheck]) -> HubDoctorReport {
    HubDoctorReport(
        version: 1,
        hubId: ForkTowerFixtureCatalog.hubId,
        spaceId: ForkTowerFixtureCatalog.spaceId,
        collectedAt: 1_000,
        overall: overall,
        checks: checks
    )
}

@MainActor @Test func unavailableAttentionIsNeverRenderedAsEmptyQueue() {
    let presentation = ForkAttentionPresentation(observation: .unavailable)
    #expect(presentation.phase == .unavailable)
    #expect(presentation.phase != .nothingWaiting)
    #expect(presentation.items.isEmpty)
}

@MainActor @Test func emptyAvailableQueueWithoutGapsSaysNothingWaiting() {
    let presentation = ForkAttentionPresentation(observation: .available(value: queue()))
    #expect(presentation.phase == .nothingWaiting)
}

@MainActor @Test func coverageGapsBlockNothingWaitingEvenWhenItemsAreEmpty() {
    let gap = CoverageGap(source: DoctorArea.approvals.rawValue, code: .sourceUnavailable)
    let presentation = ForkAttentionPresentation(observation: .available(value: queue(gaps: [gap])))
    #expect(presentation.phase == .waiting)
    #expect(presentation.items.isEmpty)
    #expect(presentation.coverageLabels.count == 1)
    #expect(["Aprovações", "Approvals"].contains(presentation.coverageLabels[0]))
}

@MainActor @Test func unknownCoverageSourceUsesNeutralLabelNotExternalText() {
    let external = "fonte-externa-desconhecida"
    let label = ForkAttentionPresentation.coverageLabel(for: external)
    #expect(label != external)
    #expect(["uma fonte", "a source"].contains(label))
}

@MainActor @Test func catalogResolvesCuratedLabelsForEnglishFromModuleBundle() {
    #expect(ForkTowerText.value("Precisa de você", locale: Locale(identifier: "en")) == "Needs you")
    #expect(ForkTowerText.value("Aprovações", locale: Locale(identifier: "en")) == "Approvals")
}

@MainActor @Test func normalizedItemsDeduplicateByKeyAndSortBySeverityThenRecency() {
    let items = [
        attentionItem(id: "warn", dedupeKey: "a", severity: .warning, createdAt: 500),
        attentionItem(id: "block-old", dedupeKey: "b", severity: .blocking, createdAt: 100),
        attentionItem(id: "block-new", dedupeKey: "c", severity: .blocking, createdAt: 200),
        attentionItem(id: "action", dedupeKey: "d", severity: .actionRequired, createdAt: 300),
        attentionItem(id: "dup-warn", dedupeKey: "a", severity: .warning, createdAt: 50),
    ]
    let presentation = ForkAttentionPresentation(observation: .available(value: queue(items: items)))
    #expect(presentation.items.map(\.id) == ["block-new", "block-old", "action", "warn"])
}

@MainActor @Test func resolvedTodayStartsCollapsed() {
    let resolved = ResolvedAttentionItem(
        id: "resolved-1",
        kind: .permission,
        titleKey: "attention.resolved",
        summary: "Permissão concedida",
        resolvedAt: 900,
        resolutionKind: .approved,
        target: AttentionTarget()
    )
    let value = AttentionQueue(
        version: 1,
        hubId: ForkTowerFixtureCatalog.hubId,
        spaceId: ForkTowerFixtureCatalog.spaceId,
        collectedAt: 1_000,
        items: [],
        resolvedToday: [resolved],
        coverageGaps: []
    )
    let presentation = ForkAttentionPresentation(observation: .available(value: value))
    #expect(presentation.resolvedStartsCollapsed)
    #expect(presentation.resolvedToday.count == 1)
}

@MainActor @Test func doctorHubOfflineIsNotConfusedWithAIError() {
    let report = TowerObservation<HubDoctorReport>.unavailable
    let diagnosis = DoctorTransportDiagnosis.diagnose(report, hubReachable: false)
    let presentation = ForkDoctorPresentation(diagnosis: diagnosis, report: report)
    #expect(presentation.headline == .hubOffline)
    #expect(presentation.actionKind == .startHub)
}

@MainActor @Test func doctorAIWithoutAccountIsReportedAsAIDisconnected() {
    let checks = [
        DoctorCheck(id: "ai", area: .aiEngine, status: .error, code: "ai_no_active_account", count: nil, actionKind: .connectAI),
    ]
    let report = TowerObservation<HubDoctorReport>.available(value: doctorReport(overall: .error, checks: checks))
    let diagnosis = DoctorTransportDiagnosis.diagnose(report, hubReachable: true)
    let presentation = ForkDoctorPresentation(diagnosis: diagnosis, report: report)
    #expect(presentation.headline == .aiDisconnected(code: "ai_no_active_account"))
    #expect(presentation.actionKind == .connectAI)
    #expect(presentation.checks == checks)
}

@MainActor @Test func doctorWaitingUserIsExplicitlyNotStuck() {
    let checks = [
        DoctorCheck(id: "approvals", area: .approvals, status: .warning, code: "approvals_waiting", count: 2, actionKind: .reviewWaiting),
    ]
    let report = TowerObservation<HubDoctorReport>.available(value: doctorReport(overall: .waitingUser, checks: checks))
    let diagnosis = DoctorTransportDiagnosis.diagnose(report, hubReachable: true)
    let presentation = ForkDoctorPresentation(diagnosis: diagnosis, report: report)
    #expect(presentation.headline == .waitingUser(pendingCount: 2))
    #expect(presentation.actionKind == .reviewWaiting)
}

@MainActor @Test func doctorUnavailableReportDoesNotReportHealthy() {
    let presentation = ForkDoctorPresentation(
        diagnosis: DoctorTransportDiagnosis.diagnose(.unavailable, hubReachable: true),
        report: .unavailable
    )
    #expect(presentation.headline == .unavailable)
    #expect(presentation.checks.isEmpty)
}

@MainActor @Test func towerPresentationRendersOnlyMatchingProjection() {
    let snapshot = ForkTowerFixtureCatalog.tower(for: .director)
    let matching = ForkTowerPresentation(snapshot: snapshot, selected: .director)
    #expect(matching.counts == snapshot.counts)
    #expect(matching.cards.count == snapshot.cards.count)

    let mismatched = ForkTowerPresentation(snapshot: snapshot, selected: .overview)
    #expect(mismatched.counts == nil)
    #expect(mismatched.cards.isEmpty)

    let absent = ForkTowerPresentation(snapshot: nil, selected: .overview)
    #expect(absent.counts == nil)
}

@MainActor @Test func machineLabelShowsOnlyWhenExecutionIsActive() {
    let cards = ForkTowerFixtureCatalog.cards
    let active = ForkWorkCardPresentation(card: cards[0])
    #expect(["Precisa de você", "Needs you"].contains(active.observedLabel))
    #expect(active.machineLabel == "Máquina de exemplo A")

    let inactive = ForkWorkCardPresentation(card: cards[1])
    #expect(["Sem execução observada", "No observed execution"].contains(inactive.observedLabel))
    #expect(inactive.machineLabel == nil)
    #expect(["Em andamento", "In progress"].contains(inactive.declaredState))
}

@MainActor @Test func unknownDeclaredStateUsesNeutralLabel() {
    let external = "estado-externo-x"
    let label = ForkWorkCardPresentation.stateLabel(for: external)
    #expect(label != external)
    #expect(["Estado não reconhecido", "Unrecognized state"].contains(label))
}

@MainActor @Test func permissionPauseIsLinkedToAttentionItem() {
    let card = ForkTowerFixtureCatalog.cards[0]
    let presentation = ForkWorkCardPresentation(card: card)
    #expect(presentation.isPermissionPause)
    #expect(presentation.linkedAttentionItemId == "att-permission")

    let dependency = ForkWorkCardPresentation(card: ForkTowerFixtureCatalog.cards[2])
    #expect(!dependency.isPermissionPause)
    #expect(dependency.linkedAttentionItemId == nil)
}

@MainActor @Test func workCardActionsFailClosedWithoutCapabilities() {
    let restricted = ForkWorkCardPresentation(card: ForkTowerFixtureCatalog.cards[1])
    #expect(!restricted.canOwnerAccept)
    #expect(!restricted.canDirectorInterrupt)

    let owner = ForkWorkCardPresentation(card: ForkTowerFixtureCatalog.cards[2])
    #expect(owner.canOwnerAccept)
    #expect(!owner.canDirectorInterrupt)
}

@MainActor @Test func storeDiscardsInvalidationFromOldGeneration() {
    let store = ForkTowerFixtureStore()
    let oldGeneration = store.generation
    let event = TowerInvalidationEvent(
        type: .donnaAttentionChanged,
        hubId: ForkTowerFixtureCatalog.hubId,
        spaceId: ForkTowerFixtureCatalog.spaceId
    )
    #expect(store.receive(event, capturedGeneration: oldGeneration))

    store.bind(hubId: "hub-outro", spaceId: "espaco-outro")
    #expect(!store.receive(event, capturedGeneration: oldGeneration))
    #expect(!store.receive(
        TowerInvalidationEvent(type: .donnaDoctorChanged, hubId: "hub-outro", spaceId: "espaco-outro"),
        capturedGeneration: oldGeneration
    ))
}

@MainActor @Test func storeRejectsEventForAnotherHubInCurrentGeneration() {
    let store = ForkTowerFixtureStore()
    let generation = store.generation
    let foreign = TowerInvalidationEvent(type: .donnaGovernanceChanged, hubId: "hub-estranho", spaceId: ForkTowerFixtureCatalog.spaceId)
    #expect(!store.receive(foreign, capturedGeneration: generation))
}

@MainActor @Test func bindToOtherContextPublishesUnavailableImmediately() {
    let store = ForkTowerFixtureStore()
    store.bind(hubId: "hub-outro", spaceId: "espaco-outro")
    #expect(store.attention == .unavailable)
    #expect(store.doctor == .unavailable)
    #expect(store.tower == nil)
    #expect(store.doctorDiagnosis == .unavailable)
}

@MainActor @Test func perspectiveChangeInOtherContextDoesNotRestoreOldProjection() {
    let store = ForkTowerFixtureStore()
    store.bind(hubId: "hub-outro", spaceId: "espaco-outro")
    store.selectedPerspective = .director
    #expect(store.tower == nil)
    #expect(store.attention == .unavailable)
    #expect(store.doctor == .unavailable)
}

@MainActor @Test func returningToOriginalContextRestoresProjectionForCurrentPerspective() {
    let store = ForkTowerFixtureStore()
    store.bind(hubId: "hub-outro", spaceId: "espaco-outro")
    store.selectedPerspective = .bot
    store.bind(hubId: ForkTowerFixtureCatalog.hubId, spaceId: ForkTowerFixtureCatalog.spaceId)
    #expect(store.attention.isAvailable)
    #expect(store.doctor.isAvailable)
    #expect(store.tower?.perspective == .bot)
    #expect(store.tower?.cards.count == 1)
}

@MainActor @Test func lateEventFromPreviousContextIsDroppedAfterRoundTrip() {
    let store = ForkTowerFixtureStore()
    let oldGeneration = store.generation
    store.bind(hubId: "hub-outro", spaceId: "espaco-outro")
    store.bind(hubId: ForkTowerFixtureCatalog.hubId, spaceId: ForkTowerFixtureCatalog.spaceId)
    let late = TowerInvalidationEvent(
        type: .donnaAttentionChanged,
        hubId: ForkTowerFixtureCatalog.hubId,
        spaceId: ForkTowerFixtureCatalog.spaceId
    )
    #expect(!store.receive(late, capturedGeneration: oldGeneration))
    #expect(store.attention.isAvailable)
}

@MainActor @Test func storeSwitchesTowerProjectionPerPerspective() {
    let store = ForkTowerFixtureStore()
    #expect(store.tower?.perspective == .overview)
    #expect(store.tower?.cards.count == 3)

    store.selectedPerspective = .bot
    #expect(store.tower?.perspective == .bot)
    #expect(store.tower?.cards.count == 1)
    #expect(store.tower?.botId == ForkTowerFixtureCatalog.botId)
}

@MainActor @Test func storeDoctorStartsWaitingUserFromFixture() {
    let store = ForkTowerFixtureStore()
    #expect(store.doctorDiagnosis == .waitingUserApproval(pendingCount: 2))
    store.setHubReachable(false, capturedGeneration: store.generation)
    #expect(store.doctorDiagnosis == .hubOffline)
}

@MainActor @Test func hubOfflineDoesNotCarryOverToAnotherContextWithoutCTA() {
    let store = ForkTowerFixtureStore()
    store.setHubReachable(false, capturedGeneration: store.generation)
    #expect(store.doctorDiagnosis == .hubOffline)
    store.bind(hubId: "hub-outro", spaceId: "espaco-outro")
    #expect(store.doctorDiagnosis == .unavailable)
    #expect(store.doctorDiagnosis.actionKind == .none)
}

@MainActor @Test func sameContextRebindKeepsKnownReachability() {
    let store = ForkTowerFixtureStore()
    store.setHubReachable(false, capturedGeneration: store.generation)
    store.bind(hubId: ForkTowerFixtureCatalog.hubId, spaceId: ForkTowerFixtureCatalog.spaceId)
    #expect(store.doctorDiagnosis == .hubOffline)
}

@MainActor @Test func lateReachabilityUpdatesFromPreviousContextAreRejected() {
    let store = ForkTowerFixtureStore()
    let oldGeneration = store.generation
    store.bind(hubId: "hub-outro", spaceId: "espaco-outro")
    #expect(!store.setHubReachable(false, capturedGeneration: oldGeneration))
    #expect(store.doctorDiagnosis == .unavailable)
    store.bind(hubId: ForkTowerFixtureCatalog.hubId, spaceId: ForkTowerFixtureCatalog.spaceId)
    #expect(!store.setHubReachable(false, capturedGeneration: oldGeneration))
    #expect(store.doctorDiagnosis == .unavailable)
    #expect(store.setHubReachable(true, capturedGeneration: store.generation))
    #expect(store.doctorDiagnosis == .waitingUserApproval(pendingCount: 2))
}

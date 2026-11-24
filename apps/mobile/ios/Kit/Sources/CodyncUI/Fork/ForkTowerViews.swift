import CodyncKit
import SwiftUI

enum ForkTowerText {
    static func value(_ key: String.LocalizationValue, locale: Locale = .current) -> String {
        String(localized: key, bundle: .module, locale: locale)
    }
}

struct ForkAttentionPresentation: Equatable {
    enum Phase: Equatable {
        case unavailable
        case nothingWaiting
        case waiting
    }

    let phase: Phase
    let items: [AttentionItem]
    let coverageLabels: [String]
    let resolvedToday: [ResolvedAttentionItem]
    let resolvedStartsCollapsed: Bool

    init(observation: TowerObservation<AttentionQueue>) {
        guard case .available(let queue, _, _) = observation else {
            phase = .unavailable
            items = []
            coverageLabels = []
            resolvedToday = []
            resolvedStartsCollapsed = true
            return
        }
        let visibleItems = queue.normalizedItems
        let gaps = queue.coverageGaps.map { Self.coverageLabel(for: $0.source) }
        items = visibleItems
        coverageLabels = gaps
        resolvedToday = queue.resolvedToday
        resolvedStartsCollapsed = queue.isResolvedTodayCollapsedByDefault
        phase = visibleItems.isEmpty && gaps.isEmpty ? .nothingWaiting : .waiting
    }

    static func coverageLabel(for source: String) -> String {
        switch source {
        case "permissions": return ForkTowerText.value("Permissões")
        case "delegated_permissions": return ForkTowerText.value("Permissões delegadas")
        case "proposals": return ForkTowerText.value("Propostas")
        case "task_reviews": return ForkTowerText.value("Revisões de tarefa")
        case "routine_reviews": return ForkTowerText.value("Revisões de rotina")
        case "owner_questions": return ForkTowerText.value("Perguntas")
        default: return ForkTowerText.value("uma fonte")
        }
    }
}

struct ForkAttentionQueueView: View {
    let observation: TowerObservation<AttentionQueue>
    let onAction: (AttentionItem) -> Void
    @State private var resolvedExpanded: Bool

    init(observation: TowerObservation<AttentionQueue>, onAction: @escaping (AttentionItem) -> Void) {
        self.observation = observation
        self.onAction = onAction
        _resolvedExpanded = State(initialValue: !ForkAttentionPresentation(observation: observation).resolvedStartsCollapsed)
    }

    var body: some View {
        let presentation = ForkAttentionPresentation(observation: observation)
        List {
            if presentation.phase == .unavailable {
                Section {
                    Label(String(localized: "Não foi possível carregar agora", bundle: .module), systemImage: "exclamationmark.triangle")
                }
            }
            ForEach(Array(presentation.coverageLabels.enumerated()), id: \.offset) { _, label in
                Section {
                    Label(String(localized: "Não consegui ver \(label)", bundle: .module), systemImage: "eye.slash")
                        .foregroundStyle(.orange)
                }
            }
            if presentation.phase == .nothingWaiting {
                Section {
                    Label(String(localized: "Nada esperando você", bundle: .module), systemImage: "checkmark.circle")
                }
            }
            if !presentation.items.isEmpty {
                Section(String(localized: "Esperando você", bundle: .module)) {
                    ForEach(presentation.items) { item in
                        ForkAttentionCard(item: item, onAction: onAction)
                    }
                }
            }
            if !presentation.resolvedToday.isEmpty {
                Section {
                    DisclosureGroup(String(localized: "Resolvidos hoje (\(presentation.resolvedToday.count))", bundle: .module), isExpanded: $resolvedExpanded) {
                        ForEach(presentation.resolvedToday) { item in
                            Text(item.summary ?? item.kind.rotulo)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
    }
}

private struct ForkAttentionCard: View {
    let item: AttentionItem
    let onAction: (AttentionItem) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(item.severity.rotulo)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(item.severity == .blocking ? Color.red : Color.orange)
                Spacer()
                Text(item.kind.rotulo)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Text(item.summary ?? item.kind.rotulo)
            Button(item.actionKind.ctaRotulo) {
                onAction(item)
            }
            .buttonStyle(.bordered)
        }
        .padding(.vertical, 4)
    }
}

struct ForkDoctorPresentation: Equatable {
    enum Headline: Equatable {
        case hubOffline
        case aiDisconnected(code: String)
        case waitingUser(pendingCount: Int?)
        case degraded
        case healthy
        case unavailable
    }

    let headline: Headline
    let actionKind: DoctorActionKind
    let checks: [DoctorCheck]

    init(diagnosis: DoctorTransportDiagnosis, report: TowerObservation<HubDoctorReport>) {
        switch diagnosis {
        case .hubOffline: headline = .hubOffline
        case .aiDisconnected(let code): headline = .aiDisconnected(code: code)
        case .waitingUserApproval(let count): headline = .waitingUser(pendingCount: count)
        case .degraded: headline = .degraded
        case .healthy: headline = .healthy
        case .unavailable: headline = .unavailable
        }
        actionKind = diagnosis.actionKind
        checks = report.value?.checks ?? []
    }
}

struct ForkHubDoctorView: View {
    let diagnosis: DoctorTransportDiagnosis
    let report: TowerObservation<HubDoctorReport>
    let onAction: (DoctorActionKind) -> Void

    var body: some View {
        let presentation = ForkDoctorPresentation(diagnosis: diagnosis, report: report)
        List {
            Section {
                VStack(alignment: .leading, spacing: 8) {
                    Label(presentation.headline.titulo, systemImage: presentation.headline.icone)
                        .font(.headline)
                    Text(presentation.headline.mensagem)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                    if presentation.actionKind != .none {
                        Button(presentation.actionKind.ctaRotulo) {
                            onAction(presentation.actionKind)
                        }
                        .buttonStyle(.borderedProminent)
                    }
                }
                .padding(.vertical, 4)
            }
            if !presentation.checks.isEmpty {
                Section(String(localized: "Verificações", bundle: .module)) {
                    ForEach(presentation.checks) { check in
                        HStack {
                            Text(check.area.rotulo)
                            Spacer()
                            if let count = check.count {
                                Text("\(count)")
                                    .foregroundStyle(.secondary)
                            }
                            Text(check.status.rotulo)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                            if check.actionKind != .none {
                                Text(check.actionKind.ctaRotulo)
                                    .font(.caption)
                            }
                        }
                    }
                }
            }
        }
    }
}

struct ForkWorkCardPresentation: Equatable {
    let title: String
    let projectName: String
    let bucketLabel: String
    let originLabel: String
    let blockerLabel: String?
    let isPermissionPause: Bool
    let linkedAttentionItemId: String?
    let deliveryLabel: String
    let declaredState: String
    let observedLabel: String
    let machineLabel: String?
    let canOwnerAccept: Bool
    let canDirectorInterrupt: Bool

    init(card: WorkCard) {
        title = card.title
        projectName = card.projectName
        bucketLabel = card.bucket.rotulo
        originLabel = card.origin.kind.rotulo
        blockerLabel = card.blocker.kind == .none ? nil : card.blocker.kind.rotulo
        isPermissionPause = card.blocker.kind == .permissionPause
        linkedAttentionItemId = card.blocker.kind == .permissionPause ? card.blocker.attentionItemId : nil
        deliveryLabel = [
            card.delivery.hasEvidence ? String(localized: "com entrega", bundle: .module) : String(localized: "sem entrega", bundle: .module),
            card.delivery.acceptedByOwner ? String(localized: "aceita por você", bundle: .module) : String(localized: "ainda não aceita", bundle: .module),
        ].joined(separator: ", ")
        declaredState = Self.stateLabel(for: card.state)
        observedLabel = card.observedExecution.active ? card.observedExecution.status.rotulo : String(localized: "Sem execução observada", bundle: .module)
        machineLabel = card.observedExecution.active ? card.observedExecution.machineLabel : nil
        canOwnerAccept = card.capabilities.canOwnerAccept
        canDirectorInterrupt = card.capabilities.canDirectorInterrupt
    }

    static func stateLabel(for state: String) -> String {
        switch state {
        case "backlog": return ForkTowerText.value("A fazer")
        case "in_progress": return ForkTowerText.value("Em andamento")
        case "review": return ForkTowerText.value("Em conferência")
        case "done": return ForkTowerText.value("Concluída")
        case "blocked": return ForkTowerText.value("Bloqueada")
        case "cancelled": return ForkTowerText.value("Cancelada")
        default: return ForkTowerText.value("Estado não reconhecido")
        }
    }
}

enum ForkWorkCardAction: Equatable {
    case accept
    case interrupt
}

struct ForkTowerPresentation: Equatable {
    let counts: TowerViewCounts?
    let cards: [WorkCard]

    init(snapshot: TowerViewSnapshot?, selected: TowerPerspective) {
        guard let snapshot, snapshot.perspective == selected else {
            counts = nil
            cards = []
            return
        }
        counts = snapshot.counts
        cards = snapshot.cards
    }
}

struct ForkTowerScreen: View {
    @ObservedObject var store: ForkTowerFixtureStore
    let onCardAction: (WorkCard, ForkWorkCardAction) -> Void

    private let perspectives: [TowerPerspective] = [.overview, .director, .bot]

    var body: some View {
        let presentation = ForkTowerPresentation(snapshot: store.tower, selected: store.selectedPerspective)
        List {
            Section {
                Picker(String(localized: "Visão", bundle: .module), selection: $store.selectedPerspective) {
                    ForEach(perspectives, id: \.self) { perspective in
                        Text(perspective.rotulo).tag(perspective)
                    }
                }
                .pickerStyle(.segmented)
            }
            if let counts = presentation.counts {
                Section {
                    HStack {
                        ForkTowerCount(value: counts.needsYou, label: String(localized: "Precisa de você", bundle: .module))
                        ForkTowerCount(value: counts.observedWorking, label: String(localized: "Trabalhando", bundle: .module))
                        ForkTowerCount(value: counts.waitingOrBlocked, label: String(localized: "Esperando", bundle: .module))
                        ForkTowerCount(value: counts.totalVisible, label: String(localized: "Total", bundle: .module))
                    }
                }
                Section {
                    ForEach(presentation.cards) { card in
                        ForkWorkCardView(card: card, onAction: { action in
                            onCardAction(card, action)
                        })
                    }
                }
            } else {
                Section {
                    Label(String(localized: "Não foi possível carregar agora", bundle: .module), systemImage: "exclamationmark.triangle")
                }
            }
        }
    }
}

private struct ForkTowerCount: View {
    let value: Int
    let label: String

    var body: some View {
        VStack {
            Text("\(value)")
                .font(.title3.weight(.semibold))
            Text(label)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
    }
}

private struct ForkWorkCardView: View {
    let card: WorkCard
    let onAction: (ForkWorkCardAction) -> Void

    var body: some View {
        let presentation = ForkWorkCardPresentation(card: card)
        VStack(alignment: .leading, spacing: 6) {
            Text(presentation.title)
                .font(.headline)
            Text(presentation.projectName)
                .font(.caption)
                .foregroundStyle(.secondary)
            Text("\(presentation.bucketLabel) · \(presentation.originLabel)")
                .font(.caption)
            if let blocker = presentation.blockerLabel {
                Label(blocker, systemImage: presentation.isPermissionPause ? "hand.raised.fill" : "pause.circle")
                    .font(.caption)
                    .foregroundStyle(presentation.isPermissionPause ? Color.orange : Color.gray)
            }
            if presentation.linkedAttentionItemId != nil {
                Text(String(localized: "Ligado a uma pendência em Esperando você", bundle: .module))
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
            Text(presentation.deliveryLabel)
                .font(.caption)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 2) {
                Text(String(localized: "Declarado: \(presentation.declaredState)", bundle: .module))
                Text(String(localized: "Execução observada: \(presentation.observedLabel)", bundle: .module))
                if let machine = presentation.machineLabel {
                    Text(String(localized: "Máquina: \(machine)", bundle: .module))
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            if presentation.canOwnerAccept || presentation.canDirectorInterrupt {
                HStack {
                    if presentation.canOwnerAccept {
                        Button(String(localized: "Aceitar entrega", bundle: .module)) { onAction(.accept) }
                            .buttonStyle(.bordered)
                    }
                    if presentation.canDirectorInterrupt {
                        Button(String(localized: "Interromper", bundle: .module)) { onAction(.interrupt) }
                            .buttonStyle(.bordered)
                    }
                }
            }
        }
        .padding(.vertical, 4)
    }
}

enum ForkTowerFixtureCatalog {
    static let hubId = "hub-exemplo"
    static let spaceId = "espaco-exemplo"
    static let directorBotId = "tanga-exemplo"
    static let botId = "bot-exemplo"

    static let attention: TowerObservation<AttentionQueue> = .available(value: AttentionQueue(
        version: 1,
        hubId: hubId,
        spaceId: spaceId,
        collectedAt: 1_000,
        items: [
            AttentionItem(
                id: "att-permission",
                dedupeKey: "permission-1",
                kind: .permission,
                severity: .blocking,
                spaceId: spaceId,
                titleKey: "attention.permission",
                summary: "Permissão para publicar no canal de exemplo",
                actionKind: .respondPermission,
                createdAt: 300,
                target: AttentionTarget(taskId: "task-1")
            ),
            AttentionItem(
                id: "att-question",
                dedupeKey: "question-1",
                kind: .ownerQuestion,
                severity: .actionRequired,
                spaceId: spaceId,
                titleKey: "attention.question",
                summary: "Qual data de entrega você prefere?",
                actionKind: .answerQuestion,
                createdAt: 200,
                target: AttentionTarget(taskId: "task-2")
            ),
            AttentionItem(
                id: "att-doctor",
                dedupeKey: "doctor-1",
                kind: .doctorAlert,
                severity: .warning,
                spaceId: spaceId,
                titleKey: "attention.doctor",
                summary: "Uma área do Hub precisa de atenção",
                actionKind: .openDoctor,
                createdAt: 100,
                target: AttentionTarget()
            ),
        ],
        resolvedToday: [
            ResolvedAttentionItem(
                id: "resolved-1",
                kind: .permission,
                titleKey: "attention.resolved",
                summary: "Permissão concedida",
                resolvedAt: 900,
                resolutionKind: .approved,
                target: AttentionTarget(taskId: "task-3")
            ),
        ],
        coverageGaps: []
    ))

    static let doctor: TowerObservation<HubDoctorReport> = .available(value: HubDoctorReport(
        version: 1,
        hubId: hubId,
        spaceId: spaceId,
        collectedAt: 1_000,
        overall: .waitingUser,
        checks: [
            DoctorCheck(id: "hub", area: .hub, status: .ok, code: "hub_running", count: nil, actionKind: .none),
            DoctorCheck(id: "ai", area: .aiEngine, status: .ok, code: "ai_ready", count: nil, actionKind: .none),
            DoctorCheck(id: "approvals", area: .approvals, status: .warning, code: "approvals_waiting", count: 2, actionKind: .reviewWaiting),
        ]
    ))

    static let cards: [WorkCard] = [
        WorkCard(
            taskId: "task-1",
            projectId: "projeto-a",
            projectName: "Projeto Exemplo A",
            spaceId: spaceId,
            title: "Revisar proposta de campanha",
            state: "in_progress",
            bucket: .needsYou,
            origin: WorkOrigin(kind: .ownerDirect),
            requesterLabel: .owner,
            directorBotId: directorBotId,
            assigneeBotId: botId,
            blocker: WorkBlocker(kind: .permissionPause, reason: "Aguardando permissão", attentionItemId: "att-permission"),
            delivery: WorkDelivery(hasEvidence: true, acceptedByOwner: false),
            declaredUpdatedAt: 1_000,
            observedExecution: ObservedExecution(active: true, status: .needsInput, observedAt: 999, machineLabel: "Máquina de exemplo A"),
            capabilities: WorkCapabilities(canDirectorInterrupt: true, canOwnerAccept: false)
        ),
        WorkCard(
            taskId: "task-2",
            projectId: "projeto-b",
            projectName: "Projeto Exemplo B",
            spaceId: spaceId,
            title: "Organizar lista de contatos",
            state: "in_progress",
            bucket: .observedWorking,
            origin: WorkOrigin(kind: .delegated, delegatedByBotId: directorBotId),
            requesterLabel: .bot,
            directorBotId: directorBotId,
            assigneeBotId: "bot-exemplo-2",
            blocker: WorkBlocker(kind: .none),
            delivery: WorkDelivery(hasEvidence: false, acceptedByOwner: false),
            declaredUpdatedAt: 900,
            observedExecution: ObservedExecution(active: false, status: .idle, machineLabel: "Máquina oculta"),
            capabilities: WorkCapabilities(canDirectorInterrupt: false, canOwnerAccept: false)
        ),
        WorkCard(
            taskId: "task-3",
            projectId: "projeto-c",
            projectName: "Projeto Exemplo C",
            spaceId: spaceId,
            title: "Resumo semanal",
            state: "blocked",
            bucket: .waitingOrBlocked,
            origin: WorkOrigin(kind: .routine, routineId: "rotina-exemplo"),
            requesterLabel: .routine,
            blocker: WorkBlocker(kind: .dependency, reason: "Depende de outra tarefa"),
            delivery: WorkDelivery(hasEvidence: true, acceptedByOwner: true),
            declaredUpdatedAt: 800,
            observedExecution: ObservedExecution(active: true, status: .working, machineLabel: "Máquina de exemplo B"),
            capabilities: WorkCapabilities(canDirectorInterrupt: false, canOwnerAccept: true)
        ),
    ]

    static func tower(for perspective: TowerPerspective) -> TowerViewSnapshot {
        let visible: [WorkCard]
        switch perspective {
        case .overview: visible = cards
        case .director: visible = cards.filter { $0.directorBotId == directorBotId }
        case .bot: visible = cards.filter { $0.assigneeBotId == botId }
        }
        return TowerViewSnapshot(
            version: 1,
            hubId: hubId,
            spaceId: spaceId,
            perspective: perspective,
            directorBotId: perspective == .director ? directorBotId : nil,
            botId: perspective == .bot ? botId : nil,
            collectedAt: 1_000,
            cards: visible,
            counts: TowerViewCounts(
                needsYou: visible.filter { $0.bucket == .needsYou }.count,
                observedWorking: visible.filter { $0.bucket == .observedWorking }.count,
                waitingOrBlocked: visible.filter { $0.bucket == .waitingOrBlocked }.count,
                totalVisible: visible.count
            )
        )
    }
}

@MainActor
final class ForkTowerFixtureStore: ObservableObject {
    @Published private(set) var attention: TowerObservation<AttentionQueue> = .unavailable
    @Published private(set) var doctor: TowerObservation<HubDoctorReport> = .unavailable
    @Published private(set) var tower: TowerViewSnapshot?
    @Published private(set) var hubReachable: Bool? = nil
    @Published var selectedPerspective: TowerPerspective = .overview {
        didSet { publish() }
    }

    private var invalidation: TowerInvalidationGuard

    init() {
        var guardState = TowerInvalidationGuard()
        guardState.bindContext(hubId: ForkTowerFixtureCatalog.hubId, spaceId: ForkTowerFixtureCatalog.spaceId)
        invalidation = guardState
        hubReachable = true
        publish()
    }

    var generation: UInt64 {
        invalidation.generation
    }

    var doctorDiagnosis: DoctorTransportDiagnosis {
        guard let hubReachable else {
            return .unavailable
        }
        return DoctorTransportDiagnosis.diagnose(doctor, hubReachable: hubReachable)
    }

    func bind(hubId: String, spaceId: String) {
        let changesContext = invalidation.activeHubId != hubId || invalidation.activeSpaceId != spaceId
        invalidation.bindContext(hubId: hubId, spaceId: spaceId)
        if changesContext {
            hubReachable = nil
        }
        publish()
    }

    @discardableResult
    func setHubReachable(_ reachable: Bool, capturedGeneration: UInt64) -> Bool {
        guard invalidation.activeHubId == ForkTowerFixtureCatalog.hubId,
              invalidation.activeSpaceId == ForkTowerFixtureCatalog.spaceId,
              capturedGeneration == invalidation.generation else {
            return false
        }
        hubReachable = reachable
        return true
    }

    @discardableResult
    func receive(_ event: TowerInvalidationEvent, capturedGeneration: UInt64) -> Bool {
        guard invalidation.shouldAccept(event, capturedGeneration: capturedGeneration) else {
            return false
        }
        publish()
        return true
    }

    private func publish() {
        guard invalidation.activeHubId == ForkTowerFixtureCatalog.hubId,
              invalidation.activeSpaceId == ForkTowerFixtureCatalog.spaceId else {
            attention = .unavailable
            doctor = .unavailable
            tower = nil
            return
        }
        attention = ForkTowerFixtureCatalog.attention
        doctor = ForkTowerFixtureCatalog.doctor
        tower = ForkTowerFixtureCatalog.tower(for: selectedPerspective)
    }
}

private extension AttentionSeverity {
    var rotulo: String {
        switch self {
        case .blocking: return String(localized: "Bloqueando", bundle: .module)
        case .actionRequired: return String(localized: "Precisa de ação", bundle: .module)
        case .warning: return String(localized: "Atenção", bundle: .module)
        }
    }
}

private extension AttentionKind {
    var rotulo: String {
        switch self {
        case .permission, .delegatedPermission: return String(localized: "Permissão", bundle: .module)
        case .ownerQuestion: return String(localized: "Pergunta", bundle: .module)
        case .taskProposal: return String(localized: "Proposta de tarefa", bundle: .module)
        case .taskReview: return String(localized: "Revisão de tarefa", bundle: .module)
        case .routineReview: return String(localized: "Revisão de rotina", bundle: .module)
        case .doctorAlert: return String(localized: "Alerta do Doctor", bundle: .module)
        }
    }
}

private extension AttentionActionKind {
    var ctaRotulo: String {
        switch self {
        case .respondPermission: return String(localized: "Responder permissão", bundle: .module)
        case .answerQuestion: return String(localized: "Responder", bundle: .module)
        case .decideProposal: return String(localized: "Decidir proposta", bundle: .module)
        case .reviewTask: return String(localized: "Revisar tarefa", bundle: .module)
        case .reviewRoutine: return String(localized: "Revisar rotina", bundle: .module)
        case .openConversation: return String(localized: "Abrir conversa", bundle: .module)
        case .openDoctor: return String(localized: "Abrir Doctor", bundle: .module)
        }
    }
}

private extension DoctorActionKind {
    var ctaRotulo: String {
        switch self {
        case .none: return String(localized: "Nenhuma ação necessária", bundle: .module)
        case .startHub: return String(localized: "Iniciar Hub", bundle: .module)
        case .connectAI: return String(localized: "Conectar IA", bundle: .module)
        case .reviewWaiting: return String(localized: "Revisar pendências", bundle: .module)
        case .checkChannels: return String(localized: "Verificar canais", bundle: .module)
        case .checkMCPs: return String(localized: "Verificar integrações", bundle: .module)
        case .checkRoutines: return String(localized: "Verificar rotinas", bundle: .module)
        case .openTorre: return String(localized: "Abrir Torre", bundle: .module)
        }
    }
}

private extension DoctorArea {
    var rotulo: String {
        switch self {
        case .hub: return String(localized: "Hub", bundle: .module)
        case .aiEngine: return String(localized: "IA", bundle: .module)
        case .approvals: return String(localized: "Aprovações", bundle: .module)
        case .channels: return String(localized: "Canais", bundle: .module)
        case .mcps: return String(localized: "Integrações", bundle: .module)
        case .routines: return String(localized: "Rotinas", bundle: .module)
        case .delegation: return String(localized: "Delegação", bundle: .module)
        }
    }
}

private extension DoctorCheckStatus {
    var rotulo: String {
        switch self {
        case .ok: return String(localized: "OK", bundle: .module)
        case .warning: return String(localized: "Atenção", bundle: .module)
        case .error: return String(localized: "Erro", bundle: .module)
        case .unavailable: return String(localized: "Indisponível", bundle: .module)
        }
    }
}

private extension ForkDoctorPresentation.Headline {
    var titulo: String {
        switch self {
        case .hubOffline: return String(localized: "Hub offline", bundle: .module)
        case .aiDisconnected: return String(localized: "IA desconectada", bundle: .module)
        case .waitingUser: return String(localized: "Aguardando sua aprovação", bundle: .module)
        case .degraded: return String(localized: "Algumas áreas precisam de atenção", bundle: .module)
        case .healthy: return String(localized: "Tudo funcionando", bundle: .module)
        case .unavailable: return String(localized: "Não foi possível carregar agora", bundle: .module)
        }
    }

    var mensagem: String {
        switch self {
        case .hubOffline:
            return String(localized: "O Hub está inacessível. Isso não é erro de IA: inicie o Hub para ver o estado.", bundle: .module)
        case .aiDisconnected(let code):
            switch code {
            case "ai_no_active_account": return String(localized: "Nenhuma conta de IA está ativa.", bundle: .module)
            case "ai_all_paused": return String(localized: "Todas as contas de IA estão pausadas.", bundle: .module)
            case "ai_rate_limited": return String(localized: "As contas de IA atingiram o limite de uso.", bundle: .module)
            default: return String(localized: "A IA não está disponível agora.", bundle: .module)
            }
        case .waitingUser(let count):
            guard let count else {
                return String(localized: "O sistema não está travado: falta só a sua decisão.", bundle: .module)
            }
            return String(localized: "O sistema não está travado: falta só a sua decisão (\(count) pendências).", bundle: .module)
        case .degraded:
            return String(localized: "Abra a Torre para ver quais áreas precisam de atenção.", bundle: .module)
        case .healthy:
            return String(localized: "Nenhuma ação necessária agora.", bundle: .module)
        case .unavailable:
            return String(localized: "Não foi possível carregar o diagnóstico do Hub.", bundle: .module)
        }
    }

    var icone: String {
        switch self {
        case .hubOffline: return "wifi.slash"
        case .aiDisconnected: return "bolt.slash"
        case .waitingUser: return "hand.raised"
        case .degraded: return "exclamationmark.triangle"
        case .healthy: return "checkmark.seal"
        case .unavailable: return "questionmark.circle"
        }
    }
}

private extension WorkBucket {
    var rotulo: String {
        switch self {
        case .needsYou: return String(localized: "Precisa de você", bundle: .module)
        case .observedWorking: return String(localized: "Trabalhando", bundle: .module)
        case .waitingOrBlocked: return String(localized: "Esperando ou bloqueado", bundle: .module)
        case .backlog: return String(localized: "Backlog", bundle: .module)
        case .done: return String(localized: "Concluído", bundle: .module)
        }
    }
}

private extension WorkOriginKind {
    var rotulo: String {
        switch self {
        case .ownerDirect: return String(localized: "Pedido direto seu", bundle: .module)
        case .delegated: return String(localized: "Delegado", bundle: .module)
        case .routine: return String(localized: "Rotina", bundle: .module)
        case .unknown: return String(localized: "Origem não informada", bundle: .module)
        }
    }
}

private extension WorkBlockerKind {
    var rotulo: String {
        switch self {
        case .permissionPause: return String(localized: "Pausado por permissão", bundle: .module)
        case .ownerDecision: return String(localized: "Aguardando decisão sua", bundle: .module)
        case .dependency: return String(localized: "Depende de outra tarefa", bundle: .module)
        case .manual: return String(localized: "Bloqueio manual", bundle: .module)
        case .none: return String(localized: "Sem bloqueio", bundle: .module)
        }
    }
}

private extension ObservedExecutionStatus {
    var rotulo: String {
        switch self {
        case .idle: return String(localized: "Parada", bundle: .module)
        case .working: return String(localized: "Trabalhando", bundle: .module)
        case .needsInput: return String(localized: "Precisa de você", bundle: .module)
        case .waiting: return String(localized: "Esperando", bundle: .module)
        case .error: return String(localized: "Com erro", bundle: .module)
        case .unavailable: return String(localized: "Indisponível", bundle: .module)
        }
    }
}

private extension TowerPerspective {
    var rotulo: String {
        switch self {
        case .overview: return String(localized: "Dono & Berga", bundle: .module)
        case .director: return String(localized: "Tanga", bundle: .module)
        case .bot: return String(localized: "Executor", bundle: .module)
        }
    }
}

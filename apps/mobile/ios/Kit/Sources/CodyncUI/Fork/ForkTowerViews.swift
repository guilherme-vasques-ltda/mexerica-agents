import CodyncKit
import SwiftUI

struct ForkAttentionPresentation: Equatable {
    enum Phase: Equatable {
        case unavailable
        case nothingWaiting
        case waiting
    }

    let phase: Phase
    let items: [AttentionItem]
    let coverageSources: [String]
    let resolvedToday: [ResolvedAttentionItem]
    let resolvedStartsCollapsed: Bool

    init(observation: Observation<AttentionQueue>) {
        guard case .available(let queue, _, _) = observation else {
            phase = .unavailable
            items = []
            coverageSources = []
            resolvedToday = []
            resolvedStartsCollapsed = true
            return
        }
        let visibleItems = queue.normalizedItems
        let gaps = queue.coverageGaps.map(\.source)
        items = visibleItems
        coverageSources = gaps
        resolvedToday = queue.resolvedToday
        resolvedStartsCollapsed = queue.isResolvedTodayCollapsedByDefault
        phase = visibleItems.isEmpty && gaps.isEmpty ? .nothingWaiting : .waiting
    }
}

struct ForkAttentionQueueView: View {
    let observation: Observation<AttentionQueue>
    let onAction: (AttentionItem) -> Void
    @State private var resolvedExpanded: Bool

    init(observation: Observation<AttentionQueue>, onAction: @escaping (AttentionItem) -> Void) {
        self.observation = observation
        self.onAction = onAction
        _resolvedExpanded = State(initialValue: !ForkAttentionPresentation(observation: observation).resolvedStartsCollapsed)
    }

    var body: some View {
        let presentation = ForkAttentionPresentation(observation: observation)
        List {
            if presentation.phase == .unavailable {
                Section {
                    Label("Não foi possível carregar agora", systemImage: "exclamationmark.triangle")
                }
            }
            ForEach(presentation.coverageSources, id: \.self) { source in
                Section {
                    Label("Não consegui ver \(source)", systemImage: "eye.slash")
                        .foregroundStyle(.orange)
                }
            }
            if presentation.phase == .nothingWaiting {
                Section {
                    Label("Nada esperando você", systemImage: "checkmark.circle")
                }
            }
            if !presentation.items.isEmpty {
                Section("Esperando você") {
                    ForEach(presentation.items) { item in
                        ForkAttentionCard(item: item, onAction: onAction)
                    }
                }
            }
            if !presentation.resolvedToday.isEmpty {
                Section {
                    DisclosureGroup("Resolvidos hoje (\(presentation.resolvedToday.count))", isExpanded: $resolvedExpanded) {
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

    init(diagnosis: DoctorTransportDiagnosis, report: Observation<HubDoctorReport>) {
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
    let report: Observation<HubDoctorReport>
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
                Section("Verificações") {
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
            card.delivery.hasEvidence ? "com entrega" : "sem entrega",
            card.delivery.acceptedByOwner ? "aceita por você" : "ainda não aceita",
        ].joined(separator: ", ")
        declaredState = card.state
        observedLabel = card.observedExecution.active ? card.observedExecution.status.rotulo : "Sem execução observada"
        machineLabel = card.observedExecution.active ? card.observedExecution.machineLabel : nil
        canOwnerAccept = card.capabilities.canOwnerAccept
        canDirectorInterrupt = card.capabilities.canDirectorInterrupt
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
                Picker("Visão", selection: $store.selectedPerspective) {
                    ForEach(perspectives, id: \.self) { perspective in
                        Text(perspective.rotulo).tag(perspective)
                    }
                }
                .pickerStyle(.segmented)
            }
            if let counts = presentation.counts {
                Section {
                    HStack {
                        ForkTowerCount(value: counts.needsYou, label: "Precisa de você")
                        ForkTowerCount(value: counts.observedWorking, label: "Trabalhando")
                        ForkTowerCount(value: counts.waitingOrBlocked, label: "Esperando")
                        ForkTowerCount(value: counts.totalVisible, label: "Total")
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
                    Label("Não foi possível carregar agora", systemImage: "exclamationmark.triangle")
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
                Text("Ligado a uma pendência em Esperando você")
                    .font(.caption)
                    .foregroundStyle(.orange)
            }
            Text(presentation.deliveryLabel)
                .font(.caption)
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 2) {
                Text("Declarado: \(presentation.declaredState)")
                Text("Execução observada: \(presentation.observedLabel)")
                if let machine = presentation.machineLabel {
                    Text("Máquina: \(machine)")
                }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            if presentation.canOwnerAccept || presentation.canDirectorInterrupt {
                HStack {
                    if presentation.canOwnerAccept {
                        Button("Aceitar entrega") { onAction(.accept) }
                            .buttonStyle(.bordered)
                    }
                    if presentation.canDirectorInterrupt {
                        Button("Interromper") { onAction(.interrupt) }
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

    static let attention: Observation<AttentionQueue> = .available(value: AttentionQueue(
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

    static let doctor: Observation<HubDoctorReport> = .available(value: HubDoctorReport(
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
            state: "waiting",
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
    @Published private(set) var attention: Observation<AttentionQueue>
    @Published private(set) var doctor: Observation<HubDoctorReport>
    @Published private(set) var tower: TowerViewSnapshot?
    @Published private(set) var hubReachable = true
    @Published var selectedPerspective: TowerPerspective = .overview {
        didSet {
            tower = ForkTowerFixtureCatalog.tower(for: selectedPerspective)
        }
    }

    private var invalidation: TowerInvalidationGuard

    init() {
        var guardState = TowerInvalidationGuard()
        guardState.bindContext(hubId: ForkTowerFixtureCatalog.hubId, spaceId: ForkTowerFixtureCatalog.spaceId)
        invalidation = guardState
        attention = ForkTowerFixtureCatalog.attention
        doctor = ForkTowerFixtureCatalog.doctor
        tower = ForkTowerFixtureCatalog.tower(for: .overview)
    }

    var generation: UInt64 {
        invalidation.generation
    }

    var doctorDiagnosis: DoctorTransportDiagnosis {
        DoctorTransportDiagnosis.diagnose(doctor, hubReachable: hubReachable)
    }

    func bind(hubId: String, spaceId: String) {
        invalidation.bindContext(hubId: hubId, spaceId: spaceId)
    }

    func setHubReachable(_ reachable: Bool) {
        hubReachable = reachable
    }

    @discardableResult
    func receive(_ event: TowerInvalidationEvent, capturedGeneration: UInt64) -> Bool {
        guard invalidation.shouldAccept(event, capturedGeneration: capturedGeneration) else {
            return false
        }
        attention = ForkTowerFixtureCatalog.attention
        doctor = ForkTowerFixtureCatalog.doctor
        tower = ForkTowerFixtureCatalog.tower(for: selectedPerspective)
        return true
    }
}

private extension AttentionSeverity {
    var rotulo: String {
        switch self {
        case .blocking: return "Bloqueando"
        case .actionRequired: return "Precisa de ação"
        case .warning: return "Atenção"
        }
    }
}

private extension AttentionKind {
    var rotulo: String {
        switch self {
        case .permission, .delegatedPermission: return "Permissão"
        case .ownerQuestion: return "Pergunta"
        case .taskProposal: return "Proposta de tarefa"
        case .taskReview: return "Revisão de tarefa"
        case .routineReview: return "Revisão de rotina"
        case .doctorAlert: return "Alerta do Doctor"
        }
    }
}

private extension AttentionActionKind {
    var ctaRotulo: String {
        switch self {
        case .respondPermission: return "Responder permissão"
        case .answerQuestion: return "Responder"
        case .decideProposal: return "Decidir proposta"
        case .reviewTask: return "Revisar tarefa"
        case .reviewRoutine: return "Revisar rotina"
        case .openConversation: return "Abrir conversa"
        case .openDoctor: return "Abrir Doctor"
        }
    }
}

private extension DoctorActionKind {
    var ctaRotulo: String {
        switch self {
        case .none: return "Nenhuma ação necessária"
        case .startHub: return "Iniciar Hub"
        case .connectAI: return "Conectar IA"
        case .reviewWaiting: return "Revisar pendências"
        case .checkChannels: return "Verificar canais"
        case .checkMCPs: return "Verificar integrações"
        case .checkRoutines: return "Verificar rotinas"
        case .openTorre: return "Abrir Torre"
        }
    }
}

private extension DoctorArea {
    var rotulo: String {
        switch self {
        case .hub: return "Hub"
        case .aiEngine: return "IA"
        case .approvals: return "Aprovações"
        case .channels: return "Canais"
        case .mcps: return "Integrações"
        case .routines: return "Rotinas"
        case .delegation: return "Delegação"
        }
    }
}

private extension DoctorCheckStatus {
    var rotulo: String {
        switch self {
        case .ok: return "OK"
        case .warning: return "Atenção"
        case .error: return "Erro"
        case .unavailable: return "Indisponível"
        }
    }
}

private extension ForkDoctorPresentation.Headline {
    var titulo: String {
        switch self {
        case .hubOffline: return "Hub offline"
        case .aiDisconnected: return "IA desconectada"
        case .waitingUser: return "Aguardando sua aprovação"
        case .degraded: return "Algumas áreas precisam de atenção"
        case .healthy: return "Tudo funcionando"
        case .unavailable: return "Não foi possível carregar agora"
        }
    }

    var mensagem: String {
        switch self {
        case .hubOffline:
            return "O Hub está inacessível. Isso não é erro de IA: inicie o Hub para ver o estado."
        case .aiDisconnected(let code):
            switch code {
            case "ai_no_active_account": return "Nenhuma conta de IA está ativa."
            case "ai_all_paused": return "Todas as contas de IA estão pausadas."
            case "ai_rate_limited": return "As contas de IA atingiram o limite de uso."
            default: return "A IA não está disponível agora."
            }
        case .waitingUser(let count):
            let pending = count.map { " (\($0) pendências)" } ?? ""
            return "O sistema não está travado: falta só a sua decisão\(pending)."
        case .degraded:
            return "Abra a Torre para ver quais áreas precisam de atenção."
        case .healthy:
            return "Nenhuma ação necessária agora."
        case .unavailable:
            return "Não foi possível carregar o diagnóstico do Hub."
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
        case .needsYou: return "Precisa de você"
        case .observedWorking: return "Trabalhando"
        case .waitingOrBlocked: return "Esperando ou bloqueado"
        case .backlog: return "Backlog"
        case .done: return "Concluído"
        }
    }
}

private extension WorkOriginKind {
    var rotulo: String {
        switch self {
        case .ownerDirect: return "Pedido direto seu"
        case .delegated: return "Delegado"
        case .routine: return "Rotina"
        case .unknown: return "Origem não informada"
        }
    }
}

private extension WorkBlockerKind {
    var rotulo: String {
        switch self {
        case .permissionPause: return "Pausado por permissão"
        case .ownerDecision: return "Aguardando decisão sua"
        case .dependency: return "Depende de outra tarefa"
        case .manual: return "Bloqueio manual"
        case .none: return "Sem bloqueio"
        }
    }
}

private extension ObservedExecutionStatus {
    var rotulo: String {
        switch self {
        case .idle: return "Parada"
        case .working: return "Trabalhando"
        case .needsInput: return "Precisa de você"
        case .waiting: return "Esperando"
        case .error: return "Com erro"
        case .unavailable: return "Indisponível"
        }
    }
}

private extension TowerPerspective {
    var rotulo: String {
        switch self {
        case .overview: return "Dono & Berga"
        case .director: return "Tanga"
        case .bot: return "Executor"
        }
    }
}

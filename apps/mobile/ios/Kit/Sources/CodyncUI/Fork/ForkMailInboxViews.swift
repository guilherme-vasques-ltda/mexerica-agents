import CodyncKit
import Foundation
import Observation
import SwiftUI
import UIKit

struct ForkMailInboxMailboxSelection: Equatable, Sendable {
    let mailboxes: [MailInboxMailbox]
    let selectedMailboxId: String?
}

struct ForkMailInboxRowPresentation: Equatable, Sendable, Identifiable {
    let threadId: String
    let subject: String?
    let lastMessageAt: Date?

    var id: String { threadId }

    init(summary: MailInboxThreadSummary) {
        threadId = summary.threadId
        subject = ForkMailInboxText.nonEmpty(summary.subject)
        lastMessageAt = ForkMailInboxText.date(summary.lastMessageAt)
    }
}

struct ForkMailInboxThreadsPresentation: Equatable, Sendable {
    let selection: ForkMailInboxMailboxSelection
    let rows: [ForkMailInboxRowPresentation]
    let canLoadMore: Bool
}

struct ForkMailInboxMessagePresentation: Equatable, Sendable, Identifiable {
    let id: String
    let senderName: String?
    let senderDomain: String?
    let sentAt: Date?
    let body: String

    init(message: MailInboxMessage) {
        id = message.messageId
        senderName = ForkMailInboxText.nonEmpty(message.fromDisplay)
        senderDomain = ForkMailInboxText.nonEmpty(message.fromDomain)
        sentAt = ForkMailInboxText.date(message.sentAt)
        body = message.bodyText
    }
}

struct ForkMailInboxThreadPresentation: Equatable, Sendable {
    let subject: String?
    let messages: [ForkMailInboxMessagePresentation]
    let truncated: Bool

    init(response: MailInboxOpenThreadResponse) {
        subject = ForkMailInboxText.nonEmpty(response.subject)
        messages = response.messages.map(ForkMailInboxMessagePresentation.init(message:))
        truncated = response.truncated
    }
}

enum ForkMailInboxPresentationState: Equatable, Sendable {
    case unavailable
    case notConnected
    case reconnectRequired
    case accessDenied
    case loading
    case emptyInbox(ForkMailInboxMailboxSelection)
    case threads(ForkMailInboxThreadsPresentation)

    @MainActor
    init(store: ForkMailInboxStore, spaceId: String) {
        guard store.featureEnabled else {
            self = .unavailable
            return
        }
        guard store.activeSpaceId == spaceId else {
            self = .loading
            return
        }
        if store.isLoadingStatus {
            self = .loading
            return
        }
        switch store.status {
        case nil:
            self = Self.stateWithoutData(store.lastError)
        case .notConnected?:
            self = .notConnected
        case .reconnectRequired?:
            self = .reconnectRequired
        case .unavailable?:
            self = .unavailable
        case .ready(let mailboxes)?:
            let selection = ForkMailInboxMailboxSelection(mailboxes: mailboxes, selectedMailboxId: store.selectedMailboxId)
            if mailboxes.isEmpty {
                self = .notConnected
            } else if store.isEmptyInbox {
                self = .emptyInbox(selection)
            } else if store.threads.isEmpty && !store.hasLoadedFirstPage {
                self = Self.stateWithoutData(store.lastError)
            } else {
                self = .threads(ForkMailInboxThreadsPresentation(
                    selection: selection,
                    rows: store.threads.map(ForkMailInboxRowPresentation.init(summary:)),
                    canLoadMore: store.nextCursor != nil
                ))
            }
        }
    }

    static func shouldShowPrivacyMask(scenePhase: ScenePhase, shouldMaskInAppSwitcher: Bool) -> Bool {
        scenePhase != .active && shouldMaskInAppSwitcher
    }

    private static func stateWithoutData(_ error: MailInboxError?) -> ForkMailInboxPresentationState {
        switch error {
        case nil:
            return .loading
        case .accessDenied?:
            return .accessDenied
        case .notConnected?:
            return .notConnected
        case .reconnectRequired?:
            return .reconnectRequired
        case .unavailable?, .invalidRequest?, .cursorInvalid?:
            return .unavailable
        }
    }
}

@MainActor
@Observable
final class ForkMailInboxScreenModel {
    private let store: ForkMailInboxStore
    private let spaceId: String
    private var requestedThreadId: String?
    private var epoch: UInt = 0
    private(set) var presentation: ForkMailInboxPresentationState
    private(set) var shouldMaskInAppSwitcher = false
    private(set) var thread: ForkMailInboxThreadPresentation?
    private(set) var isBusy = false
    var isThreadPresented = false

    init(store: ForkMailInboxStore, spaceId: String) {
        self.store = store
        self.spaceId = spaceId
        presentation = ForkMailInboxPresentationState(store: store, spaceId: spaceId)
        shouldMaskInAppSwitcher = store.activeSpaceId == spaceId && store.shouldMaskInAppSwitcher
    }

    func refresh() async {
        guard !isBusy else { return }
        guard await track({ await store.loadStatus(spaceId: spaceId) }) else { return }
        guard store.activeSpaceId == spaceId, !store.hasLoadedFirstPage,
              case .ready(let mailboxes)? = store.status,
              let target = mailboxes.first(where: { $0.mailboxId == store.selectedMailboxId }) ?? mailboxes.first
        else { return }
        await selectMailbox(target.mailboxId)
    }

    func selectMailbox(_ mailboxId: String) async {
        await track({ await store.loadFirstPage(spaceId: spaceId, mailboxId: mailboxId) })
    }

    func loadNextPage() async {
        await track({ await store.loadNextPage() })
    }

    func open(threadId: String) async {
        guard let mailboxId = store.selectedMailboxId else { return }
        requestedThreadId = threadId
        guard await track({ await store.openThread(spaceId: spaceId, mailboxId: mailboxId, threadId: threadId) }) else { return }
        if store.lastError != nil {
            requestedThreadId = nil
        }
        isThreadPresented = thread != nil
    }

    func leave() {
        epoch &+= 1
        isBusy = false
        requestedThreadId = nil
        if store.activeSpaceId == spaceId {
            store.resetForContextChange()
        }
        refreshSnapshot()
    }

    private func track(_ operation: () async -> Void) async -> Bool {
        let token = epoch
        isBusy = true
        await operation()
        guard token == epoch else { return false }
        isBusy = false
        refreshSnapshot()
        return true
    }

    private func refreshSnapshot() {
        presentation = ForkMailInboxPresentationState(store: store, spaceId: spaceId)
        shouldMaskInAppSwitcher = store.activeSpaceId == spaceId && store.shouldMaskInAppSwitcher
        thread = currentThread()
        if thread == nil {
            isThreadPresented = false
        }
    }

    private func currentThread() -> ForkMailInboxThreadPresentation? {
        guard store.activeSpaceId == spaceId,
              store.lastError == nil,
              let response = store.openThread,
              response.threadId == requestedThreadId else { return nil }
        return ForkMailInboxThreadPresentation(response: response)
    }
}

struct ForkMailInboxScreen: View {
    let store: ForkMailInboxStore
    let spaceId: String

    var body: some View {
        ForkMailInboxSession(store: store, spaceId: spaceId)
            .id(ForkMailInboxSessionKey(store: ObjectIdentifier(store), spaceId: spaceId))
    }
}

private struct ForkMailInboxSessionKey: Hashable {
    let store: ObjectIdentifier
    let spaceId: String
}

private struct ForkMailInboxSession: View {
    @State private var model: ForkMailInboxScreenModel

    init(store: ForkMailInboxStore, spaceId: String) {
        _model = State(initialValue: ForkMailInboxScreenModel(store: store, spaceId: spaceId))
    }

    var body: some View {
        @Bindable var bindableModel = model
        NavigationStack {
            content
                .navigationTitle(ForkMailInboxCopy.title.text)
                .task { await model.refresh() }
                .onDisappear { model.leave() }
        }
        .modifier(ForkMailInboxPrivacyMask(shouldMaskInAppSwitcher: model.shouldMaskInAppSwitcher))
        .sheet(isPresented: $bindableModel.isThreadPresented) {
            ForkMailInboxThreadSheet(model: model)
        }
    }

    @ViewBuilder
    private var content: some View {
        switch model.presentation {
        case .unavailable:
            ForkMailInboxPlaceholder(title: .unavailableTitle, message: .unavailableMessage, systemImage: "exclamationmark.triangle")
        case .notConnected:
            ForkMailInboxPlaceholder(title: .notConnectedTitle, message: .notConnectedMessage, systemImage: "tray")
        case .reconnectRequired:
            ForkMailInboxPlaceholder(title: .reconnectTitle, message: .reconnectMessage, systemImage: "arrow.triangle.2.circlepath")
        case .accessDenied:
            ForkMailInboxPlaceholder(title: .accessDeniedTitle, message: .accessDeniedMessage, systemImage: "lock")
        case .loading:
            ProgressView()
        case .emptyInbox(let selection):
            ForkMailInboxMailboxList(model: model, selection: selection, rows: [], canLoadMore: false, showsEmptyInbox: true)
        case .threads(let threads):
            ForkMailInboxMailboxList(model: model, selection: threads.selection, rows: threads.rows, canLoadMore: threads.canLoadMore, showsEmptyInbox: false)
        }
    }
}

private struct ForkMailInboxMailboxList: View {
    let model: ForkMailInboxScreenModel
    let selection: ForkMailInboxMailboxSelection
    let rows: [ForkMailInboxRowPresentation]
    let canLoadMore: Bool
    let showsEmptyInbox: Bool

    var body: some View {
        List {
            Section {
                Picker(ForkMailInboxCopy.mailbox.text, selection: mailboxBinding) {
                    ForEach(selection.mailboxes) { mailbox in
                        Text(verbatim: mailbox.label).tag(mailbox.mailboxId)
                    }
                }
                .disabled(model.isBusy)
            }
            if showsEmptyInbox {
                Section {
                    Text(verbatim: ForkMailInboxCopy.emptyInbox.text)
                        .foregroundStyle(.secondary)
                }
            }
            Section {
                ForEach(rows) { row in
                    Button {
                        Task { await model.open(threadId: row.threadId) }
                    } label: {
                        ForkMailInboxRowLabel(row: row)
                    }
                    .disabled(model.isBusy)
                }
            }
            if canLoadMore {
                Section {
                    Button(ForkMailInboxCopy.loadMore.text) {
                        Task { await model.loadNextPage() }
                    }
                    .disabled(model.isBusy)
                }
            }
            if model.isBusy {
                Section {
                    ProgressView()
                        .frame(maxWidth: .infinity)
                }
            }
        }
    }

    private var mailboxBinding: Binding<String> {
        Binding(
            get: { selection.selectedMailboxId ?? "" },
            set: { mailboxId in
                Task { await model.selectMailbox(mailboxId) }
            }
        )
    }
}

private struct ForkMailInboxRowLabel: View {
    let row: ForkMailInboxRowPresentation

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(verbatim: row.subject ?? ForkMailInboxCopy.noSubject.text)
                .font(.headline)
                .lineLimit(2)
            if let date = row.lastMessageAt {
                Text(verbatim: date.formatted(date: .abbreviated, time: .shortened))
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
    }
}

private struct ForkMailInboxThreadSheet: View {
    let model: ForkMailInboxScreenModel

    var body: some View {
        NavigationStack {
            Group {
                if let thread = model.thread {
                    ForkMailInboxThreadContent(thread: thread)
                        .navigationTitle(thread.subject ?? ForkMailInboxCopy.noSubject.text)
                }
            }
            .navigationBarTitleDisplayMode(.inline)
        }
        .modifier(ForkMailInboxPrivacyMask(shouldMaskInAppSwitcher: model.shouldMaskInAppSwitcher))
    }
}

private struct ForkMailInboxThreadContent: View {
    @Environment(\.dismiss) var dismiss
    let thread: ForkMailInboxThreadPresentation

    var body: some View {
        List {
            if thread.truncated {
                Section {
                    Text(verbatim: ForkMailInboxCopy.truncated.text)
                        .foregroundStyle(.secondary)
                }
            }
            ForEach(thread.messages) { message in
                Section {
                    ForkMailInboxMessageView(message: message)
                }
            }
        }
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button(ForkMailInboxCopy.close.text) { dismiss() }
            }
        }
    }
}

private struct ForkMailInboxMessageView: View {
    let message: ForkMailInboxMessagePresentation

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(verbatim: message.senderName ?? ForkMailInboxCopy.noSender.text)
                .font(.headline)
            if let domain = message.senderDomain {
                Text(verbatim: "\(ForkMailInboxCopy.declaredDomain.text): \(domain)")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            if let sentAt = message.sentAt {
                Text(verbatim: sentAt.formatted(date: .abbreviated, time: .shortened))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Text(verbatim: message.body)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct ForkMailInboxPlaceholder: View {
    let title: ForkMailInboxCopy
    let message: ForkMailInboxCopy
    let systemImage: String

    var body: some View {
        ContentUnavailableView(
            title.text,
            systemImage: systemImage,
            description: Text(verbatim: message.text)
        )
    }
}

private struct ForkMailInboxPrivacyMask: ViewModifier {
    @Environment(\.scenePhase) var scenePhase
    let shouldMaskInAppSwitcher: Bool

    func body(content: Content) -> some View {
        content.overlay {
            if ForkMailInboxPresentationState.shouldShowPrivacyMask(
                scenePhase: scenePhase,
                shouldMaskInAppSwitcher: shouldMaskInAppSwitcher
            ) {
                ZStack {
                    Color(uiColor: .systemBackground)
                    Text(verbatim: ForkMailInboxCopy.protectedContent.text)
                        .font(.headline)
                        .foregroundStyle(.secondary)
                }
                .ignoresSafeArea()
            }
        }
    }
}

private enum ForkMailInboxText {
    static func nonEmpty(_ value: String?) -> String? {
        guard let value, !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
        return value
    }

    static func date(_ raw: String) -> Date? {
        if let date = try? Date.ISO8601FormatStyle().parse(raw) { return date }
        return try? Date.ISO8601FormatStyle(includingFractionalSeconds: true).parse(raw)
    }
}

private enum ForkMailInboxCopy {
    case title, mailbox, noSubject, loadMore, emptyInbox, noSender, declaredDomain, truncated, protectedContent, close
    case unavailableTitle, unavailableMessage, notConnectedTitle, notConnectedMessage, reconnectTitle, reconnectMessage, accessDeniedTitle, accessDeniedMessage

    var text: String {
        switch self {
        case .title: return String(localized: "Caixa de e-mail", bundle: .module)
        case .mailbox: return String(localized: "Caixa", bundle: .module)
        case .noSubject: return String(localized: "Sem assunto", bundle: .module)
        case .loadMore: return String(localized: "Carregar mais", bundle: .module)
        case .emptyInbox: return String(localized: "Caixa vazia", bundle: .module)
        case .noSender: return String(localized: "Remetente não informado", bundle: .module)
        case .declaredDomain: return String(localized: "Domínio declarado", bundle: .module)
        case .truncated: return String(localized: "Conteúdo truncado: o limite de leitura foi atingido.", bundle: .module)
        case .protectedContent: return String(localized: "Conteúdo protegido", bundle: .module)
        case .close: return String(localized: "Fechar", bundle: .module)
        case .unavailableTitle: return String(localized: "Recurso indisponível", bundle: .module)
        case .unavailableMessage: return String(localized: "Esta área não está disponível agora.", bundle: .module)
        case .notConnectedTitle: return String(localized: "Caixa não conectada", bundle: .module)
        case .notConnectedMessage: return String(localized: "Não há caixa conectada para mostrar.", bundle: .module)
        case .reconnectTitle: return String(localized: "Reconexão necessária", bundle: .module)
        case .reconnectMessage: return String(localized: "Reconecte a caixa para continuar.", bundle: .module)
        case .accessDeniedTitle: return String(localized: "Acesso negado", bundle: .module)
        case .accessDeniedMessage: return String(localized: "Você não tem acesso a esta caixa.", bundle: .module)
        }
    }
}

#if DEBUG
actor ForkMailInboxFixtureTransport: MailInboxTransport {
    enum Availability: Sendable {
        case ready
        case offline
        case notConnected
        case reconnectRequired
    }

    static let spaceA = "fixture-space-a"
    static let spaceB = "fixture-space-b"
    static let emptyMailbox = MailInboxMailbox(mailboxId: "fixture-mailbox-empty", label: "Caixa de teste vazia")
    static let threadMailbox = MailInboxMailbox(mailboxId: "fixture-mailbox-thread", label: "Caixa de teste com fio")
    static let secondPageCursor = "fixture-page-2"

    static let threadSummaries: [MailInboxThreadSummary] = [
        MailInboxThreadSummary(threadId: "fixture-thread-1", subject: "Assunto fictício de teste", lastMessageAt: "2026-11-20T12:05:00Z"),
        MailInboxThreadSummary(threadId: "fixture-thread-2", subject: nil, lastMessageAt: "2026-11-19T09:00:00Z"),
    ]

    static let threadDetails: [String: MailInboxOpenThreadResponse] = [
        "fixture-thread-1": MailInboxOpenThreadResponse(
            threadId: "fixture-thread-1",
            subject: "Assunto fictício de teste",
            messages: [
                MailInboxMessage(
                    messageId: "fixture-message-1",
                    fromDisplay: "Remetente Fictício",
                    fromDomain: "remetente.example.invalid",
                    sentAt: "2026-11-20T12:00:00Z",
                    bodyText: "Primeira mensagem fictícia.\nSegunda linha."
                ),
                MailInboxMessage(
                    messageId: "fixture-message-2",
                    fromDisplay: nil,
                    fromDomain: "example.invalid",
                    sentAt: "2026-11-20T12:05:00Z",
                    bodyText: "Resposta fictícia com [link](https://example.invalid) e *texto*."
                ),
            ],
            truncated: false
        ),
        "fixture-thread-2": MailInboxOpenThreadResponse(
            threadId: "fixture-thread-2",
            subject: nil,
            messages: [
                MailInboxMessage(
                    messageId: "fixture-message-3",
                    fromDisplay: "Pessoa de Teste",
                    fromDomain: nil,
                    sentAt: "2026-11-19T09:00:00Z",
                    bodyText: "Mensagem fictícia única."
                ),
            ],
            truncated: false
        ),
    ]

    private(set) var callCount = 0
    private var availability = Availability.ready

    func setAvailability(_ value: Availability) {
        availability = value
    }

    func mailInboxStatus(spaceId: String) async throws -> MailInboxStatus {
        callCount += 1
        switch availability {
        case .ready:
            break
        case .offline:
            throw MailInboxError.unavailable
        case .notConnected:
            return .notConnected
        case .reconnectRequired:
            return .reconnectRequired
        }
        switch spaceId {
        case Self.spaceA:
            return .ready(mailboxes: [Self.emptyMailbox])
        case Self.spaceB:
            return .ready(mailboxes: [Self.threadMailbox])
        default:
            throw MailInboxError.accessDenied
        }
    }

    func mailInboxListThreads(spaceId: String, mailboxId: String, cursor: String?) async throws -> MailInboxListThreadsResponse {
        callCount += 1
        try authorize(spaceId: spaceId, mailboxId: mailboxId)
        guard mailboxId == Self.threadMailbox.mailboxId else {
            return MailInboxListThreadsResponse(threads: [], nextCursor: nil)
        }
        if cursor == nil {
            return MailInboxListThreadsResponse(threads: [Self.threadSummaries[0]], nextCursor: Self.secondPageCursor)
        }
        guard cursor == Self.secondPageCursor else { throw MailInboxError.cursorInvalid }
        return MailInboxListThreadsResponse(threads: [Self.threadSummaries[1]], nextCursor: nil)
    }

    func mailInboxOpenThread(spaceId: String, mailboxId: String, threadId: String) async throws -> MailInboxOpenThreadResponse {
        callCount += 1
        try authorize(spaceId: spaceId, mailboxId: mailboxId)
        guard mailboxId == Self.threadMailbox.mailboxId, let response = Self.threadDetails[threadId] else {
            throw MailInboxError.accessDenied
        }
        return response
    }

    private func authorize(spaceId: String, mailboxId: String) throws {
        switch availability {
        case .ready:
            break
        case .offline:
            throw MailInboxError.unavailable
        case .notConnected:
            throw MailInboxError.notConnected
        case .reconnectRequired:
            throw MailInboxError.reconnectRequired
        }
        let allowed = (spaceId == Self.spaceA && mailboxId == Self.emptyMailbox.mailboxId)
            || (spaceId == Self.spaceB && mailboxId == Self.threadMailbox.mailboxId)
        guard allowed else { throw MailInboxError.accessDenied }
    }
}

#Preview("Caixa com fio") {
    ForkMailInboxScreen(
        store: ForkMailInboxStore(transport: ForkMailInboxFixtureTransport(), featureEnabled: true),
        spaceId: ForkMailInboxFixtureTransport.spaceB
    )
}

#Preview("Caixa vazia") {
    ForkMailInboxScreen(
        store: ForkMailInboxStore(transport: ForkMailInboxFixtureTransport(), featureEnabled: true),
        spaceId: ForkMailInboxFixtureTransport.spaceA
    )
}

#Preview("Recurso desligado") {
    ForkMailInboxScreen(
        store: ForkMailInboxStore(transport: ForkMailInboxFixtureTransport()),
        spaceId: ForkMailInboxFixtureTransport.spaceB
    )
}
#endif

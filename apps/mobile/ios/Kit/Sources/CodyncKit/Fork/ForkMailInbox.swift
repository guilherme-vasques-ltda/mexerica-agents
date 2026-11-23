import Foundation

public enum MailInboxError: String, Error, Sendable, Equatable, Codable, CustomStringConvertible, CustomDebugStringConvertible {
    case invalidRequest = "invalid_request"
    case accessDenied = "access_denied"
    case notConnected = "not_connected"
    case reconnectRequired = "reconnect_required"
    case unavailable = "unavailable"
    case cursorInvalid = "cursor_invalid"

    public var description: String { "MailInboxError(\(rawValue))" }
    public var debugDescription: String { description }
}

public struct MailInboxMailbox: Sendable, Equatable, Codable, Identifiable, CustomStringConvertible, CustomDebugStringConvertible {
    public let mailboxId: String
    public let label: String

    public var id: String { mailboxId }

    public init(mailboxId: String, label: String) {
        self.mailboxId = mailboxId
        self.label = label
    }

    public var description: String { "MailInboxMailbox(redacted)" }
    public var debugDescription: String { description }
}

public enum MailInboxStatus: Sendable, Equatable, CustomStringConvertible, CustomDebugStringConvertible {
    case ready(mailboxes: [MailInboxMailbox])
    case notConnected
    case reconnectRequired
    case unavailable

    public var isReady: Bool {
        if case .ready = self { return true }
        return false
    }

    public var hasMailboxes: Bool {
        if case .ready(let mailboxes) = self { return !mailboxes.isEmpty }
        return false
    }

    public var description: String {
        switch self {
        case .ready(let mailboxes):
            return "MailInboxStatus.ready(count: \(mailboxes.count))"
        case .notConnected:
            return "MailInboxStatus.not_connected"
        case .reconnectRequired:
            return "MailInboxStatus.reconnect_required"
        case .unavailable:
            return "MailInboxStatus.unavailable"
        }
    }

    public var debugDescription: String { description }
}

public struct MailInboxThreadSummary: Sendable, Equatable, Codable, Identifiable, CustomStringConvertible, CustomDebugStringConvertible {
    public let threadId: String
    public let subject: String?
    public let lastMessageAt: String

    public var id: String { threadId }

    public init(threadId: String, subject: String?, lastMessageAt: String) {
        self.threadId = threadId
        self.subject = subject
        self.lastMessageAt = lastMessageAt
    }

    public var description: String { "MailInboxThreadSummary(redacted)" }
    public var debugDescription: String { description }
}

public struct MailInboxListThreadsResponse: Sendable, Equatable, Codable, CustomStringConvertible, CustomDebugStringConvertible {
    public let threads: [MailInboxThreadSummary]
    public let nextCursor: String?

    public init(threads: [MailInboxThreadSummary], nextCursor: String?) {
        self.threads = threads
        self.nextCursor = nextCursor
    }

    public var description: String {
        "MailInboxListThreadsResponse(count: \(threads.count), hasNext: \(nextCursor != nil))"
    }

    public var debugDescription: String { description }
}

public struct MailInboxSenderPresentation: Sendable, Equatable, CustomStringConvertible, CustomDebugStringConvertible {
    public let displayName: String?
    public let declaredDomain: String?
    public let isVerifiedIdentity: Bool

    public var hasDeclaredDomain: Bool { declaredDomain != nil }

    public init(displayName: String?, declaredDomain: String?) {
        self.displayName = displayName
        self.declaredDomain = declaredDomain
        self.isVerifiedIdentity = false
    }

    public var description: String {
        "MailInboxSenderPresentation(hasDisplay: \(displayName != nil), hasDomain: \(declaredDomain != nil), verified: \(isVerifiedIdentity))"
    }

    public var debugDescription: String { description }
}

public struct MailInboxMessage: Sendable, Equatable, Codable, Identifiable, CustomStringConvertible, CustomDebugStringConvertible {
    public let messageId: String
    public let fromDisplay: String?
    public let fromDomain: String?
    public let sentAt: String
    public let bodyText: String

    public var id: String { messageId }

    public var senderPresentation: MailInboxSenderPresentation {
        MailInboxSenderPresentation(displayName: fromDisplay, declaredDomain: fromDomain)
    }

    public init(
        messageId: String,
        fromDisplay: String?,
        fromDomain: String?,
        sentAt: String,
        bodyText: String
    ) {
        self.messageId = messageId
        self.fromDisplay = fromDisplay
        self.fromDomain = fromDomain
        self.sentAt = sentAt
        self.bodyText = bodyText
    }

    public var description: String { "MailInboxMessage(redacted)" }
    public var debugDescription: String { description }
}

public struct MailInboxOpenThreadResponse: Sendable, Equatable, Codable, CustomStringConvertible, CustomDebugStringConvertible {
    public let threadId: String
    public let subject: String?
    public let messages: [MailInboxMessage]
    public let truncated: Bool

    public init(threadId: String, subject: String?, messages: [MailInboxMessage], truncated: Bool) {
        self.threadId = threadId
        self.subject = subject
        self.messages = messages
        self.truncated = truncated
    }

    public var description: String {
        "MailInboxOpenThreadResponse(messageCount: \(messages.count), truncated: \(truncated))"
    }

    public var debugDescription: String { description }
}

public enum MailInboxSanitizer {
    private static func isBidiControl(_ value: UInt32) -> Bool {
        switch value {
        case 0x202A...0x202E, 0x2066...0x2069, 0x200E, 0x200F, 0x061C:
            return true
        default:
            return false
        }
    }

    private static func isTagCharacter(_ value: UInt32) -> Bool {
        (0xE0000...0xE007F).contains(value)
    }

    private static func isLineOrParagraphSeparator(_ value: UInt32) -> Bool {
        value == 0x2028 || value == 0x2029
    }

    private static func isNonTextualControl(_ value: UInt32, allowLineBreaksAndTabs: Bool) -> Bool {
        if allowLineBreaksAndTabs && (value == 0x000A || value == 0x0009) {
            return false
        }
        switch value {
        case 0x0000...0x001F, 0x007F...0x009F:
            return true
        default:
            return false
        }
    }

    public static func sanitizeSingleLine(_ raw: String?) -> String? {
        guard let raw else { return nil }
        var scalars = String.UnicodeScalarView()
        scalars.reserveCapacity(raw.unicodeScalars.count)
        for scalar in raw.unicodeScalars {
            let v = scalar.value
            if scalar.properties.generalCategory == .format
                || isTagCharacter(v)
                || isLineOrParagraphSeparator(v)
                || isNonTextualControl(v, allowLineBreaksAndTabs: false) {
                continue
            }
            scalars.append(scalar)
        }
        return String(scalars)
    }

    public static func sanitizeDomain(_ raw: String?) -> String? {
        guard let cleaned = sanitizeSingleLine(raw)?
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased(),
            !cleaned.isEmpty else {
            return nil
        }
        guard let ascii = idnaToASCII(cleaned), !ascii.isEmpty else {
            return nil
        }
        let isValidChars = ascii.unicodeScalars.allSatisfy { s in
            (s.value >= 0x61 && s.value <= 0x7A)
                || (s.value >= 0x30 && s.value <= 0x39)
                || s.value == 0x2E
                || s.value == 0x2D
        }
        guard isValidChars,
              !ascii.hasPrefix("."),
              !ascii.hasSuffix("."),
              !ascii.hasPrefix("-"),
              !ascii.hasSuffix("-"),
              !ascii.contains("..") else {
            return nil
        }
        return ascii
    }

    private static func idnaToASCII(_ domain: String) -> String? {
        let labels = domain.split(separator: ".", omittingEmptySubsequences: false)
        var encodedLabels: [String] = []
        encodedLabels.reserveCapacity(labels.count)
        for labelSub in labels {
            let label = String(labelSub)
            if label.isEmpty { return nil }
            if label.unicodeScalars.allSatisfy({ $0.value < 0x80 }) {
                encodedLabels.append(label)
            } else {
                guard let puny = punycodeEncode(label) else { return nil }
                encodedLabels.append("xn--" + puny)
            }
        }
        return encodedLabels.joined(separator: ".")
    }

    private static func punycodeEncode(_ input: String) -> String? {
        let base = 36
        let tMin = 1
        let tMax = 26
        let skew = 38
        let damp = 700
        let initialBias = 72
        let initialN = 128

        let scalars = input.unicodeScalars.map { Int($0.value) }
        var output = ""
        for cp in scalars where cp < 0x80 {
            guard let scalar = UnicodeScalar(cp) else { return nil }
            output.append(Character(scalar))
        }
        let basicCount = output.unicodeScalars.count
        var handled = basicCount
        if basicCount > 0 {
            output.append("-")
        }

        var n = initialN
        var delta = 0
        var bias = initialBias

        func encodeDigit(_ d: Int) -> Character {
            if d < 26 {
                return Character(UnicodeScalar(0x61 + d)!)
            }
            return Character(UnicodeScalar(0x30 + (d - 26))!)
        }

        while handled < scalars.count {
            guard let m = scalars.filter({ $0 >= n }).min() else { break }
            let diff = m - n
            if diff > (Int.max - delta) / (handled + 1) { return nil }
            delta += diff * (handled + 1)
            n = m

            for cp in scalars {
                if cp < n {
                    delta += 1
                    if delta == 0 { return nil }
                } else if cp == n {
                    var q = delta
                    var k = base
                    while true {
                        let t = k <= bias ? tMin : (k >= bias + tMax ? tMax : k - bias)
                        if q < t { break }
                        output.append(encodeDigit(t + ((q - t) % (base - t))))
                        q = (q - t) / (base - t)
                        k += base
                    }
                    output.append(encodeDigit(q))
                    var d = delta / (handled == basicCount ? damp : 2)
                    d += d / (handled + 1)
                    var kBias = 0
                    while d > ((base - tMin) * tMax) / 2 {
                        d /= (base - tMin)
                        kBias += base
                    }
                    bias = kBias + (((base - tMin + 1) * d) / (d + skew))
                    delta = 0
                    handled += 1
                }
            }
            delta += 1
            n += 1
        }
        return output
    }

    public static func sanitizeBodyText(_ raw: String) -> String {
        let normalized = raw
            .replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")
            .replacingOccurrences(of: "\u{2028}", with: "\n")
            .replacingOccurrences(of: "\u{2029}", with: "\n")
        var scalars = String.UnicodeScalarView()
        scalars.reserveCapacity(normalized.unicodeScalars.count)
        for scalar in normalized.unicodeScalars {
            let v = scalar.value
            if isBidiControl(v)
                || isTagCharacter(v)
                || v == 0x200B
                || v == 0xFEFF
                || isNonTextualControl(v, allowLineBreaksAndTabs: true) {
                continue
            }
            scalars.append(scalar)
        }
        return String(scalars)
    }

    public static func sanitizeThreadSummary(_ item: MailInboxThreadSummary) -> MailInboxThreadSummary {
        MailInboxThreadSummary(
            threadId: item.threadId,
            subject: sanitizeSingleLine(item.subject),
            lastMessageAt: item.lastMessageAt
        )
    }

    public static func sanitizeMessage(_ item: MailInboxMessage) -> MailInboxMessage {
        MailInboxMessage(
            messageId: item.messageId,
            fromDisplay: sanitizeSingleLine(item.fromDisplay),
            fromDomain: sanitizeDomain(item.fromDomain),
            sentAt: item.sentAt,
            bodyText: sanitizeBodyText(item.bodyText)
        )
    }

    public static func sanitizeOpenThread(_ response: MailInboxOpenThreadResponse) -> MailInboxOpenThreadResponse {
        MailInboxOpenThreadResponse(
            threadId: response.threadId,
            subject: sanitizeSingleLine(response.subject),
            messages: response.messages.map(sanitizeMessage),
            truncated: response.truncated
        )
    }
}

public protocol MailInboxTransport: Sendable {
    func mailInboxStatus(spaceId: String) async throws -> MailInboxStatus
    func mailInboxListThreads(
        spaceId: String,
        mailboxId: String,
        cursor: String?
    ) async throws -> MailInboxListThreadsResponse
    func mailInboxOpenThread(
        spaceId: String,
        mailboxId: String,
        threadId: String
    ) async throws -> MailInboxOpenThreadResponse
}

@MainActor
public final class ForkMailInboxStore: CustomStringConvertible, CustomDebugStringConvertible {
    private let transport: MailInboxTransport
    public let featureEnabled: Bool

    public private(set) var generation: UInt64 = 0
    private var pageRequestId: UInt64 = 0
    private var detailRequestId: UInt64 = 0
    public private(set) var activeSpaceId: String?
    public private(set) var selectedMailboxId: String?
    public private(set) var status: MailInboxStatus?
    public private(set) var isLoadingStatus: Bool = false
    public private(set) var isLoadingThreads: Bool = false
    public private(set) var isLoadingThreadDetail: Bool = false
    public private(set) var threads: [MailInboxThreadSummary] = []
    public private(set) var nextCursor: String?
    public private(set) var hasLoadedFirstPage: Bool = false
    public private(set) var openThread: MailInboxOpenThreadResponse?
    public private(set) var lastError: MailInboxError?

    public init(transport: MailInboxTransport, featureEnabled: Bool = false) {
        self.transport = transport
        self.featureEnabled = featureEnabled
    }

    public var isEmptyInbox: Bool {
        lastError == nil
            && !isLoadingStatus
            && !isLoadingThreads
            && status?.isReady == true
            && hasLoadedFirstPage
            && threads.isEmpty
            && nextCursor == nil
    }

    public var shouldMaskInAppSwitcher: Bool {
        !threads.isEmpty || openThread != nil || (status?.hasMailboxes == true)
    }

    nonisolated public var description: String {
        "ForkMailInboxStore(redacted)"
    }

    nonisolated public var debugDescription: String { description }

    public func resetForContextChange() {
        generation &+= 1
        pageRequestId &+= 1
        detailRequestId &+= 1
        activeSpaceId = nil
        selectedMailboxId = nil
        status = nil
        isLoadingStatus = false
        isLoadingThreads = false
        isLoadingThreadDetail = false
        threads = []
        nextCursor = nil
        hasLoadedFirstPage = false
        openThread = nil
        lastError = nil
    }

    public func loadStatus(spaceId: String) async {
        guard featureEnabled else {
            status = .unavailable
            lastError = .unavailable
            return
        }
        guard let cleanSpaceId = Self.nonEmpty(spaceId) else {
            lastError = .invalidRequest
            return
        }

        if activeSpaceId != cleanSpaceId {
            resetForContextChange()
            activeSpaceId = cleanSpaceId
        }

        let requestGeneration = generation
        isLoadingStatus = true
        lastError = nil

        do {
            let result = try await transport.mailInboxStatus(spaceId: cleanSpaceId)
            guard generation == requestGeneration, activeSpaceId == cleanSpaceId else { return }
            isLoadingStatus = false
            switch result {
            case .ready(let mailboxes):
                let sanitized = mailboxes.map {
                    MailInboxMailbox(
                        mailboxId: $0.mailboxId,
                        label: MailInboxSanitizer.sanitizeSingleLine($0.label) ?? ""
                    )
                }
                status = .ready(mailboxes: sanitized)
                lastError = nil
            case .notConnected:
                status = .notConnected
                lastError = .notConnected
            case .reconnectRequired:
                status = .reconnectRequired
                lastError = .reconnectRequired
            case .unavailable:
                status = .unavailable
                lastError = .unavailable
            }
        } catch {
            if error is CancellationError {
                if generation == requestGeneration && activeSpaceId == cleanSpaceId {
                    isLoadingStatus = false
                }
                return
            }
            guard generation == requestGeneration, activeSpaceId == cleanSpaceId else { return }
            isLoadingStatus = false
            let mapped = Self.mapError(error)
            if mapped == .unavailable || mapped == .notConnected || mapped == .reconnectRequired {
                status = Self.statusFromError(mapped)
            }
            lastError = mapped
        }
    }

    public func loadFirstPage(spaceId: String, mailboxId: String) async {
        guard featureEnabled else {
            status = .unavailable
            lastError = .unavailable
            return
        }
        guard let cleanSpaceId = Self.nonEmpty(spaceId),
              let cleanMailboxId = Self.nonEmpty(mailboxId) else {
            lastError = .invalidRequest
            return
        }

        if activeSpaceId != cleanSpaceId {
            resetForContextChange()
            activeSpaceId = cleanSpaceId
        } else {
            generation &+= 1
            pageRequestId &+= 1
            detailRequestId &+= 1
        }

        selectedMailboxId = cleanMailboxId
        threads = []
        nextCursor = nil
        hasLoadedFirstPage = false
        openThread = nil
        lastError = nil

        await fetchPage(spaceId: cleanSpaceId, mailboxId: cleanMailboxId, cursor: nil, isRestartFromCursorInvalid: false)
    }

    public func loadNextPage() async {
        guard featureEnabled else {
            status = .unavailable
            lastError = .unavailable
            return
        }
        guard !isLoadingThreads,
              let spaceId = activeSpaceId,
              let mailboxId = selectedMailboxId,
              let cursor = nextCursor else {
            return
        }
        await fetchPage(spaceId: spaceId, mailboxId: mailboxId, cursor: cursor, isRestartFromCursorInvalid: false)
    }

    public func openThread(spaceId: String, mailboxId: String, threadId: String) async {
        guard featureEnabled else {
            status = .unavailable
            lastError = .unavailable
            return
        }
        guard let cleanSpaceId = Self.nonEmpty(spaceId),
              let cleanMailboxId = Self.nonEmpty(mailboxId),
              let cleanThreadId = Self.nonEmpty(threadId) else {
            lastError = .invalidRequest
            return
        }

        if activeSpaceId != cleanSpaceId {
            resetForContextChange()
            activeSpaceId = cleanSpaceId
            selectedMailboxId = cleanMailboxId
        } else if selectedMailboxId == nil {
            selectedMailboxId = cleanMailboxId
        }

        detailRequestId &+= 1
        let capturedDetailRequestId = detailRequestId
        let requestGeneration = generation
        isLoadingThreadDetail = true
        lastError = nil

        do {
            let response = try await transport.mailInboxOpenThread(
                spaceId: cleanSpaceId,
                mailboxId: cleanMailboxId,
                threadId: cleanThreadId
            )
            guard capturedDetailRequestId == detailRequestId,
                  generation == requestGeneration,
                  activeSpaceId == cleanSpaceId,
                  selectedMailboxId == cleanMailboxId else { return }
            isLoadingThreadDetail = false
            openThread = MailInboxSanitizer.sanitizeOpenThread(response)
            lastError = nil
        } catch {
            if error is CancellationError {
                if capturedDetailRequestId == detailRequestId &&
                    generation == requestGeneration &&
                    activeSpaceId == cleanSpaceId &&
                    selectedMailboxId == cleanMailboxId {
                    isLoadingThreadDetail = false
                }
                return
            }
            guard capturedDetailRequestId == detailRequestId,
                  generation == requestGeneration,
                  activeSpaceId == cleanSpaceId,
                  selectedMailboxId == cleanMailboxId else { return }
            isLoadingThreadDetail = false
            lastError = Self.mapError(error)
        }
    }

    private func fetchPage(
        spaceId: String,
        mailboxId: String,
        cursor: String?,
        isRestartFromCursorInvalid: Bool
    ) async {
        pageRequestId &+= 1
        let capturedPageRequestId = pageRequestId
        let requestGeneration = generation
        isLoadingThreads = true
        lastError = nil

        do {
            let page = try await transport.mailInboxListThreads(
                spaceId: spaceId,
                mailboxId: mailboxId,
                cursor: cursor
            )
            guard capturedPageRequestId == pageRequestId,
                  generation == requestGeneration,
                  activeSpaceId == spaceId,
                  selectedMailboxId == mailboxId else { return }
            isLoadingThreads = false
            let sanitizedThreads = page.threads.map(MailInboxSanitizer.sanitizeThreadSummary)
            if cursor == nil {
                threads = sanitizedThreads
                hasLoadedFirstPage = true
            } else {
                threads.append(contentsOf: sanitizedThreads)
            }
            nextCursor = page.nextCursor
            lastError = nil
        } catch {
            if error is CancellationError {
                if capturedPageRequestId == pageRequestId &&
                    generation == requestGeneration &&
                    activeSpaceId == spaceId &&
                    selectedMailboxId == mailboxId {
                    isLoadingThreads = false
                }
                return
            }
            guard capturedPageRequestId == pageRequestId,
                  generation == requestGeneration,
                  activeSpaceId == spaceId,
                  selectedMailboxId == mailboxId else { return }
            isLoadingThreads = false
            let mapped = Self.mapError(error)
            if mapped == .cursorInvalid && cursor != nil && !isRestartFromCursorInvalid {
                threads = []
                nextCursor = nil
                hasLoadedFirstPage = false
                await fetchPage(spaceId: spaceId, mailboxId: mailboxId, cursor: nil, isRestartFromCursorInvalid: true)
                return
            }
            lastError = mapped
        }
    }

    private static func nonEmpty(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private static func mapError(_ error: Error) -> MailInboxError {
        if let mailError = error as? MailInboxError {
            return mailError
        }
        return .unavailable
    }

    private static func statusFromError(_ error: MailInboxError) -> MailInboxStatus {
        switch error {
        case .notConnected: return .notConnected
        case .reconnectRequired: return .reconnectRequired
        default: return .unavailable
        }
    }
}

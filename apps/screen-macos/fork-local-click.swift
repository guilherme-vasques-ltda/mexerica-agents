import AppKit
import ApplicationServices
import CoreGraphics
import Darwin

struct ForkClickLatch<T> {
    private var pending: (token: UUID, deadline: UInt64, context: T, ready: Bool)?

    mutating func prepare(_ context: T, deadline: UInt64) -> UUID {
        let token = UUID()
        pending = (token, deadline, context, false)
        return token
    }

    func lookup(_ token: UUID, deadline: UInt64, now: UInt64) -> T? {
        guard let pending, pending.token == token, now > 0,
              now < deadline, deadline <= pending.deadline else { return nil }
        return pending.context
    }

    mutating func markReady(_ token: UUID) {
        if pending?.token == token { pending?.ready = true }
    }

    mutating func consume(_ token: UUID, deadline: UInt64, now: UInt64) -> T? {
        guard let context = lookup(token, deadline: deadline, now: now),
              pending?.ready == true else { return nil }
        pending = nil
        return context
    }

    mutating func invalidate() {
        pending = nil
    }
}

@MainActor
final class ForkLocalClick {
    private struct Selection {
        let display: CGDirectDisplayID
        let app: NSRunningApplication
        let launched: Date
        let window: AXUIElement
        let windowName: String
        let windowBounds: CGRect
        let button: AXUIElement
        let buttonName: String
        let buttonBounds: CGRect
    }

    private var selections = ForkClickLatch<Selection>()
    private static let messageTimeout: Float = 0.1
    private static let changed = "The approved application, window or button changed."

    func handle(_ request: [String: Any], display: CGDirectDisplayID) throws -> [String: Any] {
        guard let method = request["method"] as? String,
              let value = request["deadlineNs"] as? String, let deadline = UInt64(value) else {
            throw HelperError(Self.changed)
        }
        try Self.checkTime(deadline)
        if method == "clickPrepare" {
            guard let app = NSWorkspace.shared.frontmostApplication,
                  let launched = app.launchDate,
                  let bundle = request["bundleId"] as? String, app.bundleIdentifier == bundle,
                  bundle != "app.donnabot.mac.dev",
                  let windowName = request["window"] as? String,
                  let buttonName = request["button"] as? String else { throw HelperError(Self.changed) }
            let (window, windowBounds) = try Self.findWindow(app, windowName, display)
            let (button, buttonBounds) = try Self.findButton(window, buttonName, windowBounds, deadline)
            try Self.checkTime(deadline)
            guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier else {
                throw HelperError(Self.changed)
            }
            let snapshot = Selection(display: display, app: app, launched: launched, window: window,
                                     windowName: windowName, windowBounds: windowBounds,
                                     button: button, buttonName: buttonName, buttonBounds: buttonBounds)
            return ["token": selections.prepare(snapshot, deadline: deadline).uuidString]
        }
        guard let raw = request["token"] as? String, let token = UUID(uuidString: raw) else {
            throw HelperError(Self.changed)
        }
        let commit = method == "clickCommit"
        guard commit || method == "clickReady" else { throw HelperError(Self.changed) }
        let now = clock_gettime_nsec_np(CLOCK_MONOTONIC_RAW)
        let snapshot = commit ? selections.consume(token, deadline: deadline, now: now)
                              : selections.lookup(token, deadline: deadline, now: now)
        guard let selected = snapshot, selected.display == display else { return ["state": "refused"] }
        guard !selected.app.isTerminated,
              let app = NSRunningApplication(processIdentifier: selected.app.processIdentifier),
              app.launchDate == selected.launched, app.bundleIdentifier == selected.app.bundleIdentifier else {
            return ["state": "refused"]
        }
        guard NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier else {
            return ["state": commit ? "refused" : "waiting"]
        }
        do {
            let (window, windowBounds) = try Self.findWindow(app, selected.windowName, display)
            guard CFEqual(window, selected.window), windowBounds == selected.windowBounds else {
                throw HelperError(Self.changed)
            }
            let (button, bounds) = try Self.findButton(window, selected.buttonName, windowBounds, deadline)
            let (currentWindow, currentBounds) = try Self.findWindow(app, selected.windowName, display)
            guard CFEqual(button, selected.button), bounds == selected.buttonBounds,
                  CFEqual(currentWindow, window), currentBounds == windowBounds,
                  let buttonWindow: AXUIElement = Self.read(button, kAXWindowAttribute),
                  CFEqual(buttonWindow, currentWindow), Self.bounds(button) == bounds,
                  try Self.matchesButton(button, name: selected.buttonName),
                  AXIsProcessTrusted(), CGPreflightScreenCaptureAccess(), display == CGMainDisplayID(),
                  !app.isTerminated,
                  NSWorkspace.shared.frontmostApplication?.processIdentifier == app.processIdentifier else {
                throw HelperError(Self.changed)
            }
            try Self.checkTime(deadline)
            if !commit {
                selections.markReady(token)
                return ["state": "ready"]
            }
            guard Self.setTimeout(button) else { throw HelperError(Self.changed) }
            try Self.checkTime(deadline)
            return ["state": AXUIElementPerformAction(button, kAXPressAction as CFString) == .success ? "pressed" : "unknown"]
        } catch {
            selections.invalidate()
            return ["state": "refused"]
        }
    }

    private static func checkTime(_ deadline: UInt64) throws {
        let now = clock_gettime_nsec_np(CLOCK_MONOTONIC_RAW)
        guard now > 0, now < deadline else { throw HelperError("Click approval expired.") }
    }

    private static func findWindow(_ app: NSRunningApplication, _ name: String, _ display: CGDirectDisplayID) throws
        -> (AXUIElement, CGRect) {
        let root = AXUIElementCreateApplication(app.processIdentifier)
        guard let window: AXUIElement = read(root, kAXFocusedWindowAttribute),
              (read(window, kAXRoleAttribute) as String?) == kAXWindowRole,
              (read(window, kAXTitleAttribute) as String?)?.utf8.elementsEqual(name.utf8) == true,
              (read(window, kAXMinimizedAttribute) as Bool?) == false,
              let area = bounds(window), CGDisplayBounds(display).contains(area) else { throw HelperError(changed) }
        return (window, area)
    }

    private static func findButton(_ window: AXUIElement, _ name: String, _ area: CGRect, _ deadline: UInt64) throws
        -> (AXUIElement, CGRect) {
        var nodes = [window]
        var visited = 0
        var found: (AXUIElement, CGRect)?
        while let element = nodes.popLast() {
            try checkTime(deadline)
            visited += 1
            guard visited <= 192, let role: String = read(element, kAXRoleAttribute) else { throw HelperError(changed) }
            if role == kAXButtonRole {
                guard let title: String = read(element, kAXTitleAttribute) else { throw HelperError(changed) }
                if title.utf8.elementsEqual(name.utf8) {
                    guard found == nil, try matchesButton(element, name: name),
                          let actualWindow: AXUIElement = read(element, kAXWindowAttribute), CFEqual(actualWindow, window),
                          let box = bounds(element), area.contains(box) else { throw HelperError(changed) }
                    found = (element, box)
                }
                continue
            }
            if role == "AXWebArea" { continue }
            guard setTimeout(element) else { throw HelperError(changed) }
            var children: CFArray?
            switch AXUIElementCopyAttributeValues(element, kAXChildrenAttribute as CFString, 0, 193, &children) {
            case .noValue, .attributeUnsupported: break
            case .success:
                guard let values = children as? [AXUIElement], visited + nodes.count + values.count <= 192 else {
                    throw HelperError(changed)
                }
                nodes.append(contentsOf: values)
            default: throw HelperError(changed)
            }
        }
        guard let found else { throw HelperError(changed) }
        return found
    }

    private static func matchesButton(_ el: AXUIElement, name: String) throws -> Bool {
        guard (read(el, kAXRoleAttribute) as String?) == kAXButtonRole,
              (read(el, kAXTitleAttribute) as String?)?.utf8.elementsEqual(name.utf8) == true,
              (read(el, kAXEnabledAttribute) as Bool?) == true else { return false }
        var names: CFArray?
        guard setTimeout(el), AXUIElementCopyActionNames(el, &names) == .success, let actions = names as? [String] else {
            throw HelperError(changed)
        }
        return actions.contains(kAXPressAction)
    }

    private static func bounds(_ el: AXUIElement) -> CGRect? {
        guard let position: AXValue = read(el, kAXPositionAttribute), let size: AXValue = read(el, kAXSizeAttribute)
        else { return nil }
        var p = CGPoint.zero
        var s = CGSize.zero
        guard AXValueGetValue(position, .cgPoint, &p), AXValueGetValue(size, .cgSize, &s),
              p.x.isFinite, p.y.isFinite, s.width.isFinite, s.height.isFinite, s.width > 0, s.height > 0 else { return nil }
        return CGRect(origin: p, size: s)
    }

    private static func setTimeout(_ el: AXUIElement) -> Bool {
        AXUIElementSetMessagingTimeout(el, messageTimeout) == .success
    }

    private static func read<T>(_ el: AXUIElement, _ key: String) -> T? {
        var value: CFTypeRef?
        guard setTimeout(el), AXUIElementCopyAttributeValue(el, key as CFString, &value) == .success else { return nil }
        return value as? T
    }
}

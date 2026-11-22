import Foundation

/// The one way the active account changes. Signing in (password, OAuth, Apple, a finished
/// email code, an OAuth callback), switching and signing out, one account or all, go through `run`.
/// Changes run one at a time, start to end, in the order they were asked: being on the main actor
/// doesn't keep one from starting while another awaits. When its turn comes, each reads which account
/// is active now and, if one is, lets that account clean up first (`beforeLeaving`: the access requests
/// it left open) while its session still works, since its cloud client stops once another account is
/// active. Without an active account there is nothing to wait for.
///
/// A browser sign-in may come back through the app's URL (`callback`) while it runs; queued behind it,
/// that callback would wait forever. So each browser sign-in opens its own window, only once its
/// clean-up is done (when the browser can actually answer) and until its Clerk call ends: a callback
/// arriving in that window is part of it and runs at once, and the sign-in doesn't let the next change
/// start before every callback it took in has ended. A callback outside any window (during a clean-up,
/// or after the sign-in ended) is a change of its own: queued, with its clean-up.
@MainActor
public final class AccountTransition {
    public enum Kind: Sendable {
        /// Signs in, switches or signs out.
        case change
        /// Signs in through the browser, which may come back through the app's URL (`callback`).
        case signInWithCallback
        /// A URL back from the browser: part of the browser sign-in whose window is open, if one is;
        /// otherwise a change of its own.
        case callback
    }

    /// Whether an account is active right now.
    public var isAccountActive: @MainActor () -> Bool = { false }
    /// What the active account does before it stops being the active one.
    public private(set) var beforeLeaving: (@MainActor () async -> Void)?
    /// Ends when the last change asked so far has ended: the next one waits for it.
    private var tail: Task<Void, Never>?
    /// The window of the browser sign-in running now, between the end of its clean-up and the end of its
    /// Clerk call; nil otherwise.
    private var window: CallbackWindow?

    public init() {}

    /// The active account's clean-up is its store's: open access requests end, within their deadline
    /// (`AccountStore.settleBeforeLeaving`). `store` is read when a change starts, so it's the current one.
    public func attach(_ store: @escaping @MainActor () -> AccountStore?) {
        beforeLeaving = { await store()?.settleBeforeLeaving() }
    }

    /// Runs `change` (the call that signs in, switches or signs out) once every change asked before it
    /// has ended, after the clean-up of the account active at that point. Ending in any way (an error
    /// or a cancellation too) lets the next one start. Cancelled before its turn or during its clean-up
    /// (which may then not have finished), it throws `CancellationError` without calling `change`.
    @discardableResult
    public func run<T>(_ kind: Kind = .change, _ change: @MainActor () async throws -> T) async throws -> T {
        // Part of the browser sign-in whose window is open: at once, and that sign-in waits for it.
        if kind == .callback, let window {
            window.admit()
            defer { window.leave() }
            return try await change()
        }
        let previous = tail
        let (ended, end) = AsyncStream<Void>.makeStream()
        tail = Task {
            await previous?.value
            var iterator = ended.makeAsyncIterator()
            _ = await iterator.next(isolation: #isolation)
        }
        defer { end.finish() }
        await previous?.value
        // Read now, after the queue: the account active at this point is the one to clean up.
        if isAccountActive(), let beforeLeaving { await beforeLeaving() }
        try Task.checkCancellation()
        guard kind == .signInWithCallback else { return try await change() }

        // The browser can answer from now on, until this sign-in's Clerk call ends.
        let opened = CallbackWindow()
        window = opened
        let result: Result<T, any Error>
        do {
            result = .success(try await change())
        } catch {
            result = .failure(error)
        }
        if window === opened { window = nil }
        // Callbacks taken in still run: the next change waits for them (even if this one was cancelled).
        await opened.drained()
        return try result.get()
    }
}

/// The callbacks one browser sign-in took in, until they have all ended.
@MainActor
private final class CallbackWindow {
    private var running = 0
    private var waiter: CheckedContinuation<Void, Never>?

    func admit() { running += 1 }

    func leave() {
        running -= 1
        if running == 0, let waiter {
            self.waiter = nil
            waiter.resume()
        }
    }

    /// Returns once every callback taken in has ended; not cut short by cancellation.
    func drained() async {
        guard running > 0 else { return }
        await withCheckedContinuation { waiter = $0 }
    }
}

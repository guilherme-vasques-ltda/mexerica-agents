import CodyncKit
import Foundation

struct ForkWorkspaceSession: Equatable {
    let userId: String?
    let sessionId: String?
    let usesDirectory: Bool
}

struct ForkWorkspaceRefreshKey: Equatable {
    let session: ForkWorkspaceSession
    let foreground: Bool
}

extension AppStore {
    var workspaceSession: ForkWorkspaceSession {
        ForkWorkspaceSession(userId: account.userID, sessionId: account.workspaceSessionID,
                             usesDirectory: account.usesWorkspaceDirectory)
    }

    var showsWorkspaceStatus: Bool { account.isSignedIn && account.usesWorkspaceDirectory }

    static func initialStorage(for session: AccountSession) -> SharedStore.Context {
        if session.requiresLogin || session.usesWorkspaceDirectory {
            return SharedStore.Context(accountID: session.userID, scope: "inactive")
        }
        return SharedStore.Context(accountID: session.userID)
    }

    func workspaceRefreshKey(foreground: Bool) -> ForkWorkspaceRefreshKey {
        ForkWorkspaceRefreshKey(session: workspaceSession, foreground: foreground)
    }

    func syncAccountContext() {
        let session = workspaceSession
        guard session != appliedWorkspaceSession else { return }
        let previous = appliedWorkspaceSession
        appliedWorkspaceSession = session
        workspaceDirectoryUnavailable = false
        workspaceSelection = session.usesDirectory ? session.userId.map(ForkWorkspaceSelection.init(userId:)) : nil
        let storage = Self.initialStorage(for: account)
        guard storage.id != contextID || previous != nil else { return }
        replaceContext(storage, loadContext: !session.usesDirectory && !account.requiresLogin)
    }

    func refreshWorkspacesWhileActive() async {
        syncAccountContext()
        let session = workspaceSession
        guard session.usesDirectory, session.userId != nil else { return }
        while !Task.isCancelled, workspaceSession == session {
            do {
                let memberships = try await account.loadWorkspaceMemberships()
                guard !Task.isCancelled, workspaceSession == session, var next = workspaceSelection else { return }
                workspaceDirectoryUnavailable = false
                let purged = next.update(memberships)
                applyWorkspaceSelection(next, purge: purged)
            } catch {
                guard !Task.isCancelled, workspaceSession == session, var next = workspaceSelection else { return }
                workspaceDirectoryUnavailable = true
                next.suspend()
                applyWorkspaceSelection(next)
            }
            do { try await Task.sleep(for: .seconds(30)) } catch { return }
        }
    }

    func switchWorkspace(to workspace: ForkWorkspaceID) async {
        let session = workspaceSession
        let expected = workspaceSelection?.generation
        try? await account.transition.run(.workspace) {
            guard self.workspaceSession == session, !self.workspaceDirectoryUnavailable,
                  var next = self.workspaceSelection, next.generation == expected else { return }
            next.select(workspace)
            self.applyWorkspaceSelection(next)
            self.account.showSwitcher = false
        }
    }

    func handleWorkspaceIssue(_ issue: ForkWorkspaceIssue, from token: ForkWorkspaceSelection.Token) {
        guard workspaceSession == appliedWorkspaceSession, var next = workspaceSelection,
              let workspace = next.invalidate(issue, from: token) else { return }
        applyWorkspaceSelection(next, purge: [workspace])
    }

    private func applyWorkspaceSelection(_ next: ForkWorkspaceSelection, purge: Set<ForkWorkspaceID> = []) {
        let previous = workspaceSelection?.token
        workspaceSelection = next
        var removedScopes = Set(purge.map { $0.storageScopeID(userId: next.userId) })
        if !workspaceDirectoryUnavailable, next.memberships != nil {
            let allowed = Set(next.choices.map { $0.storageScopeID(userId: next.userId) })
            removedScopes.formUnion(SharedStore.replaceWorkspaceScopes(allowed, userID: next.userId))
        }
        let purged = removedScopes.map { SharedStore.Context(accountID: next.userId, scope: $0) }
        guard next.token != previous else {
            for context in purged { context.erase() }
            return
        }
        let storage = next.token.map { SharedStore.Context(accountID: $0.userId, scope: $0.storageScopeID) }
            ?? Self.initialStorage(for: account)
        replaceContext(storage, loadContext: false, erase: purged)
    }
}

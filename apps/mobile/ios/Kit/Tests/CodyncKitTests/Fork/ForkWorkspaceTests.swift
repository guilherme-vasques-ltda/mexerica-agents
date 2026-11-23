import Foundation
import Testing
@testable import CodyncKit

private let user = "user_fixture"
private let team = ForkWorkspaceID.team(orgId: "org_fixture")

@Test(arguments: [true, false]) func personalsAreNeverInferred(hasPersonal: Bool) throws {
    let member = try #require(ForkWorkspaceMemberships.Team(orgId: "org_fixture", name: "Fixture", clerkRole: "org:member"))
    var selection = ForkWorkspaceSelection(userId: user)
    selection.update(.init(hasPersonal: hasPersonal, teams: [member]))
    #expect(selection.choices == (hasPersonal ? [.personal, team] : [team]))
    #expect(selection.active == (hasPersonal ? .personal : team))
    selection.select(.personal)
    #expect(selection.active == (hasPersonal ? .personal : team))
}

@Test func personalOnlyAndUnknownRole() {
    var selection = ForkWorkspaceSelection(userId: user)
    #expect(ForkWorkspaceMemberships.Team(orgId: "org_fixture", name: "Fixture", clerkRole: "org:unknown") == nil)
    selection.update(.init(hasPersonal: true, teams: []))
    #expect(selection.choices == [.personal])
    #expect(selection.active == .personal)
}

@Test func temporaryDirectoryFailureRestoresOnlyAnAuthorizedSelection() throws {
    let member = try #require(ForkWorkspaceMemberships.Team(orgId: "org_fixture", name: "Fixture", clerkRole: "org:member"))
    let memberships = ForkWorkspaceMemberships(hasPersonal: true, teams: [member])
    var selection = ForkWorkspaceSelection(userId: user)
    selection.update(memberships)
    selection.select(team)
    let first = try #require(selection.token)
    selection.suspend()
    selection.suspend()
    #expect(selection.token == nil)
    selection.update(memberships)
    #expect(selection.active == team)
    #expect(!selection.accepts(first))
    selection.suspend()
    selection.update(.init(hasPersonal: true, teams: []))
    #expect(selection.active == .personal)
}

@Test func scopesSeparateUsersWorkspacesAndPersistedComputerSelection() {
    let suite = "Fork.\(UUID())"
    defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
    let scopes = [
        ForkWorkspaceID.personal.storageScopeID(userId: user),
        team.storageScopeID(userId: user),
        ForkWorkspaceID.team(orgId: "another_fixture").storageScopeID(userId: user),
        team.storageScopeID(userId: "another_user"),
    ]
    #expect(Set(scopes).count == scopes.count)
    let personal = SharedStore.Context(accountID: user, scope: scopes[0], suite: suite)
    let organization = SharedStore.Context(accountID: user, scope: scopes[1], suite: suite)
    personal.lastComputerId = "personal-computer"
    organization.lastComputerId = "team-computer"
    organization.composerDrafts = ["team-computer": ["fixture-bot": "draft"]]
    organization.erase()
    #expect(personal.lastComputerId == "personal-computer")
    #expect(organization.lastComputerId == nil)
    #expect(organization.composerDrafts.isEmpty)
}

@Test func oldGenerationCannotApplyEventsAfterSwitchOrRevocation() throws {
    let member = try #require(ForkWorkspaceMemberships.Team(orgId: "org_fixture", name: "Fixture", clerkRole: "org:member"))
    var selection = ForkWorkspaceSelection(userId: user)
    selection.update(.init(hasPersonal: true, teams: [member]))
    let first = try #require(selection.token)
    selection.select(team)
    let second = try #require(selection.token)
    #expect(!selection.accepts(first))
    #expect(selection.invalidate(.workspaceRevoked, from: first) == nil)
    #expect(selection.invalidate(.workspaceRevoked, from: second) == team)
    #expect(selection.active == .personal)
    #expect(!selection.accepts(first))
    #expect(!selection.accepts(second))
    #expect(selection.choices == [.personal])
    selection.update(.init(hasPersonal: false, teams: []))
    #expect(selection.token == nil)
}

@Test func membershipRemovalAndEpochChangeInvalidateWithoutGrantingAccess() throws {
    let member = try #require(ForkWorkspaceMemberships.Team(orgId: "org_fixture", name: "Fixture", clerkRole: "org:member"))
    let admin = try #require(ForkWorkspaceMemberships.Team(orgId: "org_fixture", name: "Fixture", clerkRole: "org:admin"))
    var selection = ForkWorkspaceSelection(userId: user)
    selection.update(.init(hasPersonal: false, teams: [admin]))
    let first = try #require(selection.token)
    #expect(selection.invalidate(.leaseExpired, from: first) == nil)
    #expect(selection.accepts(first))
    #expect(selection.update(.init(hasPersonal: false, teams: [member])) == [team])
    #expect(!selection.accepts(first))
    let roleChanged = try #require(selection.token)
    #expect(selection.invalidate(.workspaceEpochMismatch, from: roleChanged) == team)
    #expect(!selection.accepts(roleChanged))
    #expect(selection.update(.init(hasPersonal: false, teams: [])) == [team])
    #expect(selection.token == nil)
}

@Test func workspaceDTORejectsUnknownAuthorityAndSeparatesEpochAndSpace() throws {
    let json = #"{"schema":1,"hubId":"hub_fixture","kind":"team","workspaceId":"org:org_fixture","userId":"user_fixture","role":"member","workspaceEpoch":7,"capabilities":["chat"]}"#
    let decoder = JSONDecoder()
    let first = try decoder.decode(ForkWorkspaceContext.self, from: Data(json.utf8))
    let next = try decoder.decode(ForkWorkspaceContext.self, from: Data(json.replacingOccurrences(of: ":7", with: ":8").utf8))
    #expect(first.cacheScopeID(spaceId: "root") != next.cacheScopeID(spaceId: "root"))
    #expect(first.cacheScopeID(spaceId: "root") != first.cacheScopeID(spaceId: "private"))
    #expect(throws: DecodingError.self) {
        try decoder.decode(ForkWorkspaceContext.self, from: Data(json.replacingOccurrences(of: "member", with: "unknown").utf8))
    }
}

@Test func stableRejectOverridesLegacyAndExpiredLeaseIsNotTerminal() throws {
    let terminal = try #require(ForkWorkspaceFailure.channelReject(
        Data(#"{"t":"reject","code":"revoked","stableCode":"role_changed"}"#.utf8), closeCode: 4003))
    let lease = try #require(ForkWorkspaceFailure.channelReject(
        Data(#"{"t":"reject","code":"unauthorized","stableCode":"lease_expired"}"#.utf8), closeCode: 4001))
    let oldLease = try ForkWorkspaceFailure.channelReject(Data(#"{"t":"reject","code":"leaseExpired"}"#.utf8), closeCode: 4001)
    let oldRevocation = try ForkWorkspaceFailure.channelReject(Data(#"{"t":"reject","code":"revoked"}"#.utf8), closeCode: 4003)
    #expect(terminal.issue == .roleChanged && terminal.closesChannel)
    #expect(lease.issue == .leaseExpired && lease.closesChannel)
    #expect(oldLease == lease)
    #expect(oldRevocation?.issue == .deviceRevoked)
}

@Test func epochMismatchInvalidatesWithoutClosingTheChannel() throws {
    let reply = Data(#"{"id":1,"err":{"status":409,"code":"workspace_epoch_mismatch"}}"#.utf8)
    let failure = try #require(ForkWorkspaceFailure.channelReply(reply))
    #expect(failure.issue == .workspaceEpochMismatch && !failure.closesChannel)
    #expect(ForkWorkspaceFailure.http(status: 409, code: "workspace_epoch_mismatch") == failure)
    #expect(ForkWorkspaceFailure.http(status: 403, code: "workspace_epoch_mismatch") == nil)
    #expect(ForkWorkspaceFailure.http(status: 503, code: "hub_offline") == nil)
}

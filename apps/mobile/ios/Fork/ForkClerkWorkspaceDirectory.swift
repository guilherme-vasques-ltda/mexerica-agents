import ClerkKit
import CodyncKit
import Foundation

@MainActor
struct ForkClerkWorkspaceDirectory {
    struct Configuration: Decodable {
        let workspaceDirectoryEnabled: Bool
    }

    func load(user: User, organizationsEnabled: Bool) async throws -> ForkWorkspaceMemberships {
        let reloaded = try await user.reload()
        guard reloaded.id == user.id else { throw DirectoryError.inconsistentMemberships }
        let pageSize = 20
        var memberships: [OrganizationMembership] = []
        var ids = Set<String>()
        var expectedCount: Int?
        if organizationsEnabled {
            for page in 1...100 {
                try Task.checkCancellation()
                let reply = try await reloaded.getOrganizationMemberships(page: page, pageSize: pageSize)
                guard reply.totalCount >= 0, expectedCount == nil || expectedCount == reply.totalCount,
                      reply.data.allSatisfy({ ids.insert($0.organization.id).inserted }) else {
                    throw DirectoryError.inconsistentMemberships
                }
                expectedCount = reply.totalCount
                memberships += reply.data
                if memberships.count == reply.totalCount { break }
                guard memberships.count < reply.totalCount, reply.data.count == pageSize else {
                    throw DirectoryError.inconsistentMemberships
                }
            }
            guard memberships.count == expectedCount else { throw DirectoryError.inconsistentMemberships }
        }
        let hasPersonal: Bool
        if case .object(let metadata)? = reloaded.publicMetadata,
           case .bool(true)? = metadata["hasPersonal"] {
            hasPersonal = true
        } else {
            hasPersonal = false
        }
        return ForkWorkspaceMemberships(hasPersonal: hasPersonal, teams: memberships.compactMap {
            .init(orgId: $0.organization.id, name: $0.organization.name, clerkRole: $0.role)
        })
    }

    private enum DirectoryError: Error { case inconsistentMemberships }
}

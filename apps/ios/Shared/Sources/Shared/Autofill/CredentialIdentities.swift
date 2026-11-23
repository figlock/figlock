import AuthenticationServices
import CryptoKit
import Foundation

/// One entry of the system's QuickType suggestions: a domain and a username, never a secret. The record
/// identifier is the item id, which is how the extension finds the login when the person picks it.
public struct CredentialIdentity: Equatable, Sendable {
    public let domain: String
    public let username: String
    public let recordIdentifier: String

    public init(domain: String, username: String, recordIdentifier: String) {
        self.domain = domain
        self.username = username
        self.recordIdentifier = recordIdentifier
    }

    /// One identity per website of each login that has a username; deleted items and logins without a username
    /// are left out (there is nothing to suggest).
    public static func identities(for items: [VaultItem]) -> [CredentialIdentity] {
        var result: [CredentialIdentity] = []
        for item in items where item.kind == .login && !item.isDeleted {
            guard let username = item.fields["username"], !username.isEmpty else { continue }
            var seen = Set<String>()
            for site in item.websites {
                guard let domain = ServiceMatch.host(of: site), seen.insert(domain).inserted else { continue }
                result.append(CredentialIdentity(domain: domain, username: username, recordIdentifier: item.id))
            }
        }
        return result
    }
}

/// The system store behind QuickType, as a protocol so tests do not need the system.
public protocol CredentialIdentityStoring: Sendable {
    /// True when the system store now holds exactly these identities.
    func replace(with identities: [CredentialIdentity]) async -> Bool
    func removeAll() async
}

public struct SystemCredentialIdentityStore: CredentialIdentityStoring {
    public init() {}

    public func replace(with identities: [CredentialIdentity]) async -> Bool {
        let store = ASCredentialIdentityStore.shared
        let state = await store.state()
        guard state.isEnabled else { return false }
        let system: [ASPasswordCredentialIdentity] = identities.map {
            ASPasswordCredentialIdentity(
                serviceIdentifier: ASCredentialServiceIdentifier(identifier: $0.domain, type: .domain),
                user: $0.username, recordIdentifier: $0.recordIdentifier)
        }
        do {
            try await store.replaceCredentialIdentities(system)
            return true
        } catch {
            return false
        }
    }

    public func removeAll() async {
        try? await ASCredentialIdentityStore.shared.removeAllCredentialIdentities()
    }
}

/// Keeps the system store in step with the vault (ADR-0041 decision 7). It runs in the app after the list is
/// loaded, skips the write when nothing changed, and `clear` is called when the vault leaves this phone.
@MainActor
public final class CredentialIdentitySync {
    private let store: any CredentialIdentityStoring
    private var lastDigest: Data?

    public init(store: any CredentialIdentityStoring = SystemCredentialIdentityStore()) {
        self.store = store
    }

    public func update(from items: [VaultItem]) {
        let identities = CredentialIdentity.identities(for: items)
        var hasher = SHA256()
        for identity in identities.sorted(by: { $0.recordIdentifier + $0.domain < $1.recordIdentifier + $1.domain }) {
            for part in [identity.recordIdentifier, identity.domain, identity.username] {
                hasher.update(data: Data(part.utf8))
                hasher.update(data: Data([0]))
            }
        }
        let digest = Data(hasher.finalize())
        guard digest != lastDigest else { return }
        let store = self.store
        Task {
            if await store.replace(with: identities) { remember(digest) }
        }
    }

    private func remember(_ digest: Data) {
        lastDigest = digest
    }

    public func clear() {
        lastDigest = nil
        let store = self.store
        Task { await store.removeAll() }
    }
}

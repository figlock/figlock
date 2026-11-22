import FigLockFFI
import Foundation

/// One row of the device list.
public struct DeviceRow: Identifiable, Equatable, Sendable {
    public let id: String
    /// What the vault knows the device as; nil when it never had a name (an old revocation, ADR-0036).
    public let vaultName: String?
    /// The name given on this iPhone, if any.
    public let nickname: String?
    public let fingerprint: String
    public let revokedAt: Date?
    public let isThisDevice: Bool

    /// What the list shows.
    public var displayName: String { nickname ?? vaultName ?? "Unnamed device" }
    public var isRevoked: Bool { revokedAt != nil }
}

public enum DeviceList {
    /// Live devices first (this iPhone at the top), then revoked ones, newest revocation first.
    public static func rows(entries: [DeviceEntry], nicknames: [String: String]) -> [DeviceRow] {
        let rows = entries.map { entry in
            DeviceRow(
                id: entry.id,
                vaultName: entry.name,
                nickname: nicknames[entry.id],
                fingerprint: entry.fingerprint,
                revokedAt: entry.revokedMs.map { Date(timeIntervalSince1970: TimeInterval($0) / 1000) },
                isThisDevice: entry.isThisDevice)
        }
        let live = rows.filter { !$0.isRevoked }.sorted {
            if $0.isThisDevice != $1.isThisDevice { return $0.isThisDevice }
            return $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending
        }
        let gone = rows.filter(\.isRevoked).sorted { ($0.revokedAt ?? .distantPast) > ($1.revokedAt ?? .distantPast) }
        return live + gone
    }
}

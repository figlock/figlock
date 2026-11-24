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
    /// The live device whose record says it replaces this one: the same phone added again after it was removed
    /// (ADR-0044). The list keeps such an entry out of the main devices and shows it as an earlier one.
    public let replacedBy: String?

    /// What the list shows.
    public var displayName: String { nickname ?? vaultName ?? "Unnamed device" }
    public var isRevoked: Bool { revokedAt != nil }
    /// A live entry that a later entry of the same device took over; it can still be revoked by hand.
    public var isEarlierEntry: Bool { replacedBy != nil && !isRevoked }
}

public enum DeviceList {
    /// Live devices first (this iPhone at the top), then earlier entries of devices that were added again, then
    /// revoked ones, newest revocation first.
    public static func rows(entries: [DeviceEntry], nicknames: [String: String]) -> [DeviceRow] {
        let rows = entries.map { entry in
            DeviceRow(
                id: entry.id,
                vaultName: entry.name,
                nickname: nicknames[entry.id],
                fingerprint: entry.fingerprint,
                revokedAt: entry.revokedMs.map { Date(timeIntervalSince1970: TimeInterval($0) / 1000) },
                isThisDevice: entry.isThisDevice,
                replacedBy: entry.replacedBy)
        }
        let live = rows.filter { !$0.isRevoked && !$0.isEarlierEntry }.sorted {
            if $0.isThisDevice != $1.isThisDevice { return $0.isThisDevice }
            return $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending
        }
        let earlier = rows.filter(\.isEarlierEntry).sorted {
            $0.displayName.localizedCaseInsensitiveCompare($1.displayName) == .orderedAscending
        }
        let gone = rows.filter(\.isRevoked).sorted { ($0.revokedAt ?? .distantPast) > ($1.revokedAt ?? .distantPast) }
        return live + earlier + gone
    }
}

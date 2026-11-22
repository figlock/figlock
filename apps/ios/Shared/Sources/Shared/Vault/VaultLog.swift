import FigLockFFI
import Foundation

/// Reads the op files of the local vault into an unlocked session (`sync.md` 2: `log/<device>/<seq>.op`).
/// Files are applied in order per device and only once; a file that fails to apply is counted, never skipped
/// silently, and later files of that device are not applied past the gap.
public struct VaultLog: Sendable {
    public struct Report: Equatable, Sendable {
        public var applied = 0
        /// Files that could not be applied, by path.
        public var failed: [String] = []
    }

    private let store: any BlobStoreFfi

    public init(store: any BlobStoreFfi) {
        self.store = store
    }

    /// Apply every file not yet applied. `applied` maps a device id to the highest sequence number already in the
    /// session, and is updated.
    public func apply(
        to session: VaultSession, applied: inout [String: UInt64], now: () -> Date = { Date() }
    ) throws -> Report {
        var report = Report()
        var byDevice: [String: [(seq: UInt64, path: String)]] = [:]
        for entry in try store.list(prefix: "log/") where entry.available {
            guard let (device, seq) = Self.parse(entry.path) else { continue }
            byDevice[device, default: []].append((seq, entry.path))
        }
        for (device, files) in byDevice {
            for file in files.sorted(by: { $0.seq < $1.seq }) where file.seq > (applied[device] ?? 0) {
                do {
                    let bytes = try store.get(path: file.path)
                    // The clock is read for each file: the vault's hybrid clock counts ops that share one instant,
                    // so a large vault loaded at one frozen instant would overflow it.
                    let nowMs = UInt64(now().timeIntervalSince1970 * 1000)
                    try session.applyFile(bytes: bytes, deviceId: device, seq: file.seq, nowMs: nowMs)
                    applied[device] = file.seq
                    report.applied += 1
                } catch {
                    report.failed.append(file.path)
                    break
                }
            }
        }
        return report
    }

    /// `log/<32 hex device id>/<8 digit seq>.op` to the device id and the sequence number.
    static func parse(_ path: String) -> (device: String, seq: UInt64)? {
        let parts = path.split(separator: "/", omittingEmptySubsequences: false)
        guard parts.count == 3, parts[0] == "log", parts[1].count == 32, parts[2].hasSuffix(".op"),
            let seq = UInt64(parts[2].dropLast(3))
        else { return nil }
        return (String(parts[1]), seq)
    }
}

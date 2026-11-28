import FigLockFFI
import Foundation
import Observation

/// Where breach lists come from: for a 5 character prefix, the `SUFFIX:COUNT` lines of every known breached password
/// hash that starts with it. The real one is Have I Been Pwned's range service; tests substitute their own.
public protocol BreachRangeFetching: Sendable {
    func range(prefix: String) async throws -> String
}

/// The Have I Been Pwned range service (k-anonymity). Only the 5 character prefix of a SHA-1 is ever put in the URL; no
/// cookies, no redirects, a size cap and `Add-Padding` so every answer is of similar size.
public struct PwnedRangeService: BreachRangeFetching {
    public struct Unreachable: Error {
        public init() {}
    }

    static let base = "https://api.pwnedpasswords.com/range/"
    /// A range answer is about 40 KB; this only stops a hostile or broken server filling memory.
    static let maxBytes = 4 * 1024 * 1024

    private let session: URLSession

    public init() {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 15
        config.httpCookieStorage = nil
        config.urlCache = nil
        session = URLSession(configuration: config, delegate: NoRedirects(), delegateQueue: nil)
    }

    public func range(prefix: String) async throws -> String {
        guard prefix.count == 5, prefix.allSatisfy({ $0.isASCII && $0.isHexDigit }),
            let url = URL(string: Self.base + prefix)
        else { throw Unreachable() }
        var request = URLRequest(url: url)
        request.setValue("true", forHTTPHeaderField: "Add-Padding")
        let (data, response) = try await session.data(for: request)
        guard (response as? HTTPURLResponse)?.statusCode == 200, data.count <= Self.maxBytes,
            let text = String(data: data, encoding: .utf8)
        else { throw Unreachable() }
        return text
    }

    private final class NoRedirects: NSObject, URLSessionTaskDelegate {
        func urlSession(
            _ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
            newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void
        ) {
            completionHandler(nil)
        }
    }
}

/// One item with something wrong with its password. Numbers and an id, never the password.
public struct HealthRow: Identifiable, Equatable, Sendable {
    public let id: String
    /// Guessability in bits, when the password is weak.
    public let weakBits: UInt32?
    /// How many other items use the same password.
    public let reusedWith: Int
    /// Days since the item last changed, when that is a year or more.
    public let oldDays: UInt64?
    /// Times the password appears in known breaches, when the breach check ran and found it.
    public var breached: UInt64?

    public var issueCount: Int {
        (weakBits != nil ? 1 : 0) + (reusedWith > 0 ? 1 : 0) + (oldDays != nil ? 1 : 0) + (breached != nil ? 1 : 0)
    }
}

/// The password health screen's state (FIG-292): weak, reused and old passwords from the Rust analysis, and the opt-in
/// breach check. The breach check sends only 5 character hash prefixes (see `PwnedRangeService`); a failure of the
/// service never hides the local results, and a half-checked vault is never shown as clean.
@MainActor
@Observable
public final class PasswordHealthModel {
    public enum Breaches: Equatable, Sendable {
        case notChecked
        case checking
        case checked
        /// The service could not be reached, so breaches were not checked.
        case unreachable
    }

    public private(set) var loaded = false
    /// Passwords looked at.
    public private(set) var checked = 0
    public private(set) var rows: [HealthRow] = []
    public private(set) var breaches: Breaches = .notChecked

    private var local: [HealthRow] = []
    private let session: VaultSession
    private let service: any BreachRangeFetching
    private let now: @Sendable () -> Date

    public init(
        session: VaultSession, service: any BreachRangeFetching = PwnedRangeService(),
        now: @escaping @Sendable () -> Date = { Date() }
    ) {
        self.session = session
        self.service = service
        self.now = now
    }

    /// The local analysis. Needs no network, so it runs as the screen opens.
    public func load() async {
        let session = self.session
        let nowMs = UInt64(now().timeIntervalSince1970 * 1000)
        let summary = await Task.detached(priority: .userInitiated) { try? session.healthReport(nowMs: nowMs) }.value
        guard let summary else {
            loaded = true
            return
        }
        checked = Int(summary.checked)
        local = summary.findings.map {
            HealthRow(id: $0.id, weakBits: $0.weakBits, reusedWith: Int($0.reusedWith), oldDays: $0.oldDays)
        }
        rows = local
        // The breach result belongs to the vault as it was; a reload starts it over.
        breaches = .notChecked
        loaded = true
    }

    /// Asks the range service about every password's hash prefix. Any failed request fails the whole check.
    public func checkBreaches() async {
        guard breaches != .checking else { return }
        breaches = .checking
        let session = self.session
        let service = self.service
        let check = await Task.detached(priority: .userInitiated) { try? session.startBreachCheck() }.value
        guard let check else {
            breaches = .unreachable
            return
        }
        do {
            let answers = try await Self.fetch(prefixes: check.prefixes(), from: service)
            let hits = try check.finish(answers: answers)
            rows = Self.merge(local: local, hits: hits)
            breaches = .checked
        } catch {
            rows = local
            breaches = .unreachable
        }
    }

    /// The local findings with breach counts added, then passwords that are only breached, most breached first.
    nonisolated static func merge(local: [HealthRow], hits: [BreachHit]) -> [HealthRow] {
        let counts = Dictionary(hits.map { ($0.id, $0.count) }, uniquingKeysWith: max)
        var out = local.map { row -> HealthRow in
            var row = row
            row.breached = counts[row.id]
            return row
        }
        let known = Set(local.map(\.id))
        out += hits.filter { !known.contains($0.id) }.sorted { ($0.count, $1.id) > ($1.count, $0.id) }
            .map { HealthRow(id: $0.id, weakBits: nil, reusedWith: 0, oldDays: nil, breached: $0.count) }
        return out
    }

    /// At most a few requests at a time, so a big vault is neither slow nor a burst.
    private static func fetch(prefixes: [String], from service: any BreachRangeFetching) async throws -> [RangeAnswer] {
        var answers: [RangeAnswer] = []
        var next = 0
        try await withThrowingTaskGroup(of: RangeAnswer.self) { group in
            func start() {
                guard next < prefixes.count else { return }
                let prefix = prefixes[next]
                next += 1
                group.addTask { RangeAnswer(prefix: prefix, body: try await service.range(prefix: prefix)) }
            }
            for _ in 0..<6 { start() }
            while let answer = try await group.next() {
                answers.append(answer)
                start()
            }
        }
        return answers
    }
}

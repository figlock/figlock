import CryptoKit
import FigLockFFI
import Foundation
import Testing

@testable import Shared

/// A stand-in for the range service that records what it was asked and answers from a list of "breached" passwords.
private final class FakeRangeService: BreachRangeFetching, @unchecked Sendable {
    private let lock = NSLock()
    private var asked: [String] = []
    private let breached: [String: Int]
    private let fails: Bool

    init(breached: [String: Int] = [:], fails: Bool = false) {
        self.breached = breached
        self.fails = fails
    }

    var prefixes: [String] { lock.withLock { asked } }

    func range(prefix: String) async throws -> String {
        lock.withLock { asked.append(prefix) }
        if fails { throw PwnedRangeService.Unreachable() }
        var lines = ["00000000000000000000000000000000000:0"]
        for (password, count) in breached {
            let hash = Insecure.SHA1.hash(data: Data(password.utf8)).map { String(format: "%02X", $0) }.joined()
            if hash.hasPrefix(prefix) { lines.append("\(hash.dropFirst(5)):\(count)") }
        }
        return lines.joined(separator: "\r\n")
    }
}

private let weakPassword = "summer2020"
private let strongA = "tr0ub4dor&3-horse-staple-battery-9x"
private let strongB = "kq7-Zm2!vXw9-Lp4#Rt8-Yn5$Hc3"
private let seededAt = Date(timeIntervalSince1970: 1_800_000_000)

private func login(_ title: String, _ password: String) -> (String, [(String, String)]) {
    ("login", [("title", title), ("password", password)])
}

@MainActor
private func model(_ seeded: SeededVault, service: FakeRangeService, years: Double = 0) -> PasswordHealthModel {
    PasswordHealthModel(
        session: seeded.session, service: service, now: { seededAt.addingTimeInterval(years * 365 * 86_400 + 60) })
}

@MainActor
@Test func localReportFindsWeakReusedAndOldWithoutTheNetwork() async throws {
    let seeded = try SeededVault.make([
        login("Weak", weakPassword), login("One", strongA), login("Two", strongA), login("Fine", strongB),
    ])
    defer { seeded.cleanup() }
    let service = FakeRangeService()
    _ = await seeded.browser()
    let health = model(seeded, service: service)
    await health.load()

    #expect(health.loaded)
    #expect(health.checked == 4)
    let byId = Dictionary(uniqueKeysWithValues: health.rows.map { ($0.id, $0) })
    #expect(byId[seeded.itemIds[0]]?.weakBits != nil)
    #expect(byId[seeded.itemIds[1]]?.reusedWith == 1)
    #expect(byId[seeded.itemIds[2]]?.reusedWith == 1)
    #expect(byId[seeded.itemIds[3]] == nil)
    #expect(health.breaches == .notChecked)
    #expect(service.prefixes.isEmpty, "the local check must not touch the network")
}

@MainActor
@Test func passwordsUnchangedForAYearAreOld() async throws {
    let seeded = try SeededVault.make([login("Stale", strongA)])
    defer { seeded.cleanup() }
    _ = await seeded.browser()
    let health = model(seeded, service: FakeRangeService(), years: 1.5)
    await health.load()
    #expect(health.rows.first?.oldDays ?? 0 >= 365)
}

@MainActor
@Test func breachCheckSendsOnlyPrefixesAndMergesTheHits() async throws {
    let seeded = try SeededVault.make([login("Weak", weakPassword), login("Pwned", strongB), login("Fine", strongA)])
    defer { seeded.cleanup() }
    let service = FakeRangeService(breached: [weakPassword: 12, strongB: 3])
    _ = await seeded.browser()
    let health = model(seeded, service: service)
    await health.load()
    await health.checkBreaches()

    #expect(health.breaches == .checked)
    #expect(service.prefixes.count == 3)
    #expect(service.prefixes.allSatisfy { $0.count == 5 })
    let byId = Dictionary(uniqueKeysWithValues: health.rows.map { ($0.id, $0) })
    #expect(byId[seeded.itemIds[0]]?.breached == 12)
    #expect(byId[seeded.itemIds[0]]?.weakBits != nil)
    // Breached but otherwise fine: it is listed too.
    #expect(byId[seeded.itemIds[1]]?.breached == 3)
    #expect(byId[seeded.itemIds[2]] == nil)
}

@MainActor
@Test func anUnreachableServiceKeepsTheLocalResultsAndSaysSo() async throws {
    let seeded = try SeededVault.make([login("Weak", weakPassword), login("Fine", strongA)])
    defer { seeded.cleanup() }
    _ = await seeded.browser()
    let health = model(seeded, service: FakeRangeService(fails: true))
    await health.load()
    await health.checkBreaches()

    #expect(health.breaches == .unreachable)
    #expect(health.rows.map(\.id) == [seeded.itemIds[0]])
    #expect(health.rows.allSatisfy { $0.breached == nil })

    // Try again when online: the same screen recovers.
    await health.load()
    #expect(health.breaches == .notChecked)
}

@Test func mergeOrdersBreachedOnlyItemsMostBreachedFirst() {
    let local = [HealthRow(id: "a", weakBits: 20, reusedWith: 0, oldDays: nil)]
    let rows = PasswordHealthModel.merge(
        local: local,
        hits: [BreachHit(id: "z", count: 5), BreachHit(id: "b", count: 90), BreachHit(id: "a", count: 2)])
    #expect(rows.map(\.id) == ["a", "b", "z"])
    #expect(rows[0].breached == 2)
}

@Test func theRangeServiceRefusesAnythingButAFiveCharacterHexPrefix() async {
    let service = PwnedRangeService()
    for bad in ["", "ABCD", "ABCDEF", "ZZZZZ", "AB/CD", "../..."] {
        await #expect(throws: PwnedRangeService.Unreachable.self) { _ = try await service.range(prefix: bad) }
    }
}

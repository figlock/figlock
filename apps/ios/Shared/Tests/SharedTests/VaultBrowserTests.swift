import FigLockFFI
import Foundation
import Testing

@testable import Shared

private func login(_ title: String, user: String, url: String, password: String = "generated-pass-1") -> (
    String, [(String, String)]
) {
    ("login", [("title", title), ("username", user), ("url/a1", url), ("password", password)])
}

@MainActor
@Test func theListShowsEveryItemNewestFirstWithoutAnySecret() async throws {
    let seeded = try SeededVault.make([
        login("Fastmail", user: "mira@example.com", url: "https://app.fastmail.com"),
        ("token", [("title", "GitHub token"), ("provider", "github"), ("token", "fl_demo_gh_generated")]),
        ("note", [("title", "Wifi"), ("body", "generated note body")]),
    ])
    defer { seeded.cleanup() }
    let browser = await seeded.browser()

    #expect(browser.items.map(\.title) == ["Wifi", "GitHub token", "Fastmail"])
    #expect(browser.items.first { $0.title == "Fastmail" }?.subtitle == "mira@example.com")
    #expect(browser.items.first { $0.title == "GitHub token" }?.secretFields == ["token"])
    // No secret value is reachable from the item model.
    let dump = browser.items.map { "\($0.fields)" }.joined()
    #expect(!dump.contains("generated-pass-1"))
    #expect(!dump.contains("fl_demo_gh_generated"))
}

@MainActor
@Test func searchFindsByTitleDomainAndTypoAndFiltersByType() async throws {
    let seeded = try SeededVault.make([
        login("Fastmail", user: "mira@example.com", url: "https://app.fastmail.com"),
        login("Linear", user: "mira@example.com", url: "https://linear.app"),
        ("token", [("title", "Stripe API key"), ("provider", "stripe"), ("token", "fl_demo_st_generated")]),
    ])
    defer { seeded.cleanup() }
    let browser = await seeded.browser()

    browser.query = "fastmial"
    #expect(browser.visible.first?.title == "Fastmail")
    browser.query = "linear.app"
    #expect(browser.visible.first?.title == "Linear")
    browser.query = "stripe"
    #expect(browser.visible.first?.title == "Stripe API key")
    // A secret value is never searchable.
    browser.query = "fl_demo_st_generated"
    #expect(browser.visible.isEmpty)

    browser.query = ""
    browser.filter = .token
    #expect(browser.visible.map(\.title) == ["Stripe API key"])
    browser.filter = .login
    #expect(browser.visible.count == 2)
    browser.filter = .other
    #expect(browser.visible.isEmpty)
}

@MainActor
@Test func theSameQueryGivesTheSameHitsAfterAFilterChange() async throws {
    let seeded = try SeededVault.make([
        login("Fastmail", user: "mira@example.com", url: "https://app.fastmail.com"),
        ("token", [("title", "Fastmail token"), ("provider", "fastmail"), ("token", "fl_demo_fm_generated")]),
    ])
    defer { seeded.cleanup() }
    let browser = await seeded.browser()

    browser.query = "fastmail"
    let all = browser.visible.map(\.id)
    #expect(Set(all).count == 2)
    browser.filter = .token
    #expect(browser.visible.count == 1)
    browser.filter = .all
    #expect(browser.visible.map(\.id) == all)
    await browser.reload()
    #expect(browser.visible.map(\.id) == all)
}

@MainActor
@Test func reloadingReadsOnlyNewOpFilesAndCountsDamagedOnes() async throws {
    let seeded = try SeededVault.make([login("Fastmail", user: "a@example.com", url: "https://fastmail.com")])
    defer { seeded.cleanup() }
    let browser = await seeded.browser()
    #expect(browser.items.count == 1)
    #expect(browser.loadFailures.isEmpty)

    // A file that is not an op file stops that device's log at the gap and is reported, not skipped.
    let store = try seeded.vault.openStore()
    let device = try #require(try store.list(prefix: "log/").first?.path.split(separator: "/")[1])
    try store.putNew(path: "log/\(device)/00000002.op", bytes: Data("not an op file".utf8))
    await browser.reload()
    #expect(browser.items.count == 1)
    #expect(browser.loadFailures == ["log/\(device)/00000002.op"])
}

@Test func opPathsParseStrictly() {
    let device = String(repeating: "a", count: 32)
    #expect(VaultLog.parse("log/\(device)/00000001.op")?.seq == 1)
    #expect(VaultLog.parse("log/\(device)/00000001.tmp") == nil)
    #expect(VaultLog.parse("log/short/00000001.op") == nil)
    #expect(VaultLog.parse("snap/\(device)/00000001.op") == nil)
    #expect(VaultLog.parse("log/\(device)/x.op") == nil)
}

@MainActor
@Test func listAndFuzzySearchAreUnder100msAtTenThousandItems() async throws {
    var items: [(String, [(String, String)])] = []
    for index in 0..<10_000 {
        items.append(
            login(
                "Service \(index) \(index % 7 == 0 ? "Fastmail" : "Example")", user: "user\(index)@example.com",
                url: "https://site\(index).example.com", password: "generated-pass-\(index)"))
    }
    let seeded = try SeededVault.make(items)
    defer { seeded.cleanup() }

    let clock = ContinuousClock()
    let started = clock.now
    let ready = await seeded.browser()
    let load = clock.now - started
    print("FIG-162 loaded \(ready.items.count) failures \(ready.loadFailures)")
    #expect(ready.items.count == 10_000)

    // The index is built while loading, so the first search is as fast as the next.
    var firstHits: [VaultItem] = []
    ready.query = "fastmial"
    let first = clock.measure { firstHits = ready.visible }
    #expect(firstHits.first?.title.contains("Fastmail") == true)
    ready.query = "site9999"
    var hits: [VaultItem] = []
    let warm = clock.measure { hits = ready.visible }
    #expect(hits.first?.title.hasPrefix("Service 9999") == true)
    ready.query = ""
    let list = clock.measure { _ = ready.visible }

    print(
        "FIG-162 timings at 10000 items: load \(load) (off the main thread), first search \(first), warm search \(warm), list \(list)"
    )
    #expect(list < .milliseconds(100))
    #expect(warm < .milliseconds(100))
    #expect(first < .milliseconds(100))
}

import Foundation
import Testing

@testable import Shared

/// The search box of the AutoFill list. Every value is generated for these tests.

private func searchLogin(_ title: String, user: String, url: String, password: String) -> (
    String, [(String, String)]
) {
    ("login", [("title", title), ("username", user), ("url/a1", url), ("password", password)])
}

@Test func theAutofillListSearchFindsALoginByItsWebsiteName() async throws {
    let setup = try AutofillPhone.make(logins: [
        searchLogin("Work", user: "ada", url: "https://www.GitHub.com/login", password: "generated-pass-1"),
        searchLogin("Other", user: "bob", url: "https://other.org", password: "generated-pass-2"),
    ])
    defer { setup.phone.cleanup() }

    let vault = try await setup.access.open(reason: "Fill")
    defer { vault.close() }
    for query in ["github", "GitHub.com", "github.com", "https://github.com", "www.github.com"] {
        #expect(vault.search(query).map(\.title) == ["Work"], "query: \(query)")
    }
    #expect(vault.search("gitlab").isEmpty)
}

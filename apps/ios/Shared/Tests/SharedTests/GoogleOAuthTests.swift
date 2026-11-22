import FigLockFFI
import Foundation
import Testing

@testable import Shared

/// A transport that answers from a script and records what was posted.
private final class ScriptedTransport: FormTransport, @unchecked Sendable {
    private let lock = NSLock()
    private var replies: [(Int, String)]
    private(set) var posts: [[String: String]] = []

    init(_ replies: [(Int, String)]) { self.replies = replies }

    func post(url: URL, form: [String: String]) throws -> (status: Int, body: Data) {
        lock.lock()
        defer { lock.unlock() }
        posts.append(form)
        let reply = replies.isEmpty ? (500, "") : replies.removeFirst()
        return (reply.0, Data(reply.1.utf8))
    }
}

private let config = GoogleOAuthConfig(clientId: "1234-generated.apps.googleusercontent.com")
private let scope = GoogleOAuthConfig.scope

@Test func pkceChallengeMatchesTheRFC7636Vector() {
    #expect(
        PKCE.challenge(for: "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk")
            == "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM")
    let verifier = PKCE.makeVerifier()
    #expect(verifier.count >= 43 && verifier.count <= 128)
    #expect(verifier.allSatisfy { $0.isLetter || $0.isNumber || $0 == "-" || $0 == "_" })
    #expect(PKCE.makeVerifier() != verifier)
}

@Test func theAuthorizationURLAsksForTheOneScopeWithPKCEAndTheIOSRedirect() throws {
    let url = try #require(GoogleOAuth(config: config).authorizationURL(state: "st", challenge: "ch"))
    let items = Dictionary(
        uniqueKeysWithValues: (URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems ?? []).map {
            ($0.name, $0.value ?? "")
        })
    #expect(items["scope"] == scope)
    #expect(items["code_challenge_method"] == "S256")
    #expect(items["code_challenge"] == "ch")
    #expect(items["response_type"] == "code")
    #expect(items["redirect_uri"] == "com.googleusercontent.apps.1234-generated:/oauth2redirect")
    #expect(config.redirectScheme == "com.googleusercontent.apps.1234-generated")
}

@Test func aRedirectWithAnotherStateIsIgnored() throws {
    let oauth = GoogleOAuth(config: config)
    let good = try #require(URL(string: "com.googleusercontent.apps.1234-generated:/oauth2redirect?code=abc&state=st"))
    let bad = try #require(
        URL(string: "com.googleusercontent.apps.1234-generated:/oauth2redirect?code=abc&state=other"))
    #expect(oauth.code(from: good, expectedState: "st") == "abc")
    #expect(oauth.code(from: bad, expectedState: "st") == nil)
}

@Test func exchangingACodeGivesTheGrantAndRefusesABroaderScope() throws {
    let granted = ScriptedTransport([
        (200, "{\"access_token\":\"a1\",\"refresh_token\":\"r1\",\"expires_in\":3599,\"scope\":\"\(scope)\"}")
    ])
    let grant = try GoogleOAuth(config: config, transport: granted).exchange(code: "c", verifier: "v")
    #expect(grant == GoogleOAuth.Grant(accessToken: "a1", refreshToken: "r1", lifetime: 3599))
    #expect(granted.posts.first?["code_verifier"] == "v")
    #expect(granted.posts.first?["grant_type"] == "authorization_code")

    let broad = ScriptedTransport([
        (200, "{\"access_token\":\"a\",\"refresh_token\":\"r\",\"scope\":\"\(scope) https://mail.google.com/\"}")
    ])
    #expect(throws: SignInError.refused) {
        try GoogleOAuth(config: config, transport: broad).exchange(code: "c", verifier: "v")
    }
    let denied = ScriptedTransport([(400, "{\"error\":\"invalid_grant\"}")])
    #expect(throws: SignInError.refused) {
        try GoogleOAuth(config: config, transport: denied).exchange(code: "c", verifier: "v")
    }
    let down = ScriptedTransport([(503, "")])
    #expect(throws: SignInError.unavailable) {
        try GoogleOAuth(config: config, transport: down).exchange(code: "c", verifier: "v")
    }
}

@Test func accessTokensRefreshOnceAndAreRefreshedAgainAfterInvalidate() throws {
    let secrets = MemorySecrets()
    let clock = TestClock()
    let transport = ScriptedTransport([
        (200, "{\"access_token\":\"a2\",\"expires_in\":3600}"),
        (200, "{\"access_token\":\"a3\",\"expires_in\":3600}"),
    ])
    let tokens = GoogleAccessTokens(
        oauth: GoogleOAuth(config: config, transport: transport), store: secrets, now: { clock.date })
    #expect(!tokens.isSignedIn)

    try tokens.adopt(GoogleOAuth.Grant(accessToken: "a1", refreshToken: "r1", lifetime: 3600))
    #expect(tokens.isSignedIn)
    #expect(try tokens.accessToken() == "a1")
    #expect(transport.posts.isEmpty)

    clock.advance(3600)
    #expect(try tokens.accessToken() == "a2")
    #expect(transport.posts.first?["refresh_token"] == "r1")
    #expect(try tokens.accessToken() == "a2")
    tokens.invalidate()
    #expect(try tokens.accessToken() == "a3")
    #expect(transport.posts.count == 2)
}

@Test func aRefusedRefreshTokenIsUnauthorizedAndSigningOutForgetsIt() throws {
    let secrets = MemorySecrets()
    let transport = ScriptedTransport([(400, "{\"error\":\"invalid_grant\"}")])
    let tokens = GoogleAccessTokens(oauth: GoogleOAuth(config: config, transport: transport), store: secrets)
    #expect(throws: StoreFailure.Unauthorized) { try tokens.accessToken() }

    try tokens.adopt(GoogleOAuth.Grant(accessToken: "a", refreshToken: "r", lifetime: 0))
    #expect(throws: StoreFailure.Unauthorized) { try tokens.accessToken() }

    tokens.signOut()
    #expect(!tokens.isSignedIn)
    #expect(secrets.accounts.isEmpty)
    #expect(throws: SignInError.refused) {
        try tokens.adopt(GoogleOAuth.Grant(accessToken: "a", refreshToken: nil, lifetime: 1))
    }
}

@Test func syncStateIsKeptAcrossRelaunches() throws {
    let root = FileManager.default.temporaryDirectory.appendingPathComponent("figlock-state-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: root) }
    let container = VaultContainer(root: root)
    try container.prepare()

    let state = FileSyncState(container: container)
    #expect(!state.syncedBefore())
    state.markSynced(nowMs: 42)
    state.markSignInRefused()
    #expect(FileSyncState(container: container).syncedBefore())
    #expect(FileSyncState(container: container).lastSyncMs() == 42)
    #expect(FileSyncState(container: container).signInRefused())
    state.clearSignInRefused()
    state.forget()
    #expect(!FileSyncState(container: container).syncedBefore())
}

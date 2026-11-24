import CryptoKit
import FigLockFFI
import Foundation

/// Google sign-in for an iPhone: authorization code with PKCE S256 and the custom URL scheme redirect of an iOS
/// OAuth client (ADR-0021 describes the desktop variant; an iOS client has no secret). Only the `drive.appdata`
/// scope is asked for, and a grant of anything broader is refused, as on the desktop.
public struct GoogleOAuthConfig: Equatable, Sendable {
    public static let scope = "https://www.googleapis.com/auth/drive.appdata"
    public static let authURL = "https://accounts.google.com/o/oauth2/v2/auth"
    public static let tokenURL = "https://oauth2.googleapis.com/token"

    public let clientId: String

    public init(clientId: String) {
        self.clientId = clientId
    }

    /// The redirect scheme Google derives from an iOS client id: its reverse DNS form.
    public var redirectScheme: String {
        let suffix = ".apps.googleusercontent.com"
        let base = clientId.hasSuffix(suffix) ? String(clientId.dropLast(suffix.count)) : clientId
        return "com.googleusercontent.apps.\(base)"
    }

    public var redirectURI: String { "\(redirectScheme):/oauth2redirect" }
}

public enum PKCE {
    /// 32 random bytes as base64url, within the 43 to 128 characters RFC 7636 allows.
    public static func makeVerifier() -> String {
        var bytes = [UInt8](repeating: 0, count: 32)
        for index in bytes.indices { bytes[index] = UInt8.random(in: 0...255) }
        return base64URL(Data(bytes))
    }

    public static func challenge(for verifier: String) -> String {
        base64URL(Data(SHA256.hash(data: Data(verifier.utf8))))
    }

    static func base64URL(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}

/// One blocking HTTP POST of a form. The sync engine calls for a token from a background thread, so a blocking
/// call is fine there; tests replace it.
public protocol FormTransport: Sendable {
    func post(url: URL, form: [String: String]) throws -> (status: Int, body: Data)
}

public struct URLSessionTransport: FormTransport {
    public init() {}

    public func post(url: URL, form: [String: String]) throws -> (status: Int, body: Data) {
        var request = URLRequest(url: url, timeoutInterval: 30)
        request.httpMethod = "POST"
        request.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        var components = URLComponents()
        components.queryItems = form.sorted { $0.key < $1.key }.map { URLQueryItem(name: $0.key, value: $0.value) }
        request.httpBody = Data((components.percentEncodedQuery ?? "").utf8)
        let box = ResultBox()
        let done = DispatchSemaphore(value: 0)
        URLSession.shared.dataTask(with: request) { data, response, error in
            box.set(data: data, status: (response as? HTTPURLResponse)?.statusCode, error: error)
            done.signal()
        }.resume()
        done.wait()
        let outcome = box.get()
        guard outcome.error == nil, let status = outcome.status else { throw StoreFailure.Unavailable }
        return (status, outcome.data ?? Data())
    }

    private struct Reply {
        let data: Data?
        let status: Int?
        let error: Error?
    }

    private final class ResultBox: @unchecked Sendable {
        private let lock = NSLock()
        private var data: Data?
        private var status: Int?
        private var error: Error?

        func set(data: Data?, status: Int?, error: Error?) {
            lock.lock()
            defer { lock.unlock() }
            self.data = data
            self.status = status
            self.error = error
        }

        func get() -> Reply {
            lock.lock()
            defer { lock.unlock() }
            return Reply(data: data, status: status, error: error)
        }
    }
}

/// Why a sign-in step failed.
public enum SignInError: Error, Equatable, Sendable {
    /// This build has no Google client id (`FIGLOCK_GOOGLE_CLIENT_ID` in project.yml is empty).
    case notConfigured
    case cancelled
    /// Google or its token endpoint said no, for no reason this app can name.
    case refused
    /// The person pressed Cancel or Deny on Google's own consent screen.
    case accessDenied
    /// Google does not accept this build's OAuth client: the wrong client type (an iOS client has no secret and
    /// this app sends none), a client id that is not an iOS client of this bundle id, or a redirect it does not
    /// know. A setup problem of the build, not something the person did.
    case clientRejected
    /// The sign-in code was already used or has expired.
    case codeRejected
    /// Google finished the sign-in but gave no refresh token, so the phone could not stay signed in.
    case noRefreshToken
    /// Google granted more than the one scope that was asked for.
    case broaderScope
    case unavailable
}

private struct TokenResponse: Decodable {
    let accessToken: String
    let refreshToken: String?
    let expiresIn: Double?
    let scope: String?

    enum CodingKeys: String, CodingKey {
        case accessToken = "access_token"
        case refreshToken = "refresh_token"
        case expiresIn = "expires_in"
        case scope
    }
}

public struct GoogleOAuth: Sendable {
    public let config: GoogleOAuthConfig
    private let transport: any FormTransport

    public init(config: GoogleOAuthConfig, transport: any FormTransport = URLSessionTransport()) {
        self.config = config
        self.transport = transport
    }

    public func authorizationURL(state: String, challenge: String) -> URL? {
        var components = URLComponents(string: GoogleOAuthConfig.authURL)
        components?.queryItems = [
            URLQueryItem(name: "client_id", value: config.clientId),
            URLQueryItem(name: "redirect_uri", value: config.redirectURI),
            URLQueryItem(name: "response_type", value: "code"),
            URLQueryItem(name: "scope", value: GoogleOAuthConfig.scope),
            URLQueryItem(name: "state", value: state),
            URLQueryItem(name: "code_challenge", value: challenge),
            URLQueryItem(name: "code_challenge_method", value: "S256"),
            URLQueryItem(name: "access_type", value: "offline"),
            URLQueryItem(name: "prompt", value: "consent"),
        ]
        return components?.url
    }

    /// Why a redirect carries no code, when Google put an `error` in it. `nil` when there is none.
    public func failure(from redirect: URL) -> SignInError? {
        guard let items = URLComponents(url: redirect, resolvingAgainstBaseURL: false)?.queryItems,
            let error = items.first(where: { $0.name == "error" })?.value
        else { return nil }
        return error == "access_denied" ? .accessDenied : .refused
    }

    /// The `code` of a redirect, when its `state` is the one we sent. Anything else is `nil`.
    public func code(from redirect: URL, expectedState: String) -> String? {
        guard let items = URLComponents(url: redirect, resolvingAgainstBaseURL: false)?.queryItems,
            items.first(where: { $0.name == "state" })?.value == expectedState
        else { return nil }
        return items.first(where: { $0.name == "code" })?.value
    }

    public struct Grant: Sendable, Equatable {
        public let accessToken: String
        public let refreshToken: String?
        public let lifetime: TimeInterval
    }

    /// What the token endpoint's error body says. Only the standard `error` code is read, never a description,
    /// so nothing from the reply reaches a message.
    static func tokenError(_ body: Data) -> SignInError {
        let code = (try? JSONSerialization.jsonObject(with: body) as? [String: Any])?["error"] as? String
        switch code {
        case "invalid_client", "unauthorized_client", "redirect_uri_mismatch", "invalid_request": return .clientRejected
        case "invalid_grant": return .codeRejected
        case "access_denied": return .accessDenied
        default: return .refused
        }
    }

    private func decode(_ result: (status: Int, body: Data)) throws -> Grant {
        if result.status == 400 || result.status == 401 { throw Self.tokenError(result.body) }
        guard result.status == 200, let response = try? JSONDecoder().decode(TokenResponse.self, from: result.body)
        else { throw SignInError.unavailable }
        // Anything broader than the one scope means something other than what was asked for.
        if let scope = response.scope, Set(scope.split(separator: " ").map(String.init)) != [GoogleOAuthConfig.scope] {
            throw SignInError.broaderScope
        }
        return Grant(
            accessToken: response.accessToken, refreshToken: response.refreshToken,
            lifetime: response.expiresIn ?? 3000)
    }

    public func exchange(code: String, verifier: String) throws -> Grant {
        let form = [
            "client_id": config.clientId, "code": code, "code_verifier": verifier,
            "grant_type": "authorization_code", "redirect_uri": config.redirectURI,
        ]
        guard let url = URL(string: GoogleOAuthConfig.tokenURL) else { throw SignInError.unavailable }
        return try decode(try transport.post(url: url, form: form))
    }

    public func refresh(refreshToken: String) throws -> Grant {
        let form = ["client_id": config.clientId, "refresh_token": refreshToken, "grant_type": "refresh_token"]
        guard let url = URL(string: GoogleOAuthConfig.tokenURL) else { throw SignInError.unavailable }
        return try decode(try transport.post(url: url, form: form))
    }
}

/// The Drive adapter's token source: keeps the refresh token in the keychain (`ThisDeviceOnly`, like the other
/// secrets) and a short lived access token in memory.
public final class GoogleAccessTokens: AccessTokens, @unchecked Sendable {
    static let account = "google-drive-refresh-token"

    private let oauth: GoogleOAuth
    private let store: any SecretStore
    private let now: @Sendable () -> Date
    private let lock = NSLock()
    private var cached: Cached?

    private struct Cached {
        let token: String
        let expires: Date
    }

    public init(oauth: GoogleOAuth, store: any SecretStore, now: @escaping @Sendable () -> Date = { Date() }) {
        self.oauth = oauth
        self.store = store
        self.now = now
    }

    public var isSignedIn: Bool {
        ((try? store.read(Self.account)) ?? nil) != nil
    }

    /// Keep the grant of a finished sign-in.
    public func adopt(_ grant: GoogleOAuth.Grant) throws {
        guard let refresh = grant.refreshToken else { throw SignInError.noRefreshToken }
        try store.write(Self.account, Data(refresh.utf8))
        setCache(Cached(token: grant.accessToken, expires: now().addingTimeInterval(grant.lifetime - 60)))
    }

    public func signOut() {
        try? store.delete(Self.account)
        setCache(nil)
    }

    /// Delete a kept Google grant from `store` without a client id or a running sign-in: what "remove this vault from
    /// this iPhone" needs, since a build or a state with no sign-in can still hold a refresh token. Only the token
    /// on this phone goes; nothing in Drive is touched.
    public static func forgetGrant(in store: any SecretStore) {
        try? store.delete(account)
    }

    private func setCache(_ value: Cached?) {
        lock.lock()
        defer { lock.unlock() }
        cached = value
    }

    private func current() -> Cached? {
        lock.lock()
        defer { lock.unlock() }
        return cached
    }

    public func accessToken() throws -> String {
        if let cached = current(), cached.expires > now() { return cached.token }
        guard let data = try? store.read(Self.account), let refresh = String(data: data, encoding: .utf8) else {
            throw StoreFailure.Unauthorized
        }
        do {
            let grant = try oauth.refresh(refreshToken: refresh)
            setCache(Cached(token: grant.accessToken, expires: now().addingTimeInterval(grant.lifetime - 60)))
            return grant.accessToken
        } catch let failure as SignInError where failure != .unavailable {
            throw StoreFailure.Unauthorized
        } catch {
            throw StoreFailure.Unavailable
        }
    }

    public func invalidate() {
        setCache(nil)
    }
}

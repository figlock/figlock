import AuthenticationServices
import FigLockFFI
import Shared
import SwiftUI

/// Google Drive sign-in and the Drive store for onboarding (ADR-0021, ADR-0041 decision 3). The client id comes
/// from the build (`FigLockGoogleClientID` in Info.plist, set by `FIGLOCK_GOOGLE_CLIENT_ID`); a build without one has no sign-in and `signIn` says so instead of failing in a browser.
@MainActor
@Observable
final class DriveConnection: NSObject, ASWebAuthenticationPresentationContextProviding {
    private let oauth: GoogleOAuth?
    private let tokens: GoogleAccessTokens?
    private(set) var isSignedIn = false

    init(store: any SecretStore) {
        let clientId = (Bundle.main.object(forInfoDictionaryKey: "FigLockGoogleClientID") as? String) ?? ""
        if clientId.isEmpty {
            oauth = nil
            tokens = nil
        } else {
            let oauth = GoogleOAuth(config: GoogleOAuthConfig(clientId: clientId))
            self.oauth = oauth
            tokens = GoogleAccessTokens(oauth: oauth, store: store)
        }
        super.init()
        isSignedIn = tokens?.isSignedIn ?? false
    }

    var isConfigured: Bool { oauth != nil }

    func signIn() async throws {
        guard let oauth, let tokens else { throw SignInError.notConfigured }
        let verifier = PKCE.makeVerifier()
        let state = PKCE.makeVerifier()
        guard let url = oauth.authorizationURL(state: state, challenge: PKCE.challenge(for: verifier)) else {
            throw SignInError.unavailable
        }
        let redirect = try await authenticate(url: url, scheme: oauth.config.redirectScheme)
        guard let code = oauth.code(from: redirect, expectedState: state) else {
            throw oauth.failure(from: redirect) ?? SignInError.refused
        }
        let grant = try await Task.detached { try oauth.exchange(code: code, verifier: verifier) }.value
        try tokens.adopt(grant)
        isSignedIn = true
    }

    func signOut() {
        tokens?.signOut()
        isSignedIn = false
    }

    /// The Drive store, fed tokens by the app. Only valid while signed in.
    /// For the background sync, which runs off the main actor: the Drive store while signed in, otherwise nil.
    nonisolated func remoteIfSignedIn() -> (any BlobStoreFfi)? {
        guard let tokens, tokens.isSignedIn else { return nil }
        return DriveStore(tokens: tokens)
    }

    func remote() throws -> any BlobStoreFfi {
        guard let tokens else { throw SignInError.notConfigured }
        return DriveStore(tokens: tokens)
    }

    private func authenticate(url: URL, scheme: String) async throws -> URL {
        try await withCheckedThrowingContinuation { continuation in
            let session = ASWebAuthenticationSession(url: url, callbackURLScheme: scheme) { callback, error in
                if let callback {
                    continuation.resume(returning: callback)
                } else if let error = error as? ASWebAuthenticationSessionError, error.code == .canceledLogin {
                    continuation.resume(throwing: SignInError.cancelled)
                } else {
                    continuation.resume(throwing: SignInError.unavailable)
                }
            }
            session.presentationContextProvider = self
            session.start()
        }
    }

    nonisolated func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        MainActor.assumeIsolated {
            let scene = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
            return scene?.keyWindow ?? ASPresentationAnchor()
        }
    }
}

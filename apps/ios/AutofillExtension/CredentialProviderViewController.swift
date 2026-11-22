import AuthenticationServices
import UIKit

/// Placeholder so the extension target builds and embeds. The real provider is FIG-166; it is read-only, never
/// derives a key and holds one secret at a time (ADR-0041).
final class CredentialProviderViewController: ASCredentialProviderViewController {
    override func prepareCredentialList(for serviceIdentifiers: [ASCredentialServiceIdentifier]) {
        extensionContext.cancelRequest(
            withError: NSError(
                domain: ASExtensionErrorDomain, code: ASExtensionError.userCanceled.rawValue))
    }
}

import AuthenticationServices
import Shared
import SwiftUI
import UIKit

/// The AutoFill Credential Provider (ADR-0041 decision 7). It reads the local vault read-only, unlocks only
/// through Face ID (`AutofillAccess`, no password derivation anywhere), and holds one secret at a time.
final class CredentialProviderViewController: ASCredentialProviderViewController {
    private static let appGroup = "group.com.figlock.ios"

    private var vault: AutofillVault?
    private var hosting: UIHostingController<AnyView>?

    private func access() -> AutofillAccess? {
        guard let container = try? VaultContainer.appGroup(Self.appGroup) else { return nil }
        let local = LocalVault(container: container, access: .readOnly)
        let keeper = SecretKeyKeeper(store: KeychainSecretStore(accessGroup: nil), container: container)
        let quick = QuickUnlock(
            container: container, chip: SecureEnclaveChip(accessGroup: nil), biometricStateIsPerProcess: true)
        return AutofillAccess(vault: local, keeper: keeper, quick: quick)
    }

    // MARK: Entry points

    /// A silent fill is never offered: the vault is locked until Face ID, so the system shows the UI instead.
    override func provideCredentialWithoutUserInteraction(for credentialRequest: any ASCredentialRequest) {
        finish(error: .userInteractionRequired)
    }

    override func prepareInterfaceToProvideCredential(for credentialRequest: any ASCredentialRequest) {
        guard let identity = credentialRequest.credentialIdentity as? ASPasswordCredentialIdentity,
            let itemId = identity.recordIdentifier
        else { return finish(error: .credentialIdentityNotFound) }
        show(.working("Unlocking"))
        Task { await unlockThen { vault in await self.complete(vault: vault, itemId: itemId) } }
    }

    override func prepareCredentialList(for serviceIdentifiers: [ASCredentialServiceIdentifier]) {
        show(.working("Unlocking"))
        let identifiers = serviceIdentifiers.map(\.identifier)
        Task { await unlockThen { vault in self.showList(vault: vault, identifiers: identifiers) } }
    }

    // MARK: Flow

    private func unlockThen(_ next: @escaping @MainActor (AutofillVault) async -> Void) async {
        guard let access = access() else { return show(.message(.storage)) }
        do {
            let vault = try await access.open(reason: "Fill a password from FigLock")
            self.vault = vault
            await next(vault)
        } catch let failure as AutofillAccess.Failure {
            switch failure {
            case .cancelled: finish(error: .userCanceled)
            case .noVault: show(.message(.openApp(why: "FigLock has no vault on this iPhone.")))
            case .needsApp(let reason):
                var why = reason?.endedText ?? "Face ID is off for FigLock."
                if reason == .biometricsChanged { why += " (\(access.biometricNote()).)" }
                show(.message(.openApp(why: why)))
            case .notRecognised(let left): show(.message(.retry(left)))
            case .failed: show(.message(.failed))
            }
        } catch {
            show(.message(.failed))
        }
    }

    private func complete(vault: AutofillVault, itemId: String) async {
        do {
            let credential = try vault.credential(itemId: itemId)
            let result = ASPasswordCredential(user: credential.username, password: credential.password)
            vault.close()
            self.vault = nil
            extensionContext.completeRequest(withSelectedCredential: result)
        } catch {
            finish(error: .credentialIdentityNotFound)
        }
    }

    private func showList(vault: AutofillVault, identifiers: [String]) {
        let matches = vault.matches(identifiers: identifiers)
        show(
            .list(
                CredentialListModel(
                    vault: vault, initial: matches, identifiers: identifiers,
                    pick: { [weak self] item in Task { await self?.complete(vault: vault, itemId: item.id) } },
                    cancel: { [weak self] in self?.finish(error: .userCanceled) })))
    }

    private func finish(error code: ASExtensionError.Code) {
        vault?.close()
        vault = nil
        extensionContext.cancelRequest(withError: NSError(domain: ASExtensionErrorDomain, code: code.rawValue))
    }

    // MARK: Screens

    private func show(_ screen: AutofillScreen.Content) {
        let view = AnyView(
            AutofillScreen(content: screen, cancel: { [weak self] in self?.finish(error: .userCanceled) }))
        if let hosting {
            hosting.rootView = view
            return
        }
        let host = UIHostingController(rootView: view)
        addChild(host)
        host.view.frame = self.view.bounds
        host.view.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        self.view.addSubview(host.view)
        host.didMove(toParent: self)
        hosting = host
    }
}

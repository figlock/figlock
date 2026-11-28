import FigLockFFI
import Shared
import SwiftUI

/// What one unlock gives the app: the item browser, the writer and the clipboard copier (ADR-0048 decision 4). There
/// is one of these per unlocked session, held by `AppEnvironment`, not by a view, so a second window shows the same
/// vault from the same single writer. It is dropped when the vault locks; a new unlock builds a new one.
@MainActor
@Observable
final class UnlockedVault {
    let session: VaultSession
    let copier = ClipboardCopier()
    private(set) var browser: VaultBrowser?
    private(set) var editor: VaultEditor?

    private let vault: LocalVault
    private let sync: SyncModel
    private let identitySync: CredentialIdentitySync
    @ObservationIgnored private var loading = false

    init(session: VaultSession, vault: LocalVault, sync: SyncModel, identitySync: CredentialIdentitySync) {
        self.session = session
        self.vault = vault
        self.sync = sync
        self.identitySync = identitySync
    }

    /// Replay the log and build the writer. Safe to call from every window: only the first call does the work.
    func load() async {
        guard browser == nil, !loading else { return }
        loading = true
        defer { loading = false }
        let store = try? vault.openStore()
        let log = store.map { VaultLog(store: $0) }
        let loaded = VaultBrowser(session: session, log: log)
        loaded.identitySync = identitySync
        await loaded.reload()
        if let store {
            // Every remote of this phone, read when asked: an iCloud-only phone has no Google Drive record.
            let container = vault.container
            editor = VaultEditor(
                browser: loaded, store: store, pending: PendingSync(container: container),
                lastSyncMs: {
                    FileSyncState.settledMs(
                        container: container, remotes: RemoteList(container: container).entries())
                },
                // Every save goes to the remotes now, not at the next turn of the 15 second loop (FIG-261).
                didWrite: { [sync] in sync.noteLocalWrite() })
        }
        browser = loaded
    }
}

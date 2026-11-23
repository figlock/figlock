import FigLockFFI
import Shared
import SwiftUI

/// The unlocked app: the list of items and the detail screens above it. Everything it holds (the items, a shown
/// secret, the copied value) goes away when the vault locks, because the root swaps this view for the lock screen.
struct VaultRootView: View {
    let lock: AppLock
    let vault: LocalVault
    let sync: SyncModel
    let identitySync: CredentialIdentitySync

    @State private var browser: VaultBrowser?
    @State private var copier = ClipboardCopier()
    @State private var editor: VaultEditor?

    var body: some View {
        NavigationStack {
            if let browser {
                VaultListView(browser: browser, copier: copier, lock: lock, sync: sync)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity).background(FLColor.bg)
            }
        }
        // Above the stack, not on the list: a pushed detail screen takes its environment from the stack, so the
        // item actions (edit, delete) only see the editor from here.
        .environment(editor)
        .task {
            guard browser == nil, let session = lock.session else { return }
            let store = try? vault.openStore()
            let log = store.map { VaultLog(store: $0) }
            let loading = VaultBrowser(session: session, log: log)
            loading.identitySync = identitySync
            await loading.reload()
            if let store {
                let state = FileSyncState(container: vault.container)
                editor = VaultEditor(
                    browser: loading, store: store, pending: PendingSync(container: vault.container),
                    lastSyncMs: { state.lastSyncMs() })
            }
            browser = loading
        }
        // A sync that brought files in: read the log again.
        .onChange(of: sync.changeCount) { Task { await browser?.reload() } }
        .onDisappear { copier.clearNow() }
    }
}

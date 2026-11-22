import FigLockFFI
import Shared
import SwiftUI

/// The unlocked app: the list of items and the detail screens above it. Everything it holds (the items, a shown
/// secret, the copied value) goes away when the vault locks, because the root swaps this view for the lock screen.
struct VaultRootView: View {
    let lock: AppLock
    let vault: LocalVault

    @State private var browser: VaultBrowser?
    @State private var copier = ClipboardCopier()

    var body: some View {
        NavigationStack {
            if let browser {
                VaultListView(browser: browser, copier: copier, lock: lock)
            } else {
                ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity).background(FLColor.bg)
            }
        }
        .task {
            guard browser == nil, let session = lock.session else { return }
            let log = (try? vault.openStore()).map { VaultLog(store: $0) }
            let loading = VaultBrowser(session: session, log: log)
            await loading.reload()
            browser = loading
        }
        .onDisappear { copier.clearNow() }
    }
}

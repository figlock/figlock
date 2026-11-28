import FigLockFFI
import Shared
import SwiftUI

/// The unlocked app. On regular width (an iPad window that is wide enough) it is the three column split view; on
/// compact width (an iPhone, Split View 1/3, Slide Over) it is the phone's tabs and stack (ADR-0048 decision 2).
/// The window can cross that line at any time, so what the person is looking at lives in a `VaultNavigation` that both
/// layouts read and write. The items, the writer and the copied value belong to the unlock (`UnlockedVault`), not to
/// this view, and go away when the vault locks.
struct VaultRootView: View {
    let unlocked: UnlockedVault
    let sync: SyncModel

    @State private var navigation = VaultNavigation()
    @Environment(\.horizontalSizeClass) private var sizeClass

    var body: some View {
        Group {
            if sizeClass == .regular {
                VaultSplitView(unlocked: unlocked, navigation: navigation, sync: sync)
            } else {
                VaultCompactView(unlocked: unlocked, navigation: navigation, sync: sync)
            }
        }
        .tint(FLColor.primary)
        // Above the stack, not on the list: a pushed detail screen takes its environment from the stack, so the
        // item actions (edit, delete) only see the editor from here.
        .environment(unlocked.editor)
        .environment(navigation)
        .task { await unlocked.load() }
        // A sync finished: items it covered are no longer "not synced".
        .onChange(of: sync.rows) { unlocked.editor?.refreshUnsynced() }
        // A sync that brought files in: read the log again.
        .onChange(of: sync.changeCount) { Task { await unlocked.browser?.reload() } }
        // An item that is gone (deleted on another device) cannot stay selected.
        .onChange(of: unlocked.browser?.items.map(\.id)) {
            if let browser = unlocked.browser { navigation.clearSelectionIfMissing(in: browser) }
        }
    }
}

/// The iPhone layout: Vault and Settings tabs, each a stack. The stack's path is the navigation model's selected
/// item, so a window that was wide and went narrow shows the item it had open.
private struct VaultCompactView: View {
    let unlocked: UnlockedVault
    @Bindable var navigation: VaultNavigation
    let sync: SyncModel

    private var path: Binding<[String]> {
        Binding(
            get: { navigation.selectedItemId.map { [$0] } ?? [] },
            set: { navigation.selectedItemId = $0.last })
    }

    var body: some View {
        let browser = unlocked.browser
        TabView {
            Tab("Vault", systemImage: "lock") {
                NavigationStack(path: path) {
                    if let browser {
                        VaultListView(browser: browser, navigation: navigation, copier: unlocked.copier, sync: sync)
                    } else {
                        ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity).background(FLColor.bg)
                    }
                }
            }
            Tab("Settings", systemImage: "gearshape") {
                NavigationStack { SettingsTab(browser: browser, copier: unlocked.copier) }
            }
        }
    }
}

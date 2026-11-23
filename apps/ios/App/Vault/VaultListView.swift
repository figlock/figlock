import Shared
import SwiftUI

/// The vault list (`design/prototypes/ios`, "list"): items newest first, a type filter, and search that appears
/// when the list is pulled down.
struct VaultListView: View {
    @Bindable var browser: VaultBrowser
    let copier: ClipboardCopier
    let lock: AppLock
    let sync: SyncModel

    var body: some View {
        List {
            SyncChipRow(sync: sync)
            if browser.damagedCount > 0 {
                Label(
                    "\(browser.damagedCount) \(browser.damagedCount == 1 ? "item" : "items") could not be read",
                    systemImage: "exclamationmark.triangle.fill"
                )
                .font(FLFont.ui(FLText.base, relativeTo: .callout))
                .foregroundStyle(FLColor.danger)
                .listRowBackground(FLColor.dangerSubtle)
            }
            if !browser.loadFailures.isEmpty {
                Label(
                    "Some changes from other devices could not be read yet", systemImage: "arrow.triangle.2.circlepath"
                )
                .font(FLFont.ui(FLText.base, relativeTo: .callout))
                .foregroundStyle(FLColor.textSecondary)
                .listRowBackground(FLColor.warningSubtle)
            }
            if !browser.items.isEmpty {
                FilterRail(selection: $browser.filter)
                    .listRowInsets(EdgeInsets())
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
            }
            let rows = browser.visible
            if rows.isEmpty {
                EmptyState(isSearching: browser.isSearching, hasItems: !browser.items.isEmpty)
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
            }
            ForEach(rows) { item in
                NavigationLink(value: item.id) { ItemRow(item: item) }
                    .listRowBackground(FLColor.surface)
                    .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                        if let field = item.secretFields.first {
                            Button {
                                if let value = try? browser.session.revealField(itemId: item.id, field: field) {
                                    copier.copy(value, key: item.id + field)
                                }
                            } label: {
                                Label("Copy \(VaultItem.label(forSecret: field))", systemImage: "doc.on.doc")
                            }
                            .tint(FLColor.primary)
                        }
                    }
            }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(FLColor.bg)
        .navigationTitle("Vault")
        .navigationDestination(for: String.self) { id in
            ItemDetailHost(id: id, browser: browser, copier: copier)
        }
        .modifier(ItemManagementToolbar(browser: browser, copier: copier))
        // Search sits above the list and shows when it is pulled down, as in the prototype.
        .searchable(
            text: $browser.query, placement: .navigationBarDrawer(displayMode: .automatic),
            prompt: "Search names, aliases, domains"
        )
        .textInputAutocapitalization(.never)
        .autocorrectionDisabled()
        .refreshable {
            await sync.syncNow(force: true)
            await browser.reload()
        }
        .toolbar {
            ToolbarItem(placement: .topBarLeading) { SettingsLink() }
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    lock.lock()
                } label: {
                    Image(systemName: "lock")
                }
                .accessibilityLabel("Lock FigLock")
                .flMinTouchTarget()
            }
        }
    }
}

private struct FilterRail: View {
    @Binding var selection: VaultBrowser.Filter

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: FLSpace.s2) {
                ForEach(VaultBrowser.Filter.allCases) { filter in
                    Button(filter.title) { selection = filter }
                        .font(FLFont.ui(FLText.base, weight: FLWeight.medium, relativeTo: .callout))
                        .padding(.horizontal, FLSpace.s3)
                        .frame(minHeight: FLSize.controlHeightLg)
                        .foregroundStyle(selection == filter ? FLColor.textOnPrimary : FLColor.text)
                        .background(
                            selection == filter ? FLColor.primary : FLColor.surfaceSunken, in: Capsule()
                        )
                        .accessibilityAddTraits(selection == filter ? .isSelected : [])
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Filter items")
    }
}

struct ItemRow: View {
    let item: VaultItem
    @Environment(VaultEditor.self) private var editor: VaultEditor?
    private var unsynced: Bool { editor?.isUnsynced(item.id) == true }

    var body: some View {
        HStack(spacing: FLSpace.s3) {
            TypeTile(kind: item.kind, size: 40)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title)
                    .font(FLFont.ui(FLText.md, weight: FLWeight.semibold))
                    .foregroundStyle(FLColor.text)
                    .lineLimit(1)
                Text(item.subtitle)
                    .font(FLFont.ui(FLText.base, relativeTo: .callout))
                    .foregroundStyle(FLColor.textSecondary)
                    .lineLimit(1)
            }
            if unsynced {
                Spacer(minLength: 0)
                NotSyncedMark()
            }
        }
        .frame(minHeight: FLSize.controlHeightLg)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(
            "\(item.title), \(item.kind.displayName), \(item.subtitle)" + (unsynced ? ", not synced yet" : ""))
    }
}

struct TypeTile: View {
    let kind: ItemKind
    let size: CGFloat

    var body: some View {
        Image(systemName: kind.symbol)
            .font(.system(size: size * 0.45, weight: .medium))
            .foregroundStyle(FLColor.primary)
            .frame(width: size, height: size)
            .background(FLColor.primarySubtle, in: RoundedRectangle(cornerRadius: FLRadius.sm))
            .accessibilityHidden(true)
    }
}

private struct EmptyState: View {
    let isSearching: Bool
    let hasItems: Bool

    var body: some View {
        VStack(spacing: FLSpace.s2) {
            Image(systemName: isSearching ? "magnifyingglass" : "tray")
                .font(.system(size: 32))
                .foregroundStyle(FLColor.textTertiary)
                .accessibilityHidden(true)
            Text(title).font(FLFont.ui(FLText.lg, weight: FLWeight.semibold, relativeTo: .headline))
                .foregroundStyle(FLColor.text)
            Text(detail).font(FLFont.ui(FLText.base, relativeTo: .callout))
                .foregroundStyle(FLColor.textSecondary).multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, FLSpace.s8)
        .accessibilityElement(children: .combine)
    }

    private var title: String { isSearching ? "No matches" : hasItems ? "Nothing of this type" : "No items yet" }
    private var detail: String {
        isSearching
            ? "Check the spelling, or search by website or username."
            : hasItems ? "Choose All to see everything." : "Items you add or sync from your other devices appear here."
    }
}

/// The sync chip of the prototype: how syncing stands, worst problem first, and a way to the Sync screen. Nothing
/// shows while the vault is on this iPhone only.
private struct SyncChipRow: View {
    let sync: SyncModel
    @Environment(\.settingsContext) private var context

    var body: some View {
        if let chip = sync.chip, let context {
            NavigationLink {
                SyncSettingsView(context: context)
            } label: {
                HStack(spacing: FLSpace.s2) {
                    HStack(spacing: FLSpace.s2) {
                        Image(systemName: chip.state.symbol).accessibilityHidden(true)
                        Text(chip.title)
                    }
                    .font(FLFont.ui(FLText.base, weight: FLWeight.semibold, relativeTo: .callout))
                    .foregroundStyle(chip.state.tone.color)
                    .padding(.horizontal, FLSpace.s3)
                    .frame(minHeight: FLSize.controlHeightLg)
                    .background(chip.state.tone.background, in: RoundedRectangle(cornerRadius: FLRadius.sm))
                    Text(chip.detail).font(FLFont.ui(FLText.sm, relativeTo: .footnote))
                        .foregroundStyle(FLColor.textSecondary).lineLimit(2)
                    Spacer(minLength: 0)
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityHint("Opens sync settings")
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets())
            .listRowSeparator(.hidden)
        }
    }
}

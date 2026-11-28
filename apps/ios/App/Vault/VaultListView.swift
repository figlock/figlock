import Shared
import SwiftUI

/// The vault list (`design/prototypes/ios`, "list"): items newest first, a type filter, and search that is always shown
/// above the list.
struct VaultListView: View {
    let browser: VaultBrowser
    @Bindable var navigation: VaultNavigation
    let copier: ClipboardCopier
    let sync: SyncModel

    var body: some View {
        List {
            VaultHeader(sync: sync, query: $navigation.query)
                .listRowInsets(EdgeInsets())
                .listRowBackground(Color.clear)
                .listRowSeparator(.hidden)
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
                FilterRail(chips: VaultScope.chips, selection: $navigation.scope)
                    .listRowInsets(EdgeInsets())
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
            }
            let rows = browser.rows(query: navigation.query, scope: navigation.scope.forCompact)
            if !browser.items.isEmpty {
                Text(rows.count == 1 ? "1 item" : "\(rows.count) items")
                    .font(FLFont.ui(FLText.base, relativeTo: .callout))
                    .foregroundStyle(FLColor.textSecondary)
                    .listRowInsets(EdgeInsets(top: 0, leading: FLSpace.s4 + FLSpace.s1, bottom: 0, trailing: 0))
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
            }
            if rows.isEmpty {
                EmptyState(isSearching: navigation.isSearching, hasItems: !browser.items.isEmpty)
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
            }
            ForEach(rows) { item in
                NavigationLink(value: item.id) { ItemRow(item: item) }
                    .flTileRows()
                    .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                        CopySecretButton(item: item, browser: browser, copier: copier).tint(FLColor.primary)
                    }
            }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .contentMargins(.top, 0, for: .scrollContent)
        .background(FLColor.bg)
        .navigationTitle("Vault")
        .navigationBarTitleDisplayMode(.inline)
        // The big title is part of the list (`VaultHeader`), so the bar keeps its name but shows no second title.
        .toolbar { ToolbarItem(placement: .principal) { Color.clear.frame(width: 1, height: 1) } }
        .navigationDestination(for: String.self) { id in
            ItemDetailHost(id: id, browser: browser, copier: copier)
        }
        .modifier(ItemManagementToolbar(browser: browser, copier: copier, listsDeleted: true))
        .refreshable {
            await sync.syncNow(force: true)
            await browser.reload()
        }
    }
}

/// The type filter as a row of large chips (the prototype's "All, Logins, ..."): each is a full 44 pt target. When the
/// window's scope is one a chip cannot show (a tag, a single kind of the "Other" group) none is selected.
struct FilterRail: View {
    let chips: [(title: String, scope: VaultScope)]
    @Binding var selection: VaultScope

    var body: some View {
        HStack(spacing: FLSpace.s2) {
            ForEach(chips, id: \.title) { chip in
                let chosen = chip.scope == selection.forCompact
                Button {
                    selection = chip.scope
                } label: {
                    Text(chip.title)
                        .font(FLFont.ui(FLText.md, weight: FLWeight.semibold))
                        .foregroundStyle(chosen ? FLColor.textOnPrimary : FLColor.text)
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightMd)
                        .background(
                            chosen ? FLColor.primary : FLColor.surface,
                            in: RoundedRectangle(cornerRadius: FLRadius.lg))
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(chosen ? .isSelected : [])
            }
        }
        .padding(.horizontal, FLSpace.s4)
        .padding(.vertical, FLSpace.s1)
        .sensoryFeedback(.selection, trigger: selection)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Filter items")
    }
}

/// "Copy password" (or the first secret's name) for a row: the swipe action of the phone list and the context menu of
/// the iPad list. Nothing when the item has no secret.
struct CopySecretButton: View {
    let item: VaultItem
    let browser: VaultBrowser
    let copier: ClipboardCopier

    var body: some View {
        if let field = item.secretFields.first {
            Button {
                if let value = try? browser.session.revealField(itemId: item.id, field: field) {
                    copier.copy(value, key: item.id + field)
                }
            } label: {
                Label("Copy \(VaultItem.label(forSecret: field))", systemImage: "doc.on.doc")
            }
        }
    }
}

struct ItemRow: View {
    let item: VaultItem
    /// The row is the selected one of the iPad list: light text on the primary highlight.
    var isSelected = false
    @Environment(VaultEditor.self) private var editor: VaultEditor?
    private var unsynced: Bool { editor?.isUnsynced(item.id) == true }

    var body: some View {
        HStack(spacing: FLSpace.s3) {
            TypeTile(kind: item.kind, size: 40, isSelected: isSelected)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title)
                    .font(FLFont.ui(FLText.md, weight: FLWeight.semibold))
                    .foregroundStyle(isSelected ? FLColor.textOnPrimary : FLColor.text)
                    .lineLimit(1)
                Text(item.subtitle)
                    .font(FLFont.ui(FLText.base, relativeTo: .callout))
                    .foregroundStyle(isSelected ? FLColor.textOnPrimary.opacity(0.85) : FLColor.textSecondary)
                    .lineLimit(1)
            }
            if unsynced {
                Spacer(minLength: 0)
                NotSyncedMark()
            }
        }
        .padding(.vertical, FLSpace.s3)
        .frame(minHeight: FLSize.controlHeightLg)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(
            "\(item.title), \(item.kind.displayName), \(item.subtitle)" + (unsynced ? ", not synced yet" : ""))
    }
}

struct TypeTile: View {
    let kind: ItemKind
    let size: CGFloat
    var isSelected = false

    var body: some View {
        Image(systemName: kind.symbol)
            .font(.system(size: size * 0.45, weight: .medium))
            .foregroundStyle(isSelected ? FLColor.textOnPrimary : FLColor.primary)
            .frame(width: size, height: size)
            .background(
                isSelected ? Color.white.opacity(0.18) : FLColor.primarySubtle,
                in: RoundedRectangle(cornerRadius: FLRadius.sm)
            )
            .accessibilityHidden(true)
    }
}

struct EmptyState: View {
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
            ? "Try a different search."
            : hasItems ? "Choose All to see everything." : "Tap + to add one."
    }
}

/// The top of the vault list: the big title, one plain line about syncing when there is something to say, and
/// search. With nothing to say there is no line and no gap between the title and the search field.
private struct VaultHeader: View {
    let sync: SyncModel
    @Binding var query: String
    @FocusState private var searching: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s3) {
            VStack(alignment: .leading, spacing: FLSpace.s1) {
                Text("Vault")
                    .font(FLFont.ui(FLText.xl4, weight: FLWeight.bold, relativeTo: .largeTitle))
                    .foregroundStyle(FLColor.text)
                    .accessibilityAddTraits(.isHeader)
                SyncStatusText(sync: sync)
            }
            .animation(.default, value: sync.statusLine)
            HStack(spacing: FLSpace.s2) {
                Image(systemName: "magnifyingglass").foregroundStyle(FLColor.textSecondary)
                    .accessibilityHidden(true)
                TextField("Search", text: $query)
                    .font(FLFont.ui(FLText.md))
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.search)
                    .focused($searching)
                    .accessibilityIdentifier("vault-search")
                if !query.isEmpty {
                    Button {
                        query = ""
                    } label: {
                        Image(systemName: "xmark.circle.fill").foregroundStyle(FLColor.textTertiary)
                    }
                    .accessibilityLabel("Clear search")
                }
            }
            .padding(.horizontal, FLSpace.s3)
            .frame(minHeight: FLSize.controlHeightMd)
            .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.lg))
        }
        .padding(.horizontal, FLSpace.s4)
        .padding(.top, FLSpace.s1)
        .padding(.bottom, FLSpace.s2)
    }
}

/// "Syncing", "Synced" (three seconds), "Offline", or a problem that needs the person (it opens Sync). Plain text with
/// a small icon; nothing at all, and no space, when there is nothing to say.
struct SyncStatusText: View {
    let sync: SyncModel
    @Environment(\.settingsContext) private var context

    var body: some View {
        if let line = sync.statusLine {
            let problem = sync.attentionRows.first
            if line == .attention, let problem, let context {
                NavigationLink {
                    SyncSettingsView(context: context)
                } label: {
                    label(for: line, text: problem.state.title)
                }
                .buttonStyle(.plain)
                .accessibilityHint("Opens sync settings")
            } else {
                label(for: line, text: line.text)
            }
        }
    }

    private func label(for line: SyncStatusLine, text: String) -> some View {
        HStack(spacing: FLSpace.s1 + 2) {
            Image(systemName: line.symbol).accessibilityHidden(true)
            Text(text)
        }
        .font(FLFont.ui(FLText.base, weight: FLWeight.medium, relativeTo: .callout))
        .foregroundStyle(line.color)
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("sync-status")
        .transition(.opacity)
    }
}

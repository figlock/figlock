import Shared
import SwiftUI

/// The iPad layout (`design/prototypes/ipad`): sidebar, item list and detail. Everything the person has chosen is in
/// the window's `VaultNavigation`; this view only draws it.
struct VaultSplitView: View {
    let unlocked: UnlockedVault
    @Bindable var navigation: VaultNavigation
    let sync: SyncModel

    @State private var columns = NavigationSplitViewVisibility.automatic
    @State private var showSettings = false

    var body: some View {
        if let browser = unlocked.browser {
            NavigationSplitView(columnVisibility: $columns) {
                SidebarColumn(browser: browser, navigation: navigation, sync: sync, showSettings: $showSettings)
            } content: {
                ItemListColumn(
                    browser: browser, navigation: navigation, copier: unlocked.copier,
                    showsChips: columns != .all)
            } detail: {
                DetailColumn(browser: browser, navigation: navigation, copier: unlocked.copier)
            }
            .navigationSplitViewStyle(.balanced)
            // Wide windows dock the sidebar; a tall one (11 inch portrait) starts with it hidden (ADR-0048). A
            // rotation or resize applies the same rule; opening the sidebar by hand holds until the next change.
            .onGeometryChange(for: Bool.self) {
                $0.size.width > $0.size.height
            } action: { landscape in
                columns = landscape ? .all : .doubleColumn
            }
            .sheet(isPresented: $showSettings) {
                NavigationStack {
                    SettingsTab(browser: browser, copier: unlocked.copier)
                        .toolbar {
                            ToolbarItem(placement: .confirmationAction) { Button("Done") { showSettings = false } }
                        }
                }
                .presentationSizing(.page)
            }
        } else {
            ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity).background(FLColor.bg)
        }
    }
}

// MARK: Sidebar

private struct SidebarColumn: View {
    let browser: VaultBrowser
    @Bindable var navigation: VaultNavigation
    let sync: SyncModel
    @Binding var showSettings: Bool

    private var selection: Binding<VaultScope?> {
        Binding(get: { navigation.scope }, set: { if let scope = $0 { navigation.scope = scope } })
    }

    var body: some View {
        List(selection: selection) {
            row("All", symbol: "square.stack.3d.up", scope: .all, count: browser.count(in: .all))
            Section("Kinds") {
                ForEach(ItemKind.allCases) { kind in
                    let count = browser.count(in: .kind(kind))
                    if count > 0 || navigation.scope == .kind(kind) {
                        row(kind.pluralName, symbol: kind.symbol, scope: .kind(kind), count: count)
                    }
                }
            }
            if !browser.tagCounts.isEmpty || navigation.scope.isTag {
                Section("Tags") {
                    ForEach(browser.tagCounts, id: \.tag) { entry in
                        row(entry.tag, symbol: "tag", scope: .tag(entry.tag), count: entry.count)
                    }
                }
            }
            Section("Vault") {
                row("Recently deleted", symbol: "trash", scope: .deleted, count: browser.count(in: .deleted))
            }
        }
        .listStyle(.sidebar)
        .scrollContentBackground(.hidden)
        .background(FLColor.bg)
        .navigationTitle("FigLock")
        .safeAreaInset(edge: .bottom, spacing: 0) {
            VStack(alignment: .leading, spacing: FLSpace.s2) {
                SyncStatusText(sync: sync)
                Button {
                    showSettings = true
                } label: {
                    Label("Settings", systemImage: "gearshape")
                        .font(FLFont.ui(FLText.md))
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                }
                .buttonStyle(.plain)
                .foregroundStyle(FLColor.text)
                .accessibilityIdentifier("sidebar-settings")
            }
            .padding(.horizontal, FLSpace.s4)
            .padding(.vertical, FLSpace.s2)
            .background(.bar)
        }
    }

    private func row(_ title: String, symbol: String, scope: VaultScope, count: Int) -> some View {
        HStack {
            Label(title, systemImage: symbol)
                .font(FLFont.ui(FLText.md))
            Spacer(minLength: FLSpace.s2)
            Text("\(count)")
                .font(FLFont.ui(FLText.base, relativeTo: .callout))
                .foregroundStyle(FLColor.textSecondary)
                .monospacedDigit()
        }
        .tag(scope)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(title), \(count)")
    }
}

extension VaultScope {
    fileprivate var isTag: Bool {
        if case .tag = self { return true }
        return false
    }
}

// MARK: Item list

private struct ItemListColumn: View {
    let browser: VaultBrowser
    @Bindable var navigation: VaultNavigation
    let copier: ClipboardCopier
    /// With the sidebar hidden (11 inch portrait) the kinds are one tap away in the list.
    let showsChips: Bool

    @Environment(VaultEditor.self) private var editor: VaultEditor?

    var body: some View {
        Group {
            if navigation.scope == .deleted {
                DeletedItemsView(browser: browser)
            } else {
                list
            }
        }
        .navigationTitle(navigation.scope.title)
        .navigationBarTitleDisplayMode(.large)
    }

    private var list: some View {
        let rows = browser.rows(query: navigation.query, scope: navigation.scope)
        return List(selection: $navigation.selectedItemId) {
            if showsChips && !browser.items.isEmpty {
                FilterRail(chips: chips, selection: $navigation.scope)
                    .listRowInsets(EdgeInsets())
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
            }
            if rows.isEmpty {
                EmptyState(isSearching: navigation.isSearching, hasItems: !browser.items.isEmpty)
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
            } else {
                Text(rows.count == 1 ? "1 item" : "\(rows.count) items")
                    .font(FLFont.ui(FLText.base, relativeTo: .callout))
                    .foregroundStyle(FLColor.textSecondary)
                    .listRowBackground(Color.clear)
                    .listRowSeparator(.hidden)
            }
            ForEach(rows) { item in
                let selected = navigation.selectedItemId == item.id
                ItemRow(item: item, isSelected: selected)
                    .tag(item.id)
                    .id(item.id)
                    .listRowSeparator(.hidden)
                    .listRowInsets(EdgeInsets(top: 2, leading: FLSpace.s3, bottom: 2, trailing: FLSpace.s3))
                    .listRowBackground(
                        RoundedRectangle(cornerRadius: FLRadius.lg).fill(selected ? FLColor.primary : FLColor.surface)
                    )
                    .contextMenu { CopySecretButton(item: item, browser: browser, copier: copier) }
            }
        }
        .listStyle(.plain)
        .scrollContentBackground(.hidden)
        .background(FLColor.bg)
        .scrollPosition(id: $navigation.scrollAnchor, anchor: .top)
        .searchable(text: $navigation.query, placement: .navigationBarDrawer(displayMode: .always), prompt: "Search")
        .autocorrectionDisabled()
        .textInputAutocapitalization(.never)
        .modifier(ItemManagementToolbar(browser: browser, copier: copier, listsDeleted: false))
    }

    /// All, then the kinds this vault has.
    private var chips: [(title: String, scope: VaultScope)] {
        [("All", VaultScope.all)]
            + ItemKind.allCases.filter { browser.count(in: .kind($0)) > 0 }.map { ($0.pluralName, .kind($0)) }
    }
}

// MARK: Detail

private struct DetailColumn: View {
    let browser: VaultBrowser
    let navigation: VaultNavigation
    let copier: ClipboardCopier

    var body: some View {
        if let id = navigation.selectedItemId {
            if browser.item(id: id) != nil {
                ItemDetailHost(id: id, browser: browser, copier: copier)
                    // A new item is a new screen: a shown secret must not carry over to the next one.
                    .id(id)
            } else {
                NoItem(title: "No longer available", detail: "This item was deleted or changed on another device.")
            }
        } else {
            NoItem(title: "No item selected", detail: "Choose an item to see its details.")
        }
    }
}

/// The detail column with nothing to show. It carries no item data.
private struct NoItem: View {
    let title: String
    let detail: String

    var body: some View {
        VStack(spacing: FLSpace.s2) {
            Image(systemName: "lock")
                .font(.system(size: 40))
                .foregroundStyle(FLColor.textTertiary)
                .accessibilityHidden(true)
            Text(title).font(FLFont.ui(FLText.lg, weight: FLWeight.semibold, relativeTo: .headline))
                .foregroundStyle(FLColor.text)
            Text(detail).font(FLFont.ui(FLText.base, relativeTo: .callout))
                .foregroundStyle(FLColor.textSecondary).multilineTextAlignment(.center)
        }
        .padding(FLSpace.s6)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(FLColor.bg)
        .accessibilityElement(children: .combine)
    }
}

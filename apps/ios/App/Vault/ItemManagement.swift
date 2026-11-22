import FigLockFFI
import Shared
import SwiftUI
import UniformTypeIdentifiers

// Everything the vault screens need to change items, kept apart from the read screens: the add menu and the
// generator on the list, "Recently deleted", the edit and delete actions of the detail screen, the files of a
// document and the "not synced" markers.

// MARK: List

/// The add menu, the generator and "Recently deleted" for the vault list.
struct ItemManagementToolbar: ViewModifier {
    let browser: VaultBrowser
    let copier: ClipboardCopier
    @Environment(VaultEditor.self) private var editor: VaultEditor?

    @State private var creating: ItemKind?
    @State private var showGenerator = false
    @State private var showDeleted = false

    func body(content: Content) -> some View {
        content
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Menu {
                        Button {
                            showGenerator = true
                        } label: {
                            Label("Password generator", systemImage: "wand.and.stars")
                        }
                        Button {
                            showDeleted = true
                        } label: {
                            Label("Recently deleted", systemImage: "trash")
                        }
                    } label: {
                        Image(systemName: "ellipsis.circle")
                    }
                    .accessibilityLabel("More")
                    .flMinTouchTarget()
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        ForEach(ItemKind.allCases) { kind in
                            Button {
                                creating = kind
                            } label: {
                                Label(kind.displayName, systemImage: kind.symbol)
                            }
                        }
                    } label: {
                        Image(systemName: "plus")
                    }
                    .disabled(editor == nil)
                    .accessibilityLabel("Add item")
                    .flMinTouchTarget()
                }
            }
            .sheet(item: $creating) { kind in
                if let editor { ItemFormView(draft: ItemDraft(kind: kind), editor: editor, copier: copier) }
            }
            .sheet(isPresented: $showGenerator) { GeneratorView(copier: copier) }
            .sheet(isPresented: $showDeleted) { DeletedItemsView(browser: browser) }
    }
}

/// The mark on a row or a detail screen for a change that has not been synced to the other devices yet.
struct NotSyncedMark: View {
    var body: some View {
        Image(systemName: "arrow.triangle.2.circlepath")
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(FLColor.textTertiary)
            .accessibilityLabel("Not synced yet")
    }
}

// MARK: Recently deleted

struct DeletedItemsView: View {
    let browser: VaultBrowser
    @Environment(VaultEditor.self) private var editor: VaultEditor?
    @Environment(\.dismiss) private var dismiss
    @State private var failure: String?

    var body: some View {
        NavigationStack {
            List {
                if let failure { Notice(text: failure, isError: true).listRowBackground(Color.clear) }
                if browser.deletedItems.isEmpty {
                    VStack(spacing: FLSpace.s2) {
                        Image(systemName: "trash").font(.system(size: 32)).foregroundStyle(FLColor.textTertiary)
                            .accessibilityHidden(true)
                        Text("Nothing deleted").font(
                            FLFont.ui(FLText.lg, weight: FLWeight.semibold, relativeTo: .headline)
                        )
                        .foregroundStyle(FLColor.text)
                        Text("Items you delete stay here, so you can bring them back.")
                            .font(FLFont.ui(FLText.base, relativeTo: .callout)).foregroundStyle(FLColor.textSecondary)
                            .multilineTextAlignment(.center)
                    }
                    .frame(maxWidth: .infinity).padding(.vertical, FLSpace.s8)
                    .listRowBackground(Color.clear).listRowSeparator(.hidden)
                }
                ForEach(browser.deletedItems) { item in
                    HStack {
                        ItemRow(item: item)
                        Spacer(minLength: FLSpace.s2)
                        Button("Restore") { Task { await restore(item) } }
                            .buttonStyle(.bordered)
                            .tint(FLColor.primary)
                            .accessibilityLabel("Restore \(item.title)")
                    }
                    .listRowBackground(FLColor.surface)
                }
            }
            .listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(FLColor.bg)
            .navigationTitle("Recently deleted")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
    }

    private func restore(_ item: VaultItem) async {
        failure = nil
        do { try await editor?.restore(id: item.id) } catch let error as EditFailure { failure = error.message } catch {
            failure = EditFailure.rejected.message
        }
    }
}

// MARK: Detail

/// The detail screen, read from the browser each time it draws, so an edit shows at once.
struct ItemDetailHost: View {
    let id: String
    let browser: VaultBrowser
    let copier: ClipboardCopier

    var body: some View {
        if let item = browser.item(id: id) {
            ItemDetailView(item: item, session: browser.session, copier: copier)
        }
    }
}

/// The edit and delete actions of the detail screen.
struct ItemActions: ViewModifier {
    let item: VaultItem
    let copier: ClipboardCopier
    @Environment(VaultEditor.self) private var editor: VaultEditor?
    @Environment(\.dismiss) private var dismiss

    @State private var editing = false
    @State private var confirmDelete = false
    @State private var failure: String?

    func body(content: Content) -> some View {
        content
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Button {
                            editing = true
                        } label: {
                            Label("Edit", systemImage: "pencil")
                        }
                        Button(role: .destructive) {
                            confirmDelete = true
                        } label: {
                            Label("Delete", systemImage: "trash")
                        }
                    } label: {
                        Image(systemName: "ellipsis.circle")
                    }
                    .disabled(editor == nil || item.isDeleted)
                    .accessibilityLabel("Item actions")
                    .flMinTouchTarget()
                }
            }
            .sheet(isPresented: $editing) {
                if let editor { ItemFormView(draft: ItemDraft(editing: item), editor: editor, copier: copier) }
            }
            .confirmationDialog("Delete \(item.title)?", isPresented: $confirmDelete, titleVisibility: .visible) {
                Button("Delete", role: .destructive) { Task { await delete() } }
            } message: {
                Text("It moves to Recently deleted, where you can restore it.")
            }
            .alert(
                "Could not delete", isPresented: Binding(get: { failure != nil }, set: { if !$0 { failure = nil } })
            ) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(failure ?? "")
            }
    }

    private func delete() async {
        do {
            try await editor?.delete(id: item.id)
            dismiss()
        } catch let error as EditFailure {
            failure = error.message
        } catch {
            failure = EditFailure.rejected.message
        }
    }
}

/// The detail screen of an item that was deleted: restore is the one action.
struct RestoreBar: View {
    let item: VaultItem
    @Environment(VaultEditor.self) private var editor: VaultEditor?
    @State private var failure: String?

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s2) {
            Notice(text: "This item is deleted.", isError: false)
            Button("Restore") {
                Task {
                    do { try await editor?.restore(id: item.id) } catch { failure = EditFailure.rejected.message }
                }
            }
            .buttonStyle(PrimaryActionStyle())
            if let failure { Notice(text: failure, isError: true) }
        }
    }
}

/// "Saved on this phone, not synced yet" for an item changed here.
struct UnsyncedNote: View {
    let itemId: String
    @Environment(VaultEditor.self) private var editor: VaultEditor?

    var body: some View {
        if editor?.isUnsynced(itemId) == true {
            Label(
                "Saved on this phone. Not synced to your other devices yet.", systemImage: "arrow.triangle.2.circlepath"
            )
            .font(FLFont.ui(FLText.sm, relativeTo: .footnote))
            .foregroundStyle(FLColor.textSecondary)
        }
    }
}

/// The files of a document item: name, type and a save-to-Files button that asks for Face ID first.
struct FilesSection: View {
    let item: VaultItem
    let session: VaultSession

    @State private var failure: String?
    @State private var exporting: SavedFile?

    var body: some View {
        if !item.files.isEmpty {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(item.files) { file in
                    HStack {
                        Image(systemName: "doc.fill").foregroundStyle(FLColor.primary).accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(file.name).font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.text).lineLimit(2)
                            Text(file.mime).font(FLFont.ui(FLText.sm, relativeTo: .footnote))
                                .foregroundStyle(FLColor.textTertiary)
                        }
                        Spacer(minLength: FLSpace.s2)
                        Button {
                            Task { await save(file) }
                        } label: {
                            Label("Save", systemImage: "square.and.arrow.down")
                        }
                        .font(FLFont.ui(FLText.base, weight: FLWeight.semibold, relativeTo: .callout))
                        .padding(.horizontal, FLSpace.s3)
                        .frame(minHeight: FLSize.controlHeightLg)
                        .background(FLColor.primarySubtle, in: Capsule())
                        .accessibilityLabel("Save \(file.name) to Files")
                    }
                    .padding(FLSpace.s4)
                    if file.id != item.files.last?.id { Divider().padding(.leading, FLSpace.s4) }
                }
                if let failure {
                    Text(failure).font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.danger)
                        .padding(FLSpace.s4)
                }
            }
            .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.md))
            .fileExporter(
                isPresented: Binding(get: { exporting != nil }, set: { if !$0 { exporting = nil } }),
                document: exporting, contentType: .data, defaultFilename: exporting?.name ?? "file"
            ) { result in
                if case .failure = result { failure = "FigLock could not save the file." }
                exporting = nil
            }
        }
    }

    private func save(_ file: VaultItem.AttachedFile) async {
        failure = nil
        let extraction = FileExtraction(session: session, itemId: item.id)
        guard let data = await extraction.contents(of: file) else {
            failure = "The file was not saved. Check your Face ID or passcode, and try again."
            return
        }
        exporting = SavedFile(name: file.name, data: data)
    }
}

struct SavedFile: FileDocument {
    static let readableContentTypes: [UTType] = [.data]
    let name: String
    let data: Data

    init(name: String, data: Data) {
        self.name = name
        self.data = data
    }

    init(configuration: ReadConfiguration) throws {
        name = configuration.file.filename ?? "file"
        data = configuration.file.regularFileContents ?? Data()
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: data)
    }
}

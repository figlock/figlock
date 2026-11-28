import FigLockFFI
import Shared
import SwiftUI

/// One item: its fields, with secrets masked until the person shows them with Face ID.
struct ItemDetailView: View {
    let item: VaultItem
    let session: VaultSession
    let copier: ClipboardCopier

    @State private var reveal: SecretReveal
    @Environment(\.scenePhase) private var scenePhase

    init(item: VaultItem, session: VaultSession, copier: ClipboardCopier) {
        self.item = item
        self.session = session
        self.copier = copier
        _reveal = State(initialValue: SecretReveal(session: session, itemId: item.id))
    }

    var body: some View {
        List {
            if item.isDeleted {
                RestoreBar(item: item).listRowBackground(Color.clear).listRowInsets(EdgeInsets())
            }
            UnsyncedNote(itemId: item.id).listRowBackground(Color.clear)
            if !item.detailRows.isEmpty || !item.textSecretFields.isEmpty {
                Section {
                    ForEach(Array(item.detailRows.enumerated()), id: \.offset) { index, row in
                        PlainRow(label: row.label, value: row.value, copier: copier, key: item.id + "row\(index)")
                    }
                    ForEach(item.textSecretFields, id: \.self) { field in
                        SecretRow(item: item, field: field, reveal: reveal, copier: copier)
                    }
                }
                .flTileRows()
            }
            if !item.files.isEmpty {
                FilesSection(item: item, session: session)
                    .listRowBackground(Color.clear).listRowInsets(EdgeInsets())
            }
            if !item.tags.isEmpty {
                Section {
                    Text(item.tags.joined(separator: ", "))
                        .font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.text)
                } header: {
                    Text("Tags")
                }
                .flTileRows()
            }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(FLColor.bg)
        .navigationTitle(item.title)
        .navigationBarTitleDisplayMode(.inline)
        .modifier(ItemActions(item: item, copier: copier))
        // A shown secret hides when the person leaves the screen or the app, not only after 10 seconds.
        .onDisappear { reveal.hideAll() }
        .onChange(of: scenePhase) { _, phase in
            if phase != .active { reveal.hideForScenePhase(isBackground: phase == .background) }
        }
    }
}

/// The small "Copied" mark a row shows for a moment after it was tapped.
private struct CopiedMark: View {
    var body: some View {
        Label("Copied", systemImage: "checkmark")
            .font(FLFont.ui(FLText.sm, weight: FLWeight.medium, relativeTo: .footnote))
            .foregroundStyle(FLColor.success)
            .transition(.opacity)
    }
}

private struct PlainRow: View {
    let label: String
    let value: String
    let copier: ClipboardCopier
    let key: String

    var body: some View {
        HStack(alignment: .center) {
            VStack(alignment: .leading, spacing: 2) {
                Text(label).font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textSecondary)
                Text(value).font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.text)
            }
            Spacer(minLength: FLSpace.s2)
            if copier.copiedKey == key { CopiedMark() }
        }
        .padding(.vertical, FLRow.verticalPadding)
        .frame(minHeight: FLSize.controlHeightLg)
        .contentShape(Rectangle())
        .onTapGesture { copier.copy(value, key: key) }
        .animation(.easeOut(duration: 0.15), value: copier.copiedKey)
        .sensoryFeedback(.success, trigger: copier.copiedKey) { _, new in new == key }
        .accessibilityElement(children: .combine)
        .accessibilityHint("Copies \(label.lowercased())")
        .accessibilityAddTraits(.isButton)
    }
}

private struct SecretRow: View {
    let item: VaultItem
    let field: String
    let reveal: SecretReveal
    let copier: ClipboardCopier

    private var label: String { item.label(forSecret: field) }
    private var shown: String? { reveal.shown[field] }
    private var copyKey: String { item.id + field }

    var body: some View {
        HStack(alignment: .center) {
            VStack(alignment: .leading, spacing: 2) {
                Text(label).font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textSecondary)
                if let shown {
                    Text(shown).font(FLFont.mono(FLText.md)).foregroundStyle(FLColor.text)
                } else {
                    // Fixed-length dots, so the length of the secret is not leaked.
                    Text("••••••••••••").font(FLFont.mono(FLText.md)).foregroundStyle(FLColor.text)
                        .accessibilityLabel("\(label) hidden")
                }
                if reveal.failed.contains(field) {
                    Text("Could not read this field.")
                        .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.danger)
                }
            }
            .contentShape(Rectangle())
            .onTapGesture {
                if let value = reveal.valueForCopy(field) { copier.copy(value, key: copyKey) }
            }
            Spacer(minLength: FLSpace.s2)
            if copier.copiedKey == copyKey { CopiedMark() }
            Button {
                if shown != nil {
                    reveal.hide(field)
                } else {
                    Task { await reveal.show(field, label: label.lowercased()) }
                }
            } label: {
                Image(systemName: shown != nil ? "eye.slash" : "eye").foregroundStyle(FLColor.primary)
            }
            .buttonStyle(.borderless)
            .flMinTouchTarget()
            .accessibilityLabel("\(shown != nil ? "Hide" : "Show") \(label.lowercased())")
        }
        .padding(.vertical, FLRow.verticalPadding)
        .frame(minHeight: FLSize.controlHeightLg)
        .animation(.easeOut(duration: 0.15), value: copier.copiedKey)
        .sensoryFeedback(.success, trigger: copier.copiedKey) { _, new in new == copyKey }
        .accessibilityElement(children: .contain)
        .accessibilityAction(named: "Copy \(label.lowercased())") {
            if let value = reveal.valueForCopy(field) { copier.copy(value, key: copyKey) }
        }
    }
}

import FigLockFFI
import Shared
import SwiftUI

/// One item: its fields, with secrets masked until the person shows them with Face ID.
struct ItemDetailView: View {
    let item: VaultItem
    let copier: ClipboardCopier

    @State private var reveal: SecretReveal
    @Environment(\.scenePhase) private var scenePhase

    init(item: VaultItem, session: VaultSession, copier: ClipboardCopier) {
        self.item = item
        self.copier = copier
        _reveal = State(initialValue: SecretReveal(session: session, itemId: item.id))
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FLSpace.s5) {
                header
                if !item.detailRows.isEmpty || !item.secretFields.isEmpty {
                    VStack(spacing: 0) {
                        ForEach(Array(item.detailRows.enumerated()), id: \.offset) { index, row in
                            PlainRow(label: row.label, value: row.value, copier: copier, key: item.id + "row\(index)")
                            Divider().padding(.leading, FLSpace.s4)
                        }
                        ForEach(item.secretFields, id: \.self) { field in
                            SecretRow(item: item, field: field, reveal: reveal, copier: copier)
                            if field != item.secretFields.last { Divider().padding(.leading, FLSpace.s4) }
                        }
                    }
                    .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.md))
                }
                if !item.tags.isEmpty {
                    Text("Tags: " + item.tags.joined(separator: ", "))
                        .font(FLFont.ui(FLText.base, relativeTo: .callout)).foregroundStyle(FLColor.textSecondary)
                }
                Text("Edited " + item.updated.formatted(.relative(presentation: .named)))
                    .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textTertiary)
            }
            .padding(FLSpace.s4)
        }
        .background(FLColor.bg)
        .navigationTitle(item.title)
        .navigationBarTitleDisplayMode(.inline)
        // A shown secret hides when the person leaves the screen or the app, not only after 10 seconds.
        .onDisappear { reveal.hideAll() }
        .onChange(of: scenePhase) { _, phase in
            if phase != .active { reveal.hideAll() }
        }
    }

    private var header: some View {
        HStack(spacing: FLSpace.s4) {
            TypeTile(kind: item.kind, size: 56)
            VStack(alignment: .leading, spacing: 2) {
                Text(item.title).font(FLFont.ui(FLText.xl2, weight: FLWeight.bold, relativeTo: .title2))
                    .foregroundStyle(FLColor.text).accessibilityAddTraits(.isHeader)
                Text(item.kind.displayName).font(FLFont.ui(FLText.base, relativeTo: .callout))
                    .foregroundStyle(FLColor.textSecondary)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

private struct PlainRow: View {
    let label: String
    let value: String
    let copier: ClipboardCopier
    let key: String

    var body: some View {
        HStack(alignment: .top) {
            VStack(alignment: .leading, spacing: 2) {
                Text(label).font(FLFont.ui(FLText.sm, weight: FLWeight.medium, relativeTo: .footnote))
                    .foregroundStyle(FLColor.textSecondary)
                Text(value).font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.text).textSelection(.enabled)
            }
            Spacer(minLength: FLSpace.s2)
            Button {
                copier.copy(value, key: key)
            } label: {
                Label("Copy", systemImage: "doc.on.doc")
            }
            .buttonStyle(PillStyle())
            .accessibilityLabel("Copy \(label.lowercased())")
        }
        .padding(FLSpace.s4)
        .accessibilityElement(children: .contain)
    }
}

private struct SecretRow: View {
    let item: VaultItem
    let field: String
    let reveal: SecretReveal
    let copier: ClipboardCopier

    private var label: String { VaultItem.label(forSecret: field) }
    private var shown: String? { reveal.shown[field] }
    private var copyKey: String { item.id + field }

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s2) {
            Text(label).font(FLFont.ui(FLText.sm, weight: FLWeight.medium, relativeTo: .footnote))
                .foregroundStyle(FLColor.textSecondary)
            Group {
                if let shown {
                    Text(shown).font(FLFont.mono(FLText.md)).textSelection(.enabled)
                } else {
                    // Fixed-length dots, so the length of the secret is not leaked.
                    Text("••••••••••••").font(FLFont.mono(FLText.md))
                        .accessibilityLabel("\(label) hidden")
                }
            }
            .foregroundStyle(FLColor.text)
            HStack(spacing: FLSpace.s2) {
                Button {
                    if shown != nil {
                        reveal.hide(field)
                    } else {
                        Task { await reveal.show(field, label: label.lowercased()) }
                    }
                } label: {
                    Label(shown != nil ? "Hide" : "Show", systemImage: shown != nil ? "eye.slash" : "eye")
                }
                .buttonStyle(PillStyle())
                .accessibilityLabel("\(shown != nil ? "Hide" : "Show") \(label.lowercased())")
                Button {
                    if let value = reveal.valueForCopy(field) { copier.copy(value, key: copyKey) }
                } label: {
                    Label("Copy", systemImage: "doc.on.doc")
                }
                .buttonStyle(PillStyle())
                .accessibilityLabel("Copy \(label.lowercased()), clears from the clipboard in 30 seconds")
            }
            if copier.copiedKey == copyKey {
                Text("Copied. The clipboard clears in 30 s")
                    .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.success)
            } else if shown != nil {
                Text("Hides again in 10 s")
                    .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textTertiary)
            }
            if reveal.failed.contains(field) {
                Text("FigLock could not read this field.")
                    .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.danger)
            }
        }
        .padding(FLSpace.s4)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .contain)
    }
}

private struct PillStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(FLFont.ui(FLText.base, weight: FLWeight.semibold, relativeTo: .callout))
            .foregroundStyle(FLColor.primary)
            .padding(.horizontal, FLSpace.s3)
            .frame(minHeight: FLSize.controlHeightLg)
            .background(FLColor.primarySubtle, in: Capsule())
            .opacity(configuration.isPressed ? 0.7 : 1)
    }
}

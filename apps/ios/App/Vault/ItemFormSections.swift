import Shared
import SwiftUI

// The repeating parts of the item form: websites and hosts, environment variables, custom fields and files.

private struct FormCard: ViewModifier {
    func body(content: Content) -> some View {
        content
            .padding(.horizontal, FLSpace.s3)
            .padding(.vertical, FLSpace.s2)
            .frame(minHeight: FLSize.controlHeightLg)
            .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.sm))
            .overlay(RoundedRectangle(cornerRadius: FLRadius.sm).stroke(FLColor.border))
    }
}

extension View {
    fileprivate func formCard() -> some View { modifier(FormCard()) }
}

private struct SectionTitle: View {
    let text: String
    var body: some View {
        Text(text).font(FLFont.ui(FLText.sm, weight: FLWeight.medium)).foregroundStyle(FLColor.textSecondary)
    }
}

private struct AddButton: View {
    let title: String
    let action: () -> Void
    var body: some View {
        Button(action: action) { Label(title, systemImage: "plus") }
            .font(FLFont.ui(FLText.base, weight: FLWeight.semibold, relativeTo: .callout))
            .foregroundStyle(FLColor.textLink)
            .flMinTouchTarget()
    }
}

private struct RemoveButton: View {
    let label: String
    let action: () -> Void
    var body: some View {
        Button(role: .destructive, action: action) { Image(systemName: "minus.circle.fill") }
            .flMinTouchTarget()
            .accessibilityLabel(label)
    }
}

/// Websites or allowed hosts: one text field per entry.
struct EntryListSection: View {
    let title: String
    @Binding var entries: [DraftEntry]
    let placeholder: String
    let addLabel: String
    var hint: String?
    let onAdd: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s2) {
            SectionTitle(text: title)
            ForEach($entries) { $entry in
                HStack {
                    TextField(placeholder, text: $entry.value)
                        .textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                    RemoveButton(label: "Remove \(title.lowercased())") {
                        entries.removeAll { $0.id == entry.id }
                    }
                }
                .formCard()
            }
            AddButton(title: addLabel, action: onAdd)
            if let hint {
                Text(hint).font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textTertiary)
            }
        }
    }
}

/// Environment variables of an env bundle. An existing variable keeps its name and shows "Unchanged".
struct EnvironmentSection: View {
    @Binding var draft: ItemDraft

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s2) {
            SectionTitle(text: "Variables")
            ForEach($draft.env) { $entry in
                VStack(alignment: .leading, spacing: FLSpace.s2) {
                    HStack {
                        if entry.isNew {
                            TextField("API_KEY", text: $entry.name)
                                .textInputAutocapitalization(.characters).autocorrectionDisabled()
                                .font(FLFont.mono(FLText.base))
                        } else {
                            Text(entry.name).font(FLFont.mono(FLText.base)).foregroundStyle(FLColor.text)
                            Spacer()
                        }
                        RemoveButton(label: "Remove variable \(entry.name)") {
                            draft.env.removeAll { $0.id == entry.id }
                        }
                    }
                    SecureField(entry.isNew ? "Value" : "Unchanged", text: $entry.value)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                        .font(FLFont.mono(FLText.base))
                }
                .formCard()
            }
            AddButton(title: "Add variable") { draft.addEnv() }
        }
    }
}

/// Custom fields: a name and a secret value each.
struct CustomFieldsSection: View {
    @Binding var draft: ItemDraft

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s2) {
            ForEach($draft.custom) { $entry in
                VStack(alignment: .leading, spacing: FLSpace.s2) {
                    HStack {
                        TextField("Field name", text: $entry.name)
                        RemoveButton(label: "Remove custom field") {
                            draft.custom.removeAll { $0.id == entry.id }
                        }
                    }
                    SecureField(entry.isNew ? "Secret value" : "Unchanged", text: $entry.value)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                        .font(FLFont.mono(FLText.base))
                }
                .formCard()
            }
            AddButton(title: "Add custom field") { draft.addCustom() }
        }
    }
}

/// The files of a document item while editing: the ones it holds and the ones just picked.
struct DraftFilesSection: View {
    @Binding var draft: ItemDraft
    @Binding var showPicker: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s2) {
            SectionTitle(text: "Files")
            ForEach(draft.files) { file in
                HStack {
                    Image(systemName: "doc.fill").foregroundStyle(FLColor.primary).accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(file.name).font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.text).lineLimit(1)
                        Text(size(of: file))
                            .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textTertiary)
                    }
                    Spacer()
                    RemoveButton(label: "Remove \(file.name)") {
                        draft.files.removeAll { $0.id == file.id }
                    }
                }
                .formCard()
            }
            AddButton(title: "Add file from Files") { showPicker = true }
                .disabled(draft.files.count >= ItemDraft.maxFiles)
            Text("Up to \(ItemDraft.maxFiles) files, 768 KB each.")
                .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textTertiary)
        }
    }

    private func size(of file: DraftFile) -> String {
        guard let data = file.data else { return "Stored in the vault" }
        return ByteCountFormatter.string(fromByteCount: Int64(data.count), countStyle: .file)
    }
}

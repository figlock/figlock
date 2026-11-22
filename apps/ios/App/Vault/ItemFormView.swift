import Shared
import SwiftUI
import UniformTypeIdentifiers

/// Create or edit one item. The form works on an `ItemDraft`: a stored secret is never put into a field, it shows
/// as "unchanged" and is only replaced when something is typed (or removed on purpose).
struct ItemFormView: View {
    @State var draft: ItemDraft
    let editor: VaultEditor
    let copier: ClipboardCopier

    @Environment(\.dismiss) private var dismiss
    @State private var saving = false
    @State private var failure: String?
    @State private var generatingFor: String?
    @State private var showFilePicker = false
    @State private var shownSecrets: Set<String> = []
    @FocusState private var titleFocused: Bool

    private var navigationTitle: String {
        draft.isNew ? "New \(draft.kind.displayName.lowercased())" : "Edit \(draft.kind.displayName.lowercased())"
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: FLSpace.s4) {
                    LabeledField(label: "Name") {
                        TextField("Name", text: $draft.title).focused($titleFocused)
                    }
                    ForEach(draft.specs, id: \.name) { spec in field(spec) }
                    if draft.usesWebsites {
                        EntryListSection(
                            title: "Websites", entries: $draft.urls, placeholder: "https://example.com",
                            addLabel: "Add website", onAdd: { draft.addURL() })
                    }
                    if draft.usesAllowedHosts {
                        EntryListSection(
                            title: "Allowed hosts", entries: $draft.allowedHosts, placeholder: "api.example.com",
                            addLabel: "Add host", onAdd: { draft.addAllowedHost() })
                    }
                    if draft.usesEnv { EnvironmentSection(draft: $draft) }
                    if draft.usesFiles { DraftFilesSection(draft: $draft, showPicker: $showFilePicker) }
                    CustomFieldsSection(draft: $draft)
                    if draft.kind != .note {
                        LabeledField(label: "Notes") {
                            TextField("Notes", text: $draft.notes, axis: .vertical).lineLimit(2...8)
                        }
                    }
                    LabeledField(label: "Tags", hint: "Separate with commas. Lowercase letters, digits, - and _.") {
                        TextField("work, mail", text: $draft.tagsText)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                    }
                    if let message = failure ?? (draft.title.isEmpty ? nil : draft.problem) {
                        Notice(text: message, isError: true)
                    }
                }
                .padding(FLSpace.s4)
            }
            .scrollDismissesKeyboard(.interactively)
            .background(FLColor.bg.ignoresSafeArea())
            .navigationTitle(navigationTitle)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Save") { Task { await save() } }
                        .disabled(saving || draft.problem != nil)
                        .fontWeight(.semibold)
                }
            }
            .interactiveDismissDisabled(saving)
            .sheet(
                isPresented: Binding(get: { generatingFor != nil }, set: { if !$0 { generatingFor = nil } })
            ) {
                GeneratorView(copier: copier) { generated in
                    if let name = generatingFor {
                        draft.values[name] = generated
                        shownSecrets.insert(name)
                    }
                }
            }
            .fileImporter(
                isPresented: $showFilePicker, allowedContentTypes: [.item], allowsMultipleSelection: true,
                onCompletion: addFiles)
        }
        .onAppear { if draft.isNew { titleFocused = true } }
    }

    // MARK: Fields

    @ViewBuilder private func field(_ spec: FieldSpec) -> some View {
        let binding = Binding(get: { draft.values[spec.name] ?? "" }, set: { draft.values[spec.name] = $0 })
        if spec.isSecret {
            secretField(spec, binding)
        } else {
            LabeledField(label: spec.label) {
                if spec.isMultiline {
                    TextField(spec.label, text: binding, axis: .vertical).lineLimit(3...12)
                        .font(FLFont.mono(FLText.base))
                } else {
                    TextField(spec.placeholder.isEmpty ? spec.label : spec.placeholder, text: binding)
                        .textInputAutocapitalization(.never).autocorrectionDisabled()
                }
            }
        }
    }

    @ViewBuilder private func secretField(_ spec: FieldSpec, _ binding: Binding<String>) -> some View {
        let stored = draft.hasStoredSecret(spec.name)
        let removed = draft.removedSecrets.contains(spec.name)
        let shown = shownSecrets.contains(spec.name)
        LabeledField(label: spec.label) {
            HStack(spacing: FLSpace.s2) {
                Group {
                    if shown || spec.isMultiline {
                        TextField(placeholder(stored: stored, removed: removed), text: binding, axis: .vertical)
                            .lineLimit(1...(spec.isMultiline ? 10 : 3))
                    } else {
                        SecureField(placeholder(stored: stored, removed: removed), text: binding)
                    }
                }
                .font(FLFont.mono(FLText.base))
                .textInputAutocapitalization(.never).autocorrectionDisabled()
                if !spec.isMultiline {
                    Button {
                        if shown { shownSecrets.remove(spec.name) } else { shownSecrets.insert(spec.name) }
                    } label: {
                        Image(systemName: shown ? "eye.slash" : "eye")
                    }
                    .flMinTouchTarget()
                    .accessibilityLabel(shown ? "Hide \(spec.label.lowercased())" : "Show \(spec.label.lowercased())")
                }
                if spec.offersGenerator {
                    Button {
                        generatingFor = spec.name
                    } label: {
                        Image(systemName: "wand.and.stars")
                    }
                    .flMinTouchTarget()
                    .accessibilityLabel("Generate a password")
                }
            }
        }
        if spec.offersGenerator, let rating = GeneratorSettings.rating(of: binding.wrappedValue) {
            StrengthLabel(rating: rating)
        }
        if stored {
            Button(
                removed ? "Keep the stored \(spec.label.lowercased())" : "Remove the stored \(spec.label.lowercased())"
            ) {
                if removed { draft.removedSecrets.remove(spec.name) } else { draft.removedSecrets.insert(spec.name) }
            }
            .font(FLFont.ui(FLText.sm, weight: FLWeight.medium, relativeTo: .footnote))
            .foregroundStyle(removed ? FLColor.textLink : FLColor.danger)
        }
    }

    private func placeholder(stored: Bool, removed: Bool) -> String {
        if removed { return "Will be removed" }
        return stored ? "Unchanged" : ""
    }

    // MARK: Actions

    private func addFiles(_ result: Result<[URL], Error>) {
        guard case .success(let urls) = result else { return }
        for url in urls {
            do {
                try draft.attach(DocumentFiles.read(url, id: draft.newFileId()))
            } catch let error as ItemDraftError {
                failure = error.message
            } catch {
                failure = ItemDraftError.unreadableFile(url.lastPathComponent).message
            }
        }
    }

    private func save() async {
        saving = true
        failure = nil
        defer { saving = false }
        do {
            if draft.isNew { try await editor.create(draft) } else { try await editor.update(draft) }
            dismiss()
        } catch let error as EditFailure {
            failure = error.message
        } catch {
            failure = EditFailure.rejected.message
        }
    }
}

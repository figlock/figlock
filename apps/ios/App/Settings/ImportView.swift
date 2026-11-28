import FigLockFFI
import Shared
import SwiftUI
import UniformTypeIdentifiers

/// Settings > Import (FIG-286): items from Chrome, Apple Passwords or a notes file. The person picks the source
/// first, then the file from the Files app. The file is parsed by the Rust core and only counts and titles come
/// back to preview; nothing is added until they confirm. Items then show as not synced until a sync has run.
struct ImportView: View {
    private enum Source: String, CaseIterable, Identifiable {
        case chrome, apple, notes
        var id: String { rawValue }
        var title: String {
            switch self {
            case .chrome: "Chrome"
            case .apple: "Apple Passwords"
            case .notes: "Notes"
            }
        }
        var help: String {
            switch self {
            case .chrome: "Export from Chrome: Password Manager, Settings, Export passwords. Choose the CSV file."
            case .apple: "Export from the Passwords app on a Mac: File, Export All Passwords. Choose the CSV file."
            case .notes: "A text file where notes are separated by a line of --- or ===."
            }
        }
        var types: [UTType] { self == .notes ? [.plainText, .text] : [.commaSeparatedText, .plainText] }
    }

    @Environment(VaultEditor.self) private var editor: VaultEditor?

    @State private var source: Source = .chrome
    @State private var picking = false
    @State private var working = false
    @State private var plan: ImportPlan?
    @State private var preview: ImportPreview?
    @State private var message: String?
    @State private var isError = true

    var body: some View {
        List {
            if let preview {
                previewSections(preview)
            } else {
                chooseSections
            }
            if let message {
                Notice(text: message, isError: isError)
                    .listRowBackground(Color.clear)
                    .accessibilityIdentifier("import-message")
            }
        }
        .flTileList()
        .navigationTitle("Import")
        .navigationBarTitleDisplayMode(.inline)
        .fileImporter(isPresented: $picking, allowedContentTypes: source.types) { result in
            guard case .success(let url) = result else { return }
            Task { await load(url) }
        }
        .onDisappear { plan = nil }
    }

    @ViewBuilder private var chooseSections: some View {
        Section {
            Picker("Where is the file from?", selection: $source) {
                ForEach(Source.allCases) { Text($0.title).tag($0) }
            }
            .accessibilityIdentifier("import-source")
        } header: {
            Text("Source").textCase(nil)
        } footer: {
            Text(source.help)
        }
        .flTileRows()

        Section {
            Notice(
                text:
                    "The file holds your passwords in plain text. Delete it once the import is done, and empty Recently Deleted in Files.",
                isError: false)
        }
        .listRowBackground(Color.clear)

        Section {
            Button {
                choose()
            } label: {
                Label(working ? "Reading..." : "Choose file", systemImage: "doc")
                    .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
            }
            .buttonStyle(PrimaryActionStyle())
            .disabled(working)
            .accessibilityIdentifier("import-choose")
        }
        .listRowBackground(Color.clear)
        .listRowInsets(EdgeInsets())
    }

    @ViewBuilder private func previewSections(_ found: ImportPreview) -> some View {
        let total = Int(found.logins + found.notes + found.tokens)
        Section {
            Text("\(found.logins) logins, \(found.notes) notes, \(found.tokens) API tokens")
                .accessibilityIdentifier("import-counts")
            if found.skipped > 0 { Text("\(found.skipped) skipped (empty, too large or not supported)") }
            if found.tokensDetected > 0 {
                Text("Tokens found in notes are saved as token items and removed from the note text.")
            }
        } header: {
            Text("\(total) \(total == 1 ? "item" : "items") found").textCase(nil)
        } footer: {
            Text("Nothing has been added yet.")
        }
        .flTileRows()

        Section {
            ForEach(Array(found.titles.enumerated()), id: \.offset) { _, title in
                Text(title.isEmpty ? "Untitled" : title).lineLimit(1)
            }
            if found.more > 0 { Text("and \(found.more) more").foregroundStyle(.secondary) }
        }
        .flTileRows()

        Section {
            Button {
                Task { await confirm(total: total) }
            } label: {
                Text(working ? "Importing..." : "Import \(total) \(total == 1 ? "item" : "items")")
                    .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
            }
            .buttonStyle(PrimaryActionStyle())
            .disabled(working)
            .accessibilityIdentifier("import-confirm")
            Button("Cancel") {
                plan = nil
                preview = nil
                message = nil
            }
            .buttonStyle(SecondaryActionStyle())
            .disabled(working)
            .accessibilityIdentifier("import-cancel")
        }
        .listRowBackground(Color.clear)
        .listRowInsets(EdgeInsets())
    }

    private func choose() {
        message = nil
        #if DEBUG
            // The UI test cannot drive the system file picker: it names generated bytes to use instead.
            if let bytes = ImportFixture.bytes(source: source.rawValue) {
                prepare(bytes)
                return
            }
        #endif
        picking = true
    }

    private func load(_ url: URL) async {
        working = true
        defer { working = false }
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        // One byte past the limit is enough for the core to say "too large" without reading a huge file whole.
        guard let handle = try? FileHandle(forReadingFrom: url),
            let data = try? handle.read(upToCount: 16 * 1024 * 1024 + 1)
        else {
            isError = true
            message = "That file could not be read."
            return
        }
        try? handle.close()
        prepare(data)
    }

    private func prepare(_ bytes: Data) {
        do {
            let made = try prepareImport(source: source.rawValue, bytes: bytes)
            plan = made
            preview = made.preview()
        } catch let failure as ImportFailure {
            isError = true
            message = Self.words(failure)
        } catch {
            isError = true
            message = "That file could not be imported."
        }
    }

    private func confirm(total: Int) async {
        guard let plan, let editor else { return }
        working = true
        defer { working = false }
        do {
            let added = try await editor.importItems(plan)
            self.plan = nil
            preview = nil
            isError = false
            message =
                "Imported \(added) \(added == 1 ? "item" : "items"). They sync with your next sync. Delete the export file now."
        } catch let partial as ImportPartial {
            self.plan = nil
            preview = nil
            isError = true
            message =
                "The import stopped after \(partial.added) items were added. Those are in the vault, the rest are not."
        } catch {
            self.plan = nil
            preview = nil
            isError = true
            message = "Nothing was imported. FigLock could not write to its storage on this phone. Try again."
        }
    }

    private static func words(_ failure: ImportFailure) -> String {
        switch failure {
        case .WrongFormat: "That file does not look like an export. Check the source above."
        case .TooLarge: "That file is too large to import (limit 16 MB)."
        case .NotUtf8: "That file is not a text file."
        case .Malformed: "That file is not a well-formed CSV file."
        case .TooManyRecords: "That file has too many records to import (limit 50,000)."
        case .Empty: "That file has nothing to import."
        case .UnknownSource: "Choose where the file came from."
        }
    }

}

#if DEBUG
    /// Generated files for the UI test, chosen with `-figlock-import-fixture <name>`.
    private enum ImportFixture {
        static func bytes(source: String) -> Data? {
            let args = ProcessInfo.processInfo.arguments
            guard let flag = args.firstIndex(of: "-figlock-import-fixture"), flag + 1 < args.count else { return nil }
            switch args[flag + 1] {
            case "good" where source == "chrome":
                return Data(
                    "name,url,username,password,note\nImported One,https://one.example,me,generated-pw-1,\nImported Two,https://two.example,me,generated-pw-2,\n"
                        .utf8)
            case "good" where source == "notes":
                return Data("Imported Note\nbody\n---\nSecond Note\nbody\n".utf8)
            default:
                return Data("this,is,not,an,export\n1,2,3,4,5\n".utf8)
            }
        }
    }
#endif

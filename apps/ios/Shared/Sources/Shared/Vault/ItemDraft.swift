import FigLockFFI
import Foundation

/// One field of an item as the editor shows it.
public struct FieldSpec: Equatable, Sendable {
    public let name: String
    public let label: String
    public let isSecret: Bool
    public let isMultiline: Bool
    /// The password generator is offered next to this field.
    public let offersGenerator: Bool
    public let placeholder: String

    init(
        _ name: String, _ label: String, secret: Bool = false, multiline: Bool = false, generator: Bool = false,
        placeholder: String = ""
    ) {
        self.name = name
        self.label = label
        self.isSecret = secret
        self.isMultiline = multiline
        self.offersGenerator = generator
        self.placeholder = placeholder
    }
}

/// A field to write: `value == nil` removes the field.
public struct FieldChange: Equatable, Sendable {
    public let name: String
    public let value: String?
}

/// A file picked for a document item, already read and checked against the limits of ADR-0024.
public struct DraftFile: Identifiable, Equatable, Sendable {
    public let id: String
    public let name: String
    public let mime: String
    /// The contents; `nil` for a file the item already holds (it stays as it is unless removed).
    public let data: Data?

    public init(id: String, name: String, mime: String, data: Data?) {
        self.id = id
        self.name = name
        self.mime = mime
        self.data = data
    }
}

/// One row of a multi-valued part of an item: a website, an allowed host, an environment variable, a custom field.
public struct DraftEntry: Identifiable, Equatable, Sendable {
    public let id: String
    public var name: String
    public var value: String
    /// Not yet in the vault. For secrets an empty value of an existing entry means "keep what is stored".
    public let isNew: Bool

    public init(id: String, name: String = "", value: String = "", isNew: Bool = true) {
        self.id = id
        self.name = name
        self.value = value
        self.isNew = isNew
    }
}

/// What a person edits: a new item or the changes to an existing one. The editor works on this and never holds a
/// stored secret: an existing secret is shown as "unchanged" and only replaced when something is typed.
/// `changes()` turns the draft into the field writes the session takes (`VaultSession.addItem`, `setField`).
public struct ItemDraft: Equatable, Sendable {
    public static let maxFiles = 8
    /// Largest file, in bytes (`MAX_FILE_BYTES` in the core: base64 of it fills the 1 MiB value ceiling).
    public static let maxFileBytes = 768 * 1024

    public var kind: ItemKind
    public var title = ""
    public var notes = ""
    public var tagsText = ""
    /// The single-valued fields of the kind, by field name.
    public var values: [String: String] = [:]
    public var urls: [DraftEntry] = []
    public var allowedHosts: [DraftEntry] = []
    /// Environment variables: `name` is the variable name and `id` the same.
    public var env: [DraftEntry] = []
    public var custom: [DraftEntry] = []
    public var files: [DraftFile] = []
    /// Existing secret fields the person chose to remove.
    public var removedSecrets: Set<String> = []

    /// The item being edited; `nil` for a new one.
    public let original: VaultItem?

    public init(kind: ItemKind) {
        self.kind = kind
        self.original = nil
    }

    public init(editing item: VaultItem) {
        kind = item.kind
        original = item
        title = item.fields["title"] ?? ""
        notes = item.fields["notes"] ?? ""
        tagsText = item.tags.joined(separator: ", ")
        for spec in Self.fields(for: item.kind) where !spec.isSecret {
            values[spec.name] = item.fields[spec.name] ?? ""
        }
        urls = Self.entries(item.fields, prefix: "url/")
        allowedHosts = Self.entries(item.fields, prefix: "allowed_hosts/")
        env = item.secretFields.filter { $0.hasPrefix("env/") }.map {
            DraftEntry(id: String($0.dropFirst(4)), name: String($0.dropFirst(4)), isNew: false)
        }
        for name in item.secretFields where name.hasPrefix("custom/") && name.hasSuffix("/value") {
            let id = String(name.dropFirst(7).dropLast(6))
            custom.append(DraftEntry(id: id, name: item.fields["custom/\(id)/name"] ?? "", isNew: false))
        }
        custom.sort { $0.id < $1.id }
        files = item.files.map { DraftFile(id: $0.id, name: $0.name, mime: $0.mime, data: nil) }
    }

    private static func entries(_ fields: [String: String], prefix: String) -> [DraftEntry] {
        fields.filter { $0.key.hasPrefix(prefix) }.sorted { $0.key < $1.key }.map {
            DraftEntry(id: String($0.key.dropFirst(prefix.count)), value: $0.value, isNew: false)
        }
    }

    // MARK: Schema

    public var specs: [FieldSpec] { Self.fields(for: kind) }
    public var usesWebsites: Bool { kind == .login || kind == .document }
    public var usesAllowedHosts: Bool { kind == .token }
    public var usesEnv: Bool { kind == .env }
    public var usesFiles: Bool { kind == .document }
    public var isNew: Bool { original == nil }

    /// Whether the item already holds a value for this secret, so an empty field means "unchanged".
    public func hasStoredSecret(_ name: String) -> Bool {
        original?.secretFields.contains(name) ?? false
    }

    // MARK: Entries

    /// A new random id for a `url/<id>`-style field: lowercase letters and digits, as `schema.rs` requires.
    public static func newSuffix() -> String {
        let alphabet = Array("abcdefghijklmnopqrstuvwxyz0123456789")
        return String((0..<8).map { _ in alphabet.randomElement()! })
    }

    private func freshSuffix(avoiding taken: [String]) -> String {
        var id = Self.newSuffix()
        while taken.contains(id) { id = Self.newSuffix() }
        return id
    }

    public mutating func addURL() { urls.append(DraftEntry(id: freshSuffix(avoiding: urls.map(\.id)))) }
    public mutating func addAllowedHost() {
        allowedHosts.append(DraftEntry(id: freshSuffix(avoiding: allowedHosts.map(\.id))))
    }
    public mutating func addEnv() { env.append(DraftEntry(id: UUID().uuidString)) }
    public mutating func addCustom() { custom.append(DraftEntry(id: freshSuffix(avoiding: custom.map(\.id)))) }

    /// Add a picked file. Fails when the document already holds the most files it may, or the file is too big.
    public mutating func attach(_ file: DraftFile) throws {
        guard files.count < Self.maxFiles else { throw ItemDraftError.tooManyFiles }
        guard (file.data?.count ?? 0) <= Self.maxFileBytes else { throw ItemDraftError.fileTooLarge(file.name) }
        files.append(file)
    }

    /// A fresh id for a file of this draft.
    public func newFileId() -> String { freshSuffix(avoiding: files.map(\.id)) }

    // MARK: Checks

    public static let tagCharacters = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyz0123456789-_")

    /// The tags typed, lowercase, split at commas, spaces and `#`. Each is at most 40 characters of letters,
    /// digits, `-` and `_` (`schema.rs`).
    public var tags: [String] {
        var seen = Set<String>()
        return tagsText.lowercased().split(whereSeparator: { ",; #".contains($0) }).map(String.init).filter {
            seen.insert($0).inserted
        }
    }

    static func isEnvName(_ name: String) -> Bool {
        guard let first = name.unicodeScalars.first, name.count <= 64 else { return false }
        let upper = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZ_")
        let rest = upper.union(CharacterSet(charactersIn: "0123456789"))
        return upper.contains(first) && name.unicodeScalars.allSatisfy(rest.contains)
    }

    /// The first thing that stops the draft being saved, in words for the person, or `nil` when it can be saved.
    public var problem: String? {
        if title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return "Give the item a name." }
        for tag in tags where tag.count > 40 || !tag.unicodeScalars.allSatisfy(Self.tagCharacters.contains) {
            return "Tags use letters, digits, - and _ only."
        }
        if usesEnv {
            var seen = Set<String>()
            for entry in env {
                let name = entry.name.trimmingCharacters(in: .whitespaces)
                if !Self.isEnvName(name) { return "Variable names use capital letters, digits and _, like API_KEY." }
                if !seen.insert(name).inserted { return "\(name) appears twice." }
                if entry.value.isEmpty && entry.isNew { return "Give \(name) a value." }
            }
        }
        for entry in custom
        where entry.name.trimmingCharacters(in: .whitespaces).isEmpty && !entry.value.isEmpty {
            return "Name the custom field."
        }
        if files.count > Self.maxFiles { return "A document holds at most \(Self.maxFiles) files." }
        return nil
    }

    // MARK: Changes

    /// The writes that turn the original (or an empty item) into this draft. Unchanged fields are left out.
    public func changes() -> [FieldChange] {
        let old = original?.fields ?? [:]
        var out = scalarChanges()
        out += listChanges(prefix: "url/", entries: urls, oldNames: old.keys.filter { $0.hasPrefix("url/") })
        out += listChanges(
            prefix: "allowed_hosts/", entries: allowedHosts,
            oldNames: old.keys.filter { $0.hasPrefix("allowed_hosts/") })
        out += tagChanges()
        if usesEnv { out += envChanges() }
        out += customChanges()
        if usesFiles { out += fileChanges() }
        return out
    }

    /// A text field: written when it differs from the stored one, removed when emptied.
    private func plain(_ name: String, _ value: String) -> [FieldChange] {
        let new = value.trimmingCharacters(in: .whitespacesAndNewlines)
        if new == (original?.fields[name] ?? "") { return [] }
        return [FieldChange(name: name, value: new.isEmpty ? nil : new)]
    }

    /// A secret: written when something is typed, removed only when the person chose to, otherwise left alone.
    private func secret(_ name: String, _ value: String) -> [FieldChange] {
        if !value.isEmpty { return [FieldChange(name: name, value: value)] }
        if removedSecrets.contains(name) && hasStoredSecret(name) { return [FieldChange(name: name, value: nil)] }
        return []
    }

    private func scalarChanges() -> [FieldChange] {
        var out = plain("title", title) + plain("notes", notes)
        for spec in specs {
            let value = values[spec.name] ?? ""
            if spec.isSecret {
                out += secret(spec.name, value)
            } else if spec.name == "body" {
                // A note keeps its line breaks and spaces as typed; only a fully empty body counts as empty.
                if value != (original?.fields[spec.name] ?? "") {
                    out.append(FieldChange(name: spec.name, value: value.isEmpty ? nil : value))
                }
            } else {
                out += plain(spec.name, value)
            }
        }
        return out
    }

    private func tagChanges() -> [FieldChange] {
        let old = original?.fields ?? [:]
        let wanted = Set(tags.map { "tags/\($0)" })
        var out = wanted.sorted().filter { old[$0] == nil }.map { FieldChange(name: $0, value: "1") }
        out += old.keys.sorted().filter { $0.hasPrefix("tags/") && !wanted.contains($0) }.map {
            FieldChange(name: $0, value: nil)
        }
        return out
    }

    private func envChanges() -> [FieldChange] {
        var out: [FieldChange] = []
        let kept = Set(env.map { "env/\($0.name.trimmingCharacters(in: .whitespaces))" })
        for entry in env { out += secret("env/" + entry.name.trimmingCharacters(in: .whitespaces), entry.value) }
        for name in original?.secretFields ?? [] where name.hasPrefix("env/") && !kept.contains(name) {
            out.append(FieldChange(name: name, value: nil))
        }
        return out
    }

    /// Custom fields: a name (plain) and a value (secret) per id.
    private func customChanges() -> [FieldChange] {
        var out: [FieldChange] = []
        let ids = Set(custom.map(\.id))
        for entry in custom {
            let name = entry.name.trimmingCharacters(in: .whitespaces)
            if name.isEmpty && entry.value.isEmpty { continue }
            out += plain("custom/\(entry.id)/name", name)
            out += secret("custom/\(entry.id)/value", entry.value)
        }
        var oldIds = Set<String>()
        for name in (original?.fields.keys.map { $0 } ?? []) + (original?.secretFields ?? [])
        where name.hasPrefix("custom/") {
            oldIds.insert(String(name.dropFirst(7).split(separator: "/").first ?? ""))
        }
        for id in oldIds.sorted() where !ids.contains(id) {
            out += removal(of: ["custom/\(id)/name", "custom/\(id)/value"])
        }
        return out
    }

    /// Files: new ones are written, existing ones the person dropped are removed.
    private func fileChanges() -> [FieldChange] {
        var out: [FieldChange] = []
        let kept = Set(files.map(\.id))
        for file in files {
            guard let data = file.data else { continue }
            out.append(FieldChange(name: "file/\(file.id)/name", value: file.name))
            out.append(FieldChange(name: "file/\(file.id)/mime", value: file.mime))
            out.append(FieldChange(name: "file/\(file.id)/data", value: data.base64EncodedString()))
        }
        for stored in original?.files ?? [] where !kept.contains(stored.id) {
            out += removal(of: ["file/\(stored.id)/name", "file/\(stored.id)/mime", "file/\(stored.id)/data"])
        }
        return out
    }

    private func removal(of names: [String]) -> [FieldChange] {
        let known = Set(original?.fields.keys.map { $0 } ?? []).union(original?.secretFields ?? [])
        return names.filter(known.contains).map { FieldChange(name: $0, value: nil) }
    }

    private func listChanges(prefix: String, entries: [DraftEntry], oldNames: [String]) -> [FieldChange] {
        var out: [FieldChange] = []
        let old = original?.fields ?? [:]
        var live = Set<String>()
        for entry in entries {
            let value = entry.value.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !value.isEmpty else { continue }
            let name = prefix + entry.id
            live.insert(name)
            if old[name] != value { out.append(FieldChange(name: name, value: value)) }
        }
        for name in oldNames.sorted() where !live.contains(name) { out.append(FieldChange(name: name, value: nil)) }
        return out
    }

    /// The fields to give `VaultSession.addItem` for a new item.
    public func newItemFields() -> [FieldInput] {
        changes().compactMap { change in change.value.map { FieldInput(name: change.name, value: $0) } }
    }
}

extension ItemDraft {
    /// The single-valued fields of each kind, in the order the editor shows them (`schema.rs`).
    public static func fields(for kind: ItemKind) -> [FieldSpec] {
        switch kind {
        case .login:
            return [
                FieldSpec("username", "Username"),
                FieldSpec("password", "Password", secret: true, generator: true),
                FieldSpec("totp", "One-time code secret", secret: true),
            ]
        case .token:
            return [
                FieldSpec("provider", "Provider", placeholder: "github"),
                FieldSpec("token", "Token", secret: true),
                FieldSpec("env_var", "Environment variable", placeholder: "GITHUB_TOKEN"),
                FieldSpec("scopes", "Scopes"),
                FieldSpec("expires_at", "Expires", placeholder: "2027-01-31"),
            ]
        case .note:
            return [FieldSpec("body", "Note", multiline: true)]
        case .env:
            return []
        case .ssh:
            return [
                FieldSpec("private_key", "Private key", secret: true, multiline: true),
                FieldSpec("public_key", "Public key", multiline: true),
                FieldSpec("passphrase", "Passphrase", secret: true),
            ]
        case .document:
            return [
                FieldSpec("password", "Password", secret: true, generator: true),
                FieldSpec("expires_at", "Expires", placeholder: "2027-01-31"),
            ]
        }
    }
}

public enum ItemDraftError: Error, Equatable {
    case tooManyFiles
    case fileTooLarge(String)
    case unreadableFile(String)

    public var message: String {
        switch self {
        case .tooManyFiles: "A document holds at most \(ItemDraft.maxFiles) files."
        case .fileTooLarge(let name): "\(name) is larger than 768 KB, the most a file in FigLock can be."
        case .unreadableFile(let name): "FigLock could not read \(name)."
        }
    }
}

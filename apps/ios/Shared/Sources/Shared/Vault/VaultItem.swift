import FigLockFFI
import Foundation

/// What kind of item a row is. Names, icons and the secret label follow the iPhone prototype.
public enum ItemKind: String, Sendable, CaseIterable {
    case login
    case token
    case note
    case env
    case ssh
    case document

    public init(wire: String) {
        switch wire {
        case "login": self = .login
        case "token": self = .token
        case "note": self = .note
        case "env": self = .env
        case "ssh": self = .ssh
        default: self = .document
        }
    }

    public var displayName: String {
        switch self {
        case .login: "Login"
        case .token: "API token"
        case .note: "Note"
        case .env: "Environment"
        case .ssh: "SSH key"
        case .document: "Document"
        }
    }

    public var symbol: String {
        switch self {
        case .login: "globe"
        case .token: "key.fill"
        case .note: "note.text"
        case .env: "terminal"
        case .ssh: "lock.laptopcomputer"
        case .document: "doc.fill"
        }
    }
}

/// One item as the list and detail screens show it: the non-secret fields and the names of the secret ones. A
/// secret value is never in here; it comes out of the session through `SecretReveal`.
public struct VaultItem: Identifiable, Equatable, Sendable {
    public let id: String
    public let kind: ItemKind
    public let updated: Date
    public let fields: [String: String]
    public let secretFields: [String]

    public init(id: String, kind: ItemKind, updated: Date, fields: [String: String], secretFields: [String]) {
        self.id = id
        self.kind = kind
        self.updated = updated
        self.fields = fields
        self.secretFields = secretFields
    }

    init(_ summary: ItemSummary) {
        self.init(
            id: summary.id, kind: ItemKind(wire: summary.itemType),
            updated: Date(timeIntervalSince1970: Double(summary.updatedMs) / 1000), fields: summary.fields,
            secretFields: summary.secretFields.sorted())
    }

    public var title: String {
        let title = fields["title"] ?? ""
        return title.isEmpty ? "Untitled" : title
    }

    /// The second line of a row: the username of a login, the provider of a token, otherwise the website or type.
    public var subtitle: String {
        for name in ["username", "provider"] {
            if let value = fields[name], !value.isEmpty { return value }
        }
        if let host = websites.first { return host }
        return kind.displayName
    }

    /// The urls of the item, in the order of their field ids.
    public var websites: [String] {
        fields.filter { $0.key.hasPrefix("url/") }.sorted { $0.key < $1.key }.map(\.value)
    }

    public var tags: [String] {
        fields.filter { $0.key.hasPrefix("tags/") }.map { String($0.key.dropFirst(5)) }.sorted()
    }

    /// The secret shown first, named for people ("Password", "Token").
    public static func label(forSecret name: String) -> String {
        switch name {
        case "password": return "Password"
        case "totp": return "One-time code secret"
        case "token": return "Token"
        case "private_key": return "Private key"
        case "passphrase": return "Passphrase"
        default:
            if name.hasPrefix("env/") { return String(name.dropFirst(4)) }
            if name.hasPrefix("file/") { return "File" }
            if name.hasPrefix("custom/") { return "Secret" }
            return name
        }
    }

    /// The non-secret fields worth a row of their own on the detail screen, labelled for people, in a fixed order.
    public var detailRows: [(label: String, value: String)] {
        var rows: [(String, String)] = []
        func add(_ label: String, _ name: String) {
            if let value = fields[name], !value.isEmpty { rows.append((label, value)) }
        }
        add("Username", "username")
        add("Provider", "provider")
        add("Environment variable", "env_var")
        for (index, url) in websites.enumerated() {
            rows.append((index == 0 ? "Website" : "Website \(index + 1)", url))
        }
        add("Scopes", "scopes")
        for (name, value) in fields.sorted(by: { $0.key < $1.key }) where name.hasPrefix("allowed_hosts/") {
            rows.append(("Allowed host", value))
        }
        add("Expires", "expires_at")
        add("Public key", "public_key")
        add("Notes", "notes")
        add("Note", "body")
        return rows
    }
}

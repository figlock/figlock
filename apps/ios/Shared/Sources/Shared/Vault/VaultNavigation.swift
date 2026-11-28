import Foundation
import Observation

/// Which items the list shows: everything, one kind, one tag, the kinds the chips call "Other", or the trash.
public enum VaultScope: Hashable, Sendable {
    case all
    case kind(ItemKind)
    /// Every kind but logins and notes: the iPhone "Other" chip.
    case otherKinds
    case tag(String)
    case deleted

    public func matches(_ item: VaultItem) -> Bool {
        switch self {
        case .all, .deleted: true
        case .kind(let kind): item.kind == kind
        case .otherKinds: item.kind != .login && item.kind != .note
        case .tag(let tag): item.tags.contains(tag)
        }
    }

    /// The scope as the compact layout can show it: its chips have no trash and no single tag.
    public var forCompact: VaultScope { self == .deleted ? .all : self }

    /// The iPhone chips, in order.
    public static let chips: [(title: String, scope: VaultScope)] = [
        ("All", .all), ("Logins", .kind(.login)), ("Notes", .kind(.note)), ("Other", .otherKinds),
    ]

    public var title: String {
        switch self {
        case .all: "All items"
        case .kind(let kind): kind.pluralName
        case .otherKinds: "Other"
        case .tag(let tag): tag
        case .deleted: "Recently deleted"
        }
    }
}

/// What one window is looking at: the sidebar choice, the search text, the selected item and the list's scroll anchor
/// (ADR-0048 decision 2). It belongs to the window, not to the view, so the regular layout and the compact one read
/// and write the same state: a window that goes narrow and wide again, or a rotation, keeps its selection. Nothing
/// here is written to disk, so a relaunch starts at All with nothing selected and no item title leaves the vault.
@MainActor
@Observable
public final class VaultNavigation {
    public var scope: VaultScope = .all {
        // A different list: what was selected in the old one may not be in it, and an old anchor means nothing.
        didSet { if scope != oldValue { scrollAnchor = nil } }
    }
    public var query = ""
    public var selectedItemId: String?
    /// The id of the row at the top of the list.
    public var scrollAnchor: String?

    public init() {}

    public var isSearching: Bool { !query.trimmingCharacters(in: .whitespaces).isEmpty }

    /// Drop the selection when the item is gone (deleted elsewhere, or the vault was reloaded without it).
    public func clearSelectionIfMissing(in browser: VaultBrowser) {
        if let id = selectedItemId, browser.item(id: id) == nil { selectedItemId = nil }
    }
}

extension ItemKind {
    /// "Logins", "API tokens": the sidebar row for the kind.
    public var pluralName: String {
        switch self {
        case .login: "Logins"
        case .token: "Tokens"
        case .note: "Notes"
        case .env: "Environments"
        case .ssh: "SSH keys"
        case .document: "Documents"
        }
    }
}

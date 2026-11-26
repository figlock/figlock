import Shared
import SwiftUI

/// What the extension shows: a short wait, a message (open the app, try again), or the list of matching logins.
struct AutofillScreen: View {
    enum Message {
        /// Face ID cannot be used here. `why` says which rule ended it, so a wrong 'open the app' is not a mystery.
        case openApp(why: String, then: String)
        case retry(Int)
        case failed
        case storage

        var title: String {
            switch self {
            case .openApp: "Open FigLock to unlock"
            case .retry: "Face ID did not recognise you"
            case .failed: "FigLock could not open the vault"
            case .storage: "FigLock cannot reach its storage"
            }
        }

        var detail: String {
            switch self {
            case .openApp(let why, let then): "\(why) \(then)"
            case .retry(let left):
                "You have \(left) more \(left == 1 ? "try" : "tries") before the master password is needed."
            case .failed: "Try again. If this stays, open FigLock."
            case .storage: "Open FigLock, or reinstall it if this stays."
            }
        }
    }

    enum Content {
        case working(String)
        case message(Message)
        case list(CredentialListModel)
    }

    let content: Content
    let cancel: () -> Void

    var body: some View {
        NavigationStack {
            Group {
                switch content {
                case .working(let text):
                    ProgressView(text).frame(maxWidth: .infinity, maxHeight: .infinity)
                case .message(let message):
                    ContentUnavailableView(message.title, systemImage: "lock", description: Text(message.detail))
                case .list(let model):
                    CredentialListView(model: model)
                }
            }
            .navigationTitle("FigLock")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel", action: cancel) }
            }
        }
    }
}

@MainActor
@Observable
final class CredentialListModel {
    let vault: AutofillVault
    let identifiers: [String]
    let matches: [VaultItem]
    let pick: (VaultItem) -> Void
    let cancel: () -> Void
    var query = ""

    init(
        vault: AutofillVault, initial: [VaultItem], identifiers: [String], pick: @escaping (VaultItem) -> Void,
        cancel: @escaping () -> Void
    ) {
        self.vault = vault
        self.matches = initial
        self.identifiers = identifiers
        self.pick = pick
        self.cancel = cancel
    }

    /// The matching logins until the person types, then a search of every login.
    var visible: [VaultItem] {
        query.trimmingCharacters(in: .whitespaces).isEmpty ? matches : vault.search(query)
    }
}

struct CredentialListView: View {
    @Bindable var model: CredentialListModel

    var body: some View {
        List {
            if model.visible.isEmpty {
                ContentUnavailableView.search(text: model.query)
            }
            ForEach(model.visible) { item in
                Button {
                    model.pick(item)
                } label: {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(item.title).font(.body)
                        Text(item.subtitle).font(.footnote).foregroundStyle(.secondary)
                    }
                }
                .buttonStyle(.plain)
            }
        }
        .searchable(text: $model.query, placement: .navigationBarDrawer(displayMode: .always))
        .textInputAutocapitalization(.never)
        .autocorrectionDisabled()
    }
}

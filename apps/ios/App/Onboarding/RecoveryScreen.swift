import Shared
import SwiftUI
import UniformTypeIdentifiers

/// Shown when this iPhone had a vault and its folder is gone (ADR-0026). The remote copy is never used as a silent
/// substitute: the person picks. On iOS the vault folder is fixed in the app's container, so there is no
/// "locate the folder" option as on the desktop.
struct RecoveryScreen: View {
    let service: Onboarding
    let drive: DriveConnection
    let lock: AppLock

    @State private var confirming: Action?
    @State private var errorText: String?
    @State private var busy = false
    @State private var picking = false

    private enum Action: Identifiable {
        case restore, startOver, remove
        var id: Self { self }
    }

    var body: some View {
        ScrollView {
            VStack(spacing: FLSpace.s4) {
                Image(systemName: "folder.badge.questionmark").font(.system(size: 44))
                    .foregroundStyle(FLColor.warning).accessibilityHidden(true)
                Text("Can't find your vault")
                    .font(FLFont.ui(FLText.xl2, weight: FLWeight.bold, relativeTo: .title2)).foregroundStyle(
                        FLColor.text
                    )
                    .accessibilityAddTraits(.isHeader)
                Text("The vault on this iPhone was removed, so FigLock is paused. Choose how to continue.")
                    .font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary).multilineTextAlignment(.center)
                if let errorText { Notice(text: errorText, isError: true) }
                if busy { ProgressView() }
                Button {
                    confirming = .restore
                } label: {
                    Label("Restore from Google Drive", systemImage: "cloud")
                }
                .buttonStyle(PrimaryActionStyle()).disabled(busy)
                Button {
                    picking = true
                } label: {
                    Label("Restore from an iCloud Drive folder", systemImage: "icloud")
                }
                .buttonStyle(SecondaryActionStyle()).disabled(busy)
                Button {
                    confirming = .startOver
                } label: {
                    Label("Create a new vault", systemImage: "plus")
                }
                .buttonStyle(SecondaryActionStyle()).disabled(busy)
                Button("Remove this vault from this iPhone") { confirming = .remove }
                    .buttonStyle(PlainActionStyle()).disabled(busy)
                Text(
                    "Restore downloads the copy in Google Drive or in the iCloud Drive folder you pick. Create starts empty with a new Secret Key. Nothing in either is deleted."
                )
                .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textTertiary)
            }
            .padding(FLSpace.s4).padding(.top, FLSpace.s8)
        }
        .background(FLColor.bg.ignoresSafeArea())
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            Task { await restore(from: result) }
        }
        .alert(title, isPresented: Binding(get: { confirming != nil }, set: { if !$0 { confirming = nil } })) {
            Button("Cancel", role: .cancel) {}
            Button(confirmLabel, role: confirming == .restore ? nil : .destructive) {
                if let action = confirming { Task { await perform(action) } }
            }
        } message: {
            Text(message)
        }
    }

    private var title: String {
        switch confirming {
        case .restore: "Restore from Google Drive?"
        case .startOver: "Create a new vault?"
        case .remove: "Remove this vault from this iPhone?"
        case nil: ""
        }
    }

    private var confirmLabel: String {
        switch confirming {
        case .restore: "Restore"
        case .startOver: "Create new vault"
        default: "Remove"
        }
    }

    private var message: String {
        switch confirming {
        case .restore:
            "FigLock downloads the vault from Google Drive to this iPhone. Changes made here since the last sync won't be kept."
        case .startOver:
            "It starts empty with a new Secret Key. The old keys on this iPhone are forgotten. The copy in Google Drive stays."
        case .remove:
            "This forgets the vault, its keys and Face ID on this iPhone. The copy in Google Drive and your Emergency Kit are not touched."
        case nil: ""
        }
    }

    /// Restore from the folder the person picked. Nothing is created in it: it has to hold the vault already.
    private func restore(from result: Result<URL, Error>) async {
        guard case .success(let url) = result else { return }
        busy = true
        errorText = nil
        defer { busy = false }
        do {
            let opened = try FolderRemote.open(url: url, localVault: service.vaultFolder)
            defer { opened.close() }
            let entry = RemoteEntry.folder(name: url.lastPathComponent, bookmark: try FolderRemote.bookmark(for: url))
            try await service.openFromRemote(opened.store, entry: entry)
            lock.vaultDidChange()
        } catch let failure as Onboarding.Failure {
            errorText = OnboardingModel.text(failure, source: .icloud)
        } catch let problem as FolderProblem {
            errorText = SyncSettingsView.text(problem)
        } catch {
            errorText = "Something went wrong. Try again."
        }
    }

    private func perform(_ action: Action) async {
        busy = true
        errorText = nil
        defer { busy = false }
        do {
            switch action {
            case .restore:
                if !drive.isSignedIn { try await drive.signIn() }
                try await service.openFromRemote(try drive.remote())
            case .startOver, .remove:
                try service.removeFromThisPhone()
            }
            lock.vaultDidChange()
        } catch let failure as Onboarding.Failure {
            errorText = OnboardingModel.text(failure)
        } catch let failure as SignInError {
            errorText = OnboardingModel.text(failure)
        } catch {
            errorText = "Something went wrong. Try again."
        }
    }
}

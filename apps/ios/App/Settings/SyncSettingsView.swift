import Shared
import SwiftUI
import UniformTypeIdentifiers

/// Sync settings (ADR-0043): one row per remote, each with its own status, plus adding an iCloud Drive folder or
/// Google Drive. A failing remote never changes another's row. Signing out and removing a folder never delete
/// anything in the remote; the copy in Google Drive goes only after the typed phrase of ADR-0035.
struct SyncSettingsView: View {
    let context: SettingsContext

    @State private var picking = false
    @State private var notice: String?
    @State private var errorText: String?
    @State private var busy = false

    private var sync: SyncModel { context.sync }

    var body: some View {
        List {
            if let notice {
                Notice(text: notice, isError: false).listRowBackground(Color.clear).listRowInsets(EdgeInsets())
            }
            if let errorText {
                Notice(text: errorText, isError: true).listRowBackground(Color.clear).listRowInsets(EdgeInsets())
            }
            Section {
                if sync.rows.isEmpty {
                    Text("This vault is only on this iPhone. Add a place to keep an encrypted copy.")
                        .font(FLFont.ui(FLText.base, relativeTo: .callout)).foregroundStyle(FLColor.textSecondary)
                }
                ForEach(sync.rows) { row in
                    NavigationLink {
                        RemoteDetailView(context: context, id: row.id)
                    } label: {
                        RemoteRow(row: row, account: row.entry.kind == .drive ? context.drive.accountEmail : nil)
                    }
                }
            } header: {
                FLSectionHeader("Where your vault is kept")
            } footer: {
                Text("Each place syncs on its own. If one fails, the others keep working.")
            }
            .flTileRows()

            Section {
                Button {
                    picking = true
                } label: {
                    Label("Add an iCloud Drive folder", systemImage: "icloud").rowLabel()
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                }
                .disabled(busy)
                if !context.drive.isSignedIn || !sync.rows.contains(where: { $0.entry.kind == .drive }) {
                    Button {
                        Task { await signInToDrive() }
                    } label: {
                        Label(
                            context.drive.isSignedIn ? "Sync with Google Drive" : "Sign in to Google Drive",
                            systemImage: "globe"
                        ).rowLabel()
                            .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                    }
                    .disabled(busy || !context.drive.isConfigured)
                }
            } footer: {
                VStack(alignment: .leading, spacing: FLSpace.s2) {
                    Text(
                        "Pick an empty folder in iCloud Drive, or make one with New Folder in the picker. FigLock only reads and writes inside the folder you pick."
                    )
                    if !FolderRemote.iCloudIsOn {
                        Text(
                            "iCloud Drive looks off on this iPhone. Turn it on in Settings, under your name, then iCloud."
                        )
                    }
                    if !context.drive.isSignedIn && !context.drive.isConfigured {
                        Text("Google sign-in is not set up in this build.")
                    }
                }
            }
            .flTileRows()
        }
        .flTileList()
        .navigationTitle("Sync")
        .navigationBarTitleDisplayMode(.inline)
        .refreshable { await sync.syncNow(force: true) }
        .task {
            await sync.reload()
            await context.drive.refreshAccountEmail()
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            Task { await added(result) }
        }
    }

    private func added(_ result: Result<URL, Error>) async {
        errorText = nil
        notice = nil
        guard case .success(let url) = result else { return }
        busy = true
        defer { busy = false }
        do {
            try await sync.addFolder(url)
            notice = "Syncing with \(FolderRemote.displayName(for: url))."
        } catch let problem as FolderProblem {
            errorText = Self.text(problem)
        } catch {
            errorText = "FigLock could not use that folder."
        }
    }

    private func signInToDrive() async {
        errorText = nil
        busy = true
        defer { busy = false }
        do {
            if !context.drive.isSignedIn { try await context.drive.signIn() }
            await sync.driveSignedIn()
        } catch let failure as SignInError {
            errorText = OnboardingModel.text(failure)
        } catch {
            errorText = "Something went wrong. Try again."
        }
    }

    static func text(_ problem: FolderProblem) -> String {
        switch problem {
        case .notEmpty:
            "That folder already has other files in it. Pick an empty folder, or make a new one, so your vault stays apart from them."
        case .differentVault: "That folder holds a different vault than this iPhone."
        case .insideVault: "That folder overlaps the vault on this iPhone. Pick another one."
        case .alreadyRemote: "FigLock already syncs with that folder, or one inside or around it. Pick a different one."
        case .iCloudOff: "iCloud Drive is off on this iPhone. Turn it on in Settings, under your name, then iCloud."
        case .missing: "That folder is not there any more."
        case .pickAgain: "FigLock could not open that folder. Pick it again."
        case .noVault: "This iPhone has no vault yet."
        case .denied: "The Files app did not let FigLock read that folder."
        }
    }
}

/// One remote: its state in words, sync now, and what can be done about it.
private struct RemoteDetailView: View {
    let context: SettingsContext
    let id: String

    @Environment(\.dismiss) private var dismiss
    @State private var confirmingSignOut = false
    @State private var confirmingRemoval = false
    @State private var confirmingUpload = false
    @State private var deleting = false
    @State private var picking = false
    @State private var syncingNow = false
    @State private var notice: String?
    @State private var errorText: String?

    private var sync: SyncModel { context.sync }

    var body: some View {
        List {
            if let row = sync.row(id: id) {
                content(for: row)
            }
        }
        .flTileList()
        .navigationTitle(sync.row(id: id)?.entry.name ?? "Sync")
        .navigationBarTitleDisplayMode(.inline)
        .task {
            await sync.reload()
            await context.drive.refreshAccountEmail()
        }
        .onChange(of: sync.row(id: id) == nil) { _, gone in if gone { dismiss() } }
        .alert("Sign out of Google Drive?", isPresented: $confirmingSignOut) {
            Button("Cancel", role: .cancel) {}
            Button("Sign out", role: .destructive) { Task { await sync.driveSignedOut() } }
        } message: {
            Text(
                "\(signOutAccountLine)This iPhone stops syncing with Google Drive until you sign in again. Your vault stays on this iPhone, other places keep syncing, and the copy in Google Drive is not deleted."
            )
        }
        .alert("Stop syncing with this folder?", isPresented: $confirmingRemoval) {
            Button("Cancel", role: .cancel) {}
            Button("Stop syncing", role: .destructive) { Task { await sync.remove(id) } }
        } message: {
            Text(
                "FigLock stops using this folder. The vault stays on this iPhone, and the files in the folder are not deleted."
            )
        }
        .alert("Upload this iPhone's copy again?", isPresented: $confirmingUpload) {
            Button("Cancel", role: .cancel) {}
            Button("Upload") { Task { await sync.sync(id, confirmReupload: true) } }
        } message: {
            Text(
                "The copy that was there is gone. This uploads the vault on this iPhone to it again. Your other devices are not changed."
            )
        }
        .sheet(isPresented: $deleting) {
            DeleteFromDriveSheet(context: context) { notice = $0 }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            Task { await picked(result) }
        }
    }

    /// "Signed in as x. " so the confirmation names the account being left.
    private var signOutAccountLine: String {
        context.drive.accountEmail.map { "Signed in as \($0). " } ?? ""
    }

    @ViewBuilder
    private func content(for row: SyncModel.Row) -> some View {
        statusSection(row)
        actionSection(row)
        stopSection(row)
    }

    @ViewBuilder
    private func statusSection(_ row: SyncModel.Row) -> some View {
        let state = row.state
        if notice != nil || errorText != nil {
            Section {
                if let notice {
                    Notice(text: notice, isError: false).listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                }
                if let errorText {
                    Notice(text: errorText, isError: true).listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
                        .listRowSeparator(.hidden)
                }
            }
        }
        Section {
            HStack(alignment: .top, spacing: FLSpace.s3) {
                Image(systemName: state.symbol).foregroundStyle(state.tone.color).accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(state.title).font(FLFont.ui(FLText.md, weight: FLWeight.semibold)).foregroundStyle(
                        FLColor.text)
                    Text(state.detail(for: row.entry)).font(FLFont.ui(FLText.base, relativeTo: .callout))
                        .foregroundStyle(FLColor.textSecondary)
                    if row.entry.kind == .drive, let email = context.drive.accountEmail {
                        Text(email).font(FLFont.ui(FLText.base, relativeTo: .callout))
                            .foregroundStyle(FLColor.textSecondary).textSelection(.enabled)
                            .accessibilityIdentifier("drive-account-email-detail")
                    }
                }
            }
            .padding(FLSpace.s1)
            .listRowBackground(state.tone.background)
            .accessibilityElement(children: .combine)
        }
    }

    @ViewBuilder
    private func actionSection(_ row: SyncModel.Row) -> some View {
        let state = row.state
        Section {
            Button {
                Task { await syncNow() }
            } label: {
                Label {
                    Text(syncingNow ? "Syncing" : "Sync now")
                } icon: {
                    if syncingNow {
                        ProgressView()
                    } else {
                        Image(systemName: "arrow.triangle.2.circlepath")
                    }
                }
                .rowLabel()
                .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
            }
            .disabled(syncingNow || state.phase == .syncing)
            if state.phase == .remoteLost {
                Button {
                    confirmingUpload = true
                } label: {
                    Label("Upload this iPhone's copy again", systemImage: "icloud.and.arrow.up").rowLabel()
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                }
            }
            if row.entry.kind == .folder, case .unavailable = state.phase {
                Button {
                    picking = true
                } label: {
                    Label("Pick the folder again", systemImage: "folder").rowLabel()
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                }
            }
        }
        .flTileRows()
    }

    @ViewBuilder
    private func stopSection(_ row: SyncModel.Row) -> some View {
        Section {
            if row.entry.kind == .drive {
                Button(role: .destructive) {
                    confirmingSignOut = true
                } label: {
                    Label("Sign out of Google Drive", systemImage: "rectangle.portrait.and.arrow.right").rowLabel(
                        destructive: true
                    )
                    .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                }
                .tint(FLColor.danger)
                Button(role: .destructive) {
                    deleting = true
                } label: {
                    Label("Delete from Google Drive", systemImage: "trash").rowLabel(destructive: true)
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                }
                .tint(FLColor.danger)
            } else {
                Button(role: .destructive) {
                    confirmingRemoval = true
                } label: {
                    Label("Stop syncing with this folder", systemImage: "minus.circle").rowLabel(destructive: true)
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                }
                .tint(FLColor.danger)
            }
        } footer: {
            Text(
                row.entry.kind == .drive
                    ? "FigLock uploads only the encrypted vault, to its own hidden folder in your Drive. Signing out never deletes it."
                    : "Only the encrypted vault is written to this folder. Stopping never deletes its files."
            )
        }
        .flTileRows()
    }

    /// Run a sync the person asked for and say how it went, including when the answer is the same problem as
    /// before, so a tap never looks like nothing happened.
    private func syncNow() async {
        notice = nil
        errorText = nil
        syncingNow = true
        defer { syncingNow = false }
        // Keep "Syncing" up long enough to be seen when the check fails at once.
        async let shown: Void = { try? await Task.sleep(for: .milliseconds(700)) }()
        await sync.sync(id)
        await shown
        guard let row = sync.row(id: id) else { return }
        let state = row.state
        switch state.phase {
        case .synced:
            notice = "Synced just now."
        case .idle where state.lastSyncMs != nil:
            notice = "Synced just now."
        case .offline, .waitingForDownload:
            errorText = "\(state.title). \(state.detail(for: row.entry))"
        default:
            errorText = "Sync did not work. \(state.title)."
        }
    }

    private func picked(_ result: Result<URL, Error>) async {
        errorText = nil
        guard case .success(let url) = result else { return }
        do {
            try await sync.replaceFolder(id, with: url)
            notice = "Syncing with \(FolderRemote.displayName(for: url))."
        } catch let problem as FolderProblem {
            errorText = SyncSettingsView.text(problem)
        } catch {
            errorText = "FigLock could not use that folder."
        }
    }
}

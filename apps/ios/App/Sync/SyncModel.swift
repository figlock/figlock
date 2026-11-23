import FigLockFFI
import Shared
import SwiftUI

/// What the screens know about syncing: one row per remote, each with its own state (ADR-0043), and the actions
/// that add, remove and run them. The work itself is `RemoteSyncCoordinator` in `Shared`.
@MainActor
@Observable
final class SyncModel {
    struct Row: Identifiable, Equatable {
        let entry: RemoteEntry
        var state: RemoteState
        var id: String { entry.id }
    }

    private(set) var rows: [Row] = []
    /// Counts syncs that changed the local vault, so the vault list reloads.
    private(set) var changeCount = 0
    private(set) var isSyncing = false

    let list: RemoteList
    let drive: DriveConnection
    private let vault: LocalVault
    private let coordinator: RemoteSyncCoordinator
    private var loop: Task<Void, Never>?

    init(
        vault: LocalVault, keeper: SecretKeyKeeper, drive: DriveConnection
    ) {
        self.vault = vault
        self.drive = drive
        let list = RemoteList(container: vault.container)
        self.list = list
        list.reconcileDrive(signedIn: drive.isSignedIn)
        coordinator = RemoteSyncCoordinator(
            .init(
                vault: vault, list: list, drive: { [drive] in drive.remoteIfSignedIn() },
                deviceId: {
                    guard case .ready(let id, _) = vault.status() else { return nil }
                    return (try? keeper.deviceKeys(vaultId: id))?.deviceId()
                }))
        Task { await reload() }
    }

    // MARK: Reading

    func reload() async {
        var next: [Row] = []
        for entry in list.entries() { next.append(Row(entry: entry, state: await coordinator.state(of: entry))) }
        if next != rows { rows = next }
        isSyncing = next.contains { $0.state.phase == .syncing }
    }

    func row(id: String) -> Row? { rows.first { $0.id == id } }

    /// The one line for the vault list and the Settings value.
    var summary: String {
        guard !rows.isEmpty else { return "This iPhone only" }
        return rows.map(\.entry.name).joined(separator: ", ")
    }

    var attentionRows: [Row] { rows.filter { $0.state.needsAttention } }

    // MARK: Running

    /// Sync every remote that is due. A person asking for it (`force`) skips the back-off.
    func syncNow(force: Bool) async {
        guard !list.entries().isEmpty else { return }
        isSyncing = true
        let work = Task { await coordinator.syncAll(force: force) }
        // Show "syncing" on the rows while the work runs.
        let watcher = Task {
            while !Task.isCancelled {
                await reload()
                try? await Task.sleep(for: .milliseconds(400))
            }
        }
        let pulled = await work.value
        watcher.cancel()
        if pulled > 0 { changeCount += 1 }
        await reload()
    }

    func sync(_ id: String, confirmReupload: Bool = false) async {
        guard let entry = list.entry(id: id) else { return }
        let pulled = await coordinator.sync(entry, force: true, confirmReupload: confirmReupload)
        if pulled > 0 { changeCount += 1 }
        await reload()
    }

    /// Keep syncing while the app is in front: now, then every `syncIntervalSecs`. Cheap to call again.
    func startForeground() {
        guard loop == nil else { return }
        loop = Task { [weak self] in
            while !Task.isCancelled {
                await self?.syncNow(force: false)
                try? await Task.sleep(for: .seconds(Double(syncIntervalSecs())))
            }
        }
    }

    func stopForeground() {
        loop?.cancel()
        loop = nil
    }

    // MARK: Changing the list

    /// Add the folder the person picked as a remote. The vault on this phone is the one it must hold or be empty
    /// for (ADR-0043): a folder of other files is refused, and nothing is created in it here.
    func addFolder(_ url: URL) async throws {
        guard case .ready(let vaultId, _) = vault.status() else { throw FolderProblem.noVault }
        let opened = try FolderRemote.open(url: url, localVault: vault.container.vaultFolder)
        defer { opened.close() }
        try FolderRemote.checkCanHold(vaultId: vaultId, in: opened.store)
        let entry = try FolderRemote.entry(for: url)
        list.add(entry)
        await sync(entry.id)
    }

    /// The person picked a folder again for a remote whose folder went missing or whose bookmark went stale. The
    /// same checks as adding apply; the remote keeps its id, so its sync record stays.
    func replaceFolder(_ id: String, with url: URL) async throws {
        guard let old = list.entry(id: id), old.kind == .folder,
            case .ready(let vaultId, _) = vault.status()
        else { throw FolderProblem.noVault }
        let opened = try FolderRemote.open(url: url, localVault: vault.container.vaultFolder)
        defer { opened.close() }
        try FolderRemote.checkCanHold(vaultId: vaultId, in: opened.store)
        var entry = old
        entry.name = FolderRemote.displayName(for: url)
        entry.bookmark = try FolderRemote.bookmark(for: url)
        list.add(entry)
        await sync(id)
    }

    func remove(_ id: String) async {
        list.remove(id: id)
        await coordinator.forget(id: id)
        await reload()
    }

    func driveSignedIn() async {
        list.reconcileDrive(signedIn: true)
        await reload()
        await sync(RemoteEntry.driveID)
    }

    func driveSignedOut() async {
        drive.signOut()
        list.reconcileDrive(signedIn: false)
        await coordinator.forget(id: RemoteEntry.driveID)
        await reload()
    }

    /// Forget everything after the vault was removed from this phone.
    func vaultRemoved() async {
        stopForeground()
        rows = []
        for entry in list.entries() { await coordinator.forget(id: entry.id) }
    }
}

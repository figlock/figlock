import FigLockFFI
import Foundation

/// Where one remote stands: what the Sync screen, the vault list chip and the background task read.
public struct RemoteState: Equatable, Sendable {
    public enum Phase: Equatable, Sendable {
        /// Not tried since the app started.
        case idle
        case syncing
        case synced
        /// No connection. The local vault works; the next run retries.
        case offline
        /// Google needs the person to sign in again.
        case signInNeeded
        /// iCloud has not downloaded the vault yet ("waiting for iCloud"). Retried on the next run.
        case waitingForDownload
        /// The remote holds a different vault than this phone. Never merged.
        case differentVault
        /// The remote lost its vault (deleted, or an empty one found). Uploading again needs the person's yes.
        case remoteLost
        case damaged
        case unavailable(FolderProblem)
        case failed
    }

    public var phase: Phase = .idle
    /// When this remote last finished a sync, from the device's own record.
    public var lastSyncMs: UInt64?
    /// Failures in a row, for the back-off.
    public var failures = 0
    public var retryAfter: Date?

    /// A problem the person has to act on, as opposed to one that clears itself.
    public var needsAttention: Bool {
        switch phase {
        case .signInNeeded, .differentVault, .remoteLost, .damaged, .unavailable, .failed: true
        default: false
        }
    }
}

/// Runs the sync of every remote of this phone, each on its own: its own sync state, back-off, same-vault check
/// and remote-lost confirmation, so a failing or lost remote never blocks another (ADR-0043, ADR-0026). The local
/// vault in the App Group container is always the working copy; a remote is pulled from and pushed to.
public actor RemoteSyncCoordinator {
    public struct Dependencies: Sendable {
        let vault: LocalVault
        let list: RemoteList
        /// The Drive store while this phone is signed in to Google, otherwise nil.
        let drive: @Sendable () -> (any BlobStoreFfi)?
        /// This device's id in hex, once it has one (it comes from the keychain, so no unlock is needed).
        let deviceId: @Sendable () -> String?
        let now: @Sendable () -> Date
        let iCloudOn: @Sendable () -> Bool

        public init(
            vault: LocalVault, list: RemoteList, drive: @escaping @Sendable () -> (any BlobStoreFfi)?,
            deviceId: @escaping @Sendable () -> String?, now: @escaping @Sendable () -> Date = { Date() },
            iCloudOn: @escaping @Sendable () -> Bool = { FolderRemote.iCloudIsOn }
        ) {
            self.vault = vault
            self.list = list
            self.drive = drive
            self.deviceId = deviceId
            self.now = now
            self.iCloudOn = iCloudOn
        }
    }

    private let dependencies: Dependencies
    private var states: [String: RemoteState] = [:]
    private var running: Set<String> = []

    public init(_ dependencies: Dependencies) {
        self.dependencies = dependencies
    }

    /// The state of `id`, with the last sync time read from the device's record.
    public func state(of entry: RemoteEntry) -> RemoteState {
        var state = states[entry.id] ?? RemoteState()
        if state.lastSyncMs == nil {
            state.lastSyncMs = FileSyncState(container: dependencies.vault.container, remote: entry).lastSyncMs()
        }
        return state
    }

    /// Drop what is remembered about `id` (a removed remote).
    public func forget(id: String) {
        states[id] = nil
    }

    /// Run `work` while no sync of `id` runs and none starts: a sync that uploads while the copy is being deleted
    /// would put files back and fail the deletion.
    public func exclusive<T: Sendable>(_ id: String, _ work: @Sendable () async throws -> T) async throws -> T {
        while running.contains(id) { try await Task.sleep(for: .milliseconds(200)) }
        running.insert(id)
        defer { running.remove(id) }
        return try await work()
    }

    /// Sync every remote that is due, all at once. `force` ignores the back-off (a person asked for it).
    /// Returns the number of remote files that changed the local vault.
    @discardableResult
    public func syncAll(force: Bool) async -> Int {
        let entries = dependencies.list.entries()
        // One task per remote: each waits on its own network and never on another remote.
        let tasks = entries.map { entry in
            Task { await self.sync(entry, force: force, confirmReupload: false) }
        }
        var pulled = 0
        for task in tasks { pulled += await task.value }
        return pulled
    }

    /// Sync one remote. `confirmReupload` is the person's yes to uploading everything again after it was lost.
    /// Returns how many files the local vault gained.
    @discardableResult
    public func sync(_ entry: RemoteEntry, force: Bool, confirmReupload: Bool) async -> Int {
        guard !running.contains(entry.id) else { return 0 }
        var state = states[entry.id] ?? RemoteState()
        if !force, let retry = state.retryAfter, retry > dependencies.now() { return 0 }
        // A remote that needs the person is not retried in the background; a manual sync tries again.
        if !force, !confirmReupload, state.phase == .remoteLost || state.phase == .differentVault { return 0 }
        guard case .ready = dependencies.vault.status() else { return 0 }
        running.insert(entry.id)
        state.phase = .syncing
        states[entry.id] = state
        defer { running.remove(entry.id) }

        let dependencies = dependencies
        let outcome = await Task.detached(priority: .utility) {
            Self.execute(entry, confirmReupload: confirmReupload, dependencies)
        }.value

        var next = states[entry.id] ?? RemoteState()
        next.phase = outcome.phase
        // A run that left files behind still finished: it pulled and pushed what it could, and the state says so.
        // It is not a failure to back off from (the files are on their way), but it is not "Synced" either.
        if outcome.phase == .synced || outcome.waiting > 0 {
            next.failures = 0
            next.retryAfter = nil
            next.lastSyncMs = FileSyncState(container: dependencies.vault.container, remote: entry).lastSyncMs()
        } else {
            next.failures += 1
            next.retryAfter = dependencies.now().addingTimeInterval(
                TimeInterval(syncBackoffSecs(failures: UInt32(max(next.failures - 1, 0)))))
        }
        states[entry.id] = next
        return outcome.pulled
    }

    struct Outcome: Error {
        var phase: RemoteState.Phase
        var pulled = 0
        /// Files the remote lists that iCloud has not downloaded yet. A download was asked for each one.
        var waiting = 0
    }

    /// The store of `entry`, ready to use, or the state to report when it cannot be opened. A folder stays open
    /// (its security scope held) for as long as the returned `OpenedFolder` lives.
    private static func open(_ entry: RemoteEntry, _ deps: Dependencies) -> Result<
        (any BlobStoreFfi, OpenedFolder?), Outcome
    > {
        switch entry.kind {
        case .drive:
            guard let drive = deps.drive() else { return .failure(Outcome(phase: .signInNeeded)) }
            return .success((drive, nil))
        case .folder:
            guard let bookmark = entry.bookmark else { return .failure(Outcome(phase: .unavailable(.pickAgain))) }
            do {
                let opened = try FolderRemote.open(
                    bookmark: bookmark, localVault: deps.vault.container.vaultFolder, iCloudOn: deps.iCloudOn)
                if let refreshed = opened.refreshed {
                    var renewed = entry
                    renewed.bookmark = refreshed
                    deps.list.add(renewed)
                }
                return .success((opened.folder.store, opened.folder))
            } catch let problem as FolderProblem {
                return .failure(Outcome(phase: .unavailable(problem)))
            } catch {
                return .failure(Outcome(phase: .unavailable(.missing)))
            }
        }
    }

    private static func execute(_ entry: RemoteEntry, confirmReupload: Bool, _ deps: Dependencies) -> Outcome {
        let remote: any BlobStoreFfi
        let folder: OpenedFolder?
        switch open(entry, deps) {
        case .failure(let outcome): return outcome
        case .success(let opened): (remote, folder) = opened
        }
        defer { folder?.close() }
        let local: any BlobStoreFfi
        do { local = try deps.vault.openStore() } catch { return Outcome(phase: .failed) }
        let engine = SyncEngine(
            local: local, remote: remote, state: FileSyncState(container: deps.vault.container, remote: entry),
            clock: SyncRunClock(), ownDevice: deps.deviceId())
        do {
            let report = try engine.run(confirmReupload: confirmReupload)
            // "Synced" only when nothing was skipped: files another device wrote that iCloud still holds in
            // the cloud mean this phone has not seen everything yet (FIG-226).
            return Outcome(
                phase: report.waiting > 0 ? .waitingForDownload : .synced, pulled: Int(report.pulled),
                waiting: Int(report.waiting))
        } catch let failure as SyncFailure {
            return Outcome(phase: phase(for: failure))
        } catch let failure as StoreFailure {
            return Outcome(phase: phase(for: failure, folder: entry.kind == .folder))
        } catch {
            return Outcome(phase: .failed)
        }
    }

    static func phase(for failure: StoreFailure, folder: Bool) -> RemoteState.Phase {
        switch failure {
        case .Unauthorized: .signInNeeded
        case .Unavailable, .NotYetAvailable: folder ? .waitingForDownload : .offline
        default: .failed
        }
    }

    static func phase(for failure: SyncFailure) -> RemoteState.Phase {
        switch failure {
        case .SignIn: .signInNeeded
        case .Offline: .offline
        case .ForeignVault: .differentVault
        case .RemoteLost: .remoteLost
        case .NotDownloaded: .waitingForDownload
        case .Damaged: .damaged
        default: .failed
        }
    }
}

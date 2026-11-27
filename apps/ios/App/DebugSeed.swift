#if DEBUG
    import FigLockFFI
    import Shared
    import SwiftUI

    /// Debug builds only: `-figlock-seed-demo` makes a vault with made-up values and cheap Argon2id settings on
    /// first launch, so the unlock screen can be run and tested before onboarding exists. The password is
    /// `demo-password-1`. Nothing here is a real secret and none of it is compiled into a release build.
    enum DebugSeed {
        static let password = "demo-password-1"

        private static let demoItems: [(type: String, fields: [(String, String)])] = [
            (
                "login",
                [
                    ("title", "Fastmail"), ("username", "mira@example.com"), ("url/a1", "https://app.fastmail.com"),
                    ("password", "Quartz-Meadow-Harbor-31"),
                ]
            ),
            (
                "login",
                [
                    ("title", "Linear"), ("username", "mira@example.com"), ("url/a1", "https://linear.app"),
                    ("password", "Birch-Signal-Copper-77"),
                ]
            ),
            (
                "token",
                [
                    ("title", "GitHub token"), ("provider", "github"), ("scopes", "repo (read), workflow"),
                    ("allowed_hosts/a1", "api.github.com"), ("token", "fl_demo_gh_Qp7xV2mK9d4R"),
                ]
            ),
            (
                "token",
                [
                    ("title", "Stripe API key"), ("provider", "stripe"), ("scopes", "charges (read)"),
                    ("token", "fl_demo_st_Rk9mF4vD1q7J"),
                ]
            ),
            ("note", [("title", "Wifi at home"), ("body", "Network: demo-net")]),
        ]

        /// `-figlock-unlock-demo` also unlocks with the demo password, to look at the vault screens in a simulator.
        @MainActor
        static func unlockIfRequested(_ environment: AppEnvironment) {
            guard ProcessInfo.processInfo.arguments.contains("-figlock-unlock-demo") else { return }
            Task { _ = await environment.lock.unlock(password: password, secretKey: nil) }
        }

        /// `-figlock-demo-remotes` adds two folder remotes in the app's Documents folder to the seeded vault, so the
        /// sync screens can be looked at in a simulator: "FigLock" works and syncs, "Old FigLock" points at a folder
        /// that is then deleted, which shows the "Folder not found" state next to a working remote.
        @MainActor
        static func remotesIfRequested(_ environment: AppEnvironment) {
            guard ProcessInfo.processInfo.arguments.contains("-figlock-demo-remotes"),
                environment.startupError == nil, case .ready = environment.vault.status(),
                environment.sync.list.entries().isEmpty
            else { return }
            let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            for name in ["FigLock", "Old FigLock"] {
                let folder = documents.appendingPathComponent(name, isDirectory: true)
                try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
                guard let bookmark = try? FolderRemote.bookmark(for: folder) else { continue }
                environment.sync.list.add(.folder(name: name, bookmark: bookmark))
                if name == "Old FigLock" { try? FileManager.default.removeItem(at: folder) }
            }
            Task { await environment.sync.reload() }
        }

        /// `-figlock-seed-recovery` puts the throwaway vault of `RecoveryFixture` (with a Recovery Key) on a phone
        /// that has none, left locked, for the forgot-password UI test.
        @MainActor
        static func seedRecoveryIfRequested(_ environment: AppEnvironment) {
            guard ProcessInfo.processInfo.arguments.contains("-figlock-seed-recovery"),
                environment.startupError == nil, environment.vault.status() == .noVault
            else { return }
            do {
                try environment.vault.openStore().putNew(path: "vault.json", bytes: RecoveryFixture.vaultJson)
                try environment.vault.markVaultPresent()
                if case .ready(let id, _) = environment.vault.status() {
                    try environment.keeper.saveDeviceKeys(try DeviceKeysHandle.generate(), vaultId: id)
                    try environment.keeper.keep(RecoveryFixture.oldSecretKey, vaultId: id)
                }
            } catch {
                print("recovery seed failed")
            }
        }

        /// `-figlock-fresh` removes any vault from this phone first, so a UI test starts at onboarding.
        @MainActor
        static func freshIfRequested(_ environment: AppEnvironment) {
            guard ProcessInfo.processInfo.arguments.contains("-figlock-fresh") else { return }
            try? environment.onboarding.removeFromThisPhone()
        }

        @MainActor
        static func seedIfRequested(_ environment: AppEnvironment) {
            guard ProcessInfo.processInfo.arguments.contains("-figlock-seed-demo") else { return }
            // Whatever an earlier run left (a recovered vault, a created one) goes first, so the demo is the demo.
            try? environment.onboarding.removeFromThisPhone()
            // Every launch starts with no password pause left over from an earlier run (UI tests trip it on purpose).
            try? FileManager.default.removeItem(
                at: environment.vault.container.deviceFolder.appendingPathComponent("password-throttle.json"))
            guard environment.startupError == nil, environment.vault.status() == .noVault else { return }
            do {
                let created = try createVault(
                    password: password, deviceName: "Demo iPhone", kdf: minimumKdfSettings(),
                    nowMs: UInt64(Date().timeIntervalSince1970 * 1000))
                let store = try environment.vault.openStore()
                try store.putNew(path: "vault.json", bytes: created.vaultJson())
                let keys = created.deviceKeys()
                try store.putNew(path: "devices/\(keys.deviceId()).json", bytes: created.deviceRecord())
                // A few made-up items, committed as an op file like any edit.
                let writer = try created.takeSession()
                let now = UInt64(Date().timeIntervalSince1970 * 1000)
                for (offset, item) in demoItems.enumerated() {
                    _ = try writer.addItem(
                        itemType: item.type, vault: "personal",
                        fields: item.fields.map { FieldInput(name: $0.0, value: $0.1) }, nowMs: now + UInt64(offset))
                }
                if let file = try writer.commit() { try store.putNew(path: file.path, bytes: file.bytes) }
                writer.lock()
                try environment.vault.markVaultPresent()
                if case .ready(let id, _) = environment.vault.status() {
                    try environment.keeper.saveDeviceKeys(keys, vaultId: id)
                    try environment.keeper.keep(try created.secretKeyText(), vaultId: id)
                }
            } catch {
                print("demo seed failed")
            }
        }
    }
#endif

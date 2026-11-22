#if DEBUG
    import FigLockFFI
    import Shared
    import SwiftUI

    /// Debug builds only: `-figlock-seed-demo` makes a vault with made-up values and cheap Argon2id settings on
    /// first launch, so the unlock screen can be run and tested before onboarding exists. The master password is
    /// `demo-password-1`. Nothing here is a real secret and none of it is compiled into a release build.
    enum DebugSeed {
        static let password = "demo-password-1"

        @MainActor
        static func seedIfRequested(_ environment: AppEnvironment) {
            guard ProcessInfo.processInfo.arguments.contains("-figlock-seed-demo") else { return }
            guard environment.startupError == nil, environment.vault.status() == .noVault else { return }
            do {
                let created = try createVault(
                    password: password, deviceName: "Demo iPhone", kdf: minimumKdfSettings(),
                    nowMs: UInt64(Date().timeIntervalSince1970 * 1000))
                let store = try environment.vault.openStore()
                try store.putNew(path: "vault.json", bytes: created.vaultJson())
                let keys = created.deviceKeys()
                try store.putNew(path: "devices/\(keys.deviceId()).json", bytes: created.deviceRecord())
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

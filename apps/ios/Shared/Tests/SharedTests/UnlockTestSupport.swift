import CryptoKit
import FigLockFFI
import Foundation

@testable import Shared

/// A software P-256 key in place of the Secure Enclave, which the simulator does not have. It does the same ECDH
/// step; the wrap around it is the Rust code under test. `behaviour` lets a test play a cancelled or failed prompt.
final class FakeChip: QuickUnlockChip, @unchecked Sendable {
    enum Behaviour { case works, cancelled, notRecognised, keyGone, failing }

    private let lock = NSLock()
    private var key: P256.KeyAgreement.PrivateKey?
    private var behaviourValue = Behaviour.works
    private var biometrics: Data? = Data("enrolled-1".utf8)
    private(set) var prompts = 0

    var behaviour: Behaviour {
        get { lock.withLock { behaviourValue } }
        set { lock.withLock { behaviourValue = newValue } }
    }

    var hasKey: Bool { lock.withLock { key != nil } }

    /// What the system reports for the enrolled faces; changing it plays "a face was added".
    var biometricsState: Data? {
        get { lock.withLock { biometrics } }
        set { lock.withLock { biometrics = newValue } }
    }

    var isAvailable: Bool { true }

    func createKey() throws -> Data {
        lock.withLock {
            let fresh = P256.KeyAgreement.PrivateKey()
            key = fresh
            return fresh.publicKey.x963Representation
        }
    }

    func publicPoint() throws -> Data {
        try lock.withLock {
            guard let key else { throw ChipError.keyMissing }
            return key.publicKey.x963Representation
        }
    }

    func agree(ephemeralPublic: Data, reason: String) async throws -> Data {
        let (key, behaviour): (P256.KeyAgreement.PrivateKey?, Behaviour) = lock.withLock {
            prompts += 1
            return (key, behaviourValue)
        }
        switch behaviour {
        case .cancelled: throw ChipError.cancelled
        case .notRecognised: throw ChipError.authFailed
        case .keyGone: throw ChipError.keyMissing
        case .failing: throw ChipError.failed
        case .works: break
        }
        guard let key else { throw ChipError.keyMissing }
        let peer = try P256.KeyAgreement.PublicKey(x963Representation: ephemeralPublic)
        return try key.sharedSecretFromKeyAgreement(with: peer).withUnsafeBytes { Data($0) }
    }

    func deleteKey() throws { lock.withLock { key = nil } }

    func biometricState() -> Data? { biometricsState }
}

final class MemorySecrets: SecretStore, @unchecked Sendable {
    private let lock = NSLock()
    private var items: [String: Data] = [:]

    func read(_ account: String) throws -> Data? { lock.withLock { items[account] } }
    func write(_ account: String, _ data: Data) throws { lock.withLock { items[account] = data } }
    func delete(_ account: String) throws { lock.withLock { _ = items.removeValue(forKey: account) } }
    var accounts: Set<String> { lock.withLock { Set(items.keys) } }
}

/// A clock a test moves by hand.
final class TestClock: @unchecked Sendable {
    private let lock = NSLock()
    private var current = Date(timeIntervalSince1970: 1_800_000_000)
    var date: Date { lock.withLock { current } }
    func advance(_ seconds: TimeInterval) { lock.withLock { current = current.addingTimeInterval(seconds) } }
}

/// A phone with one real vault (made by the Rust core with cheap Argon2id settings) in a temporary container.
struct Phone {
    static let password = "generated-test-password-1"

    let container: VaultContainer
    let vault: LocalVault
    let secrets = MemorySecrets()
    let chip = FakeChip()
    let clock = TestClock()
    let secretKey: String
    let vaultId: String
    let deviceKeys: DeviceKeysHandle
    var boot = BootBox("boot-1")

    final class BootBox: @unchecked Sendable {
        private let lock = NSLock()
        private var id: String
        init(_ id: String) { self.id = id }
        var value: String {
            get { lock.withLock { id } }
            set { lock.withLock { id = newValue } }
        }
    }

    var keeper: SecretKeyKeeper { SecretKeyKeeper(store: secrets, container: container) }

    var quick: QuickUnlock {
        let boot = boot
        let clock = clock
        return QuickUnlock(container: container, chip: chip, bootSession: { boot.value }, now: { clock.date })
    }

    func cleanup() { try? FileManager.default.removeItem(at: container.root) }

    @MainActor
    func makeLock() -> AppLock {
        let clock = clock
        return AppLock(
            vault: vault, keeper: keeper, quick: quick, deviceName: "Test iPhone", now: { clock.date })
    }

    static func make() throws -> Phone {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("figlock-unlock-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let container = VaultContainer(root: root)
        try container.prepare()
        let vault = LocalVault(container: container, access: .readWrite)
        let created = try createVault(
            password: password, deviceName: "Test iPhone", kdf: minimumKdfSettings(),
            nowMs: 1_800_000_000_000)
        let store = try vault.openStore()
        try store.putNew(path: "vault.json", bytes: created.vaultJson())
        let keys = created.deviceKeys()
        try store.putNew(path: "devices/\(keys.deviceId()).json", bytes: created.deviceRecord())
        try vault.markVaultPresent()
        guard case .ready(let id, _) = vault.status() else { throw ChipError.failed }
        let phone = Phone(
            container: container, vault: vault, secretKey: try created.secretKeyText(), vaultId: id,
            deviceKeys: keys)
        try phone.keeper.saveDeviceKeys(keys, vaultId: id)
        try phone.keeper.keep(phone.secretKey, vaultId: id)
        return phone
    }
}

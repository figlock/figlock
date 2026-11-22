import Foundation
import LocalAuthentication
import Security

/// Why the hardware key could not do its job.
public enum ChipError: Error, Equatable, Sendable {
    /// No Secure Enclave or no passcode on this device.
    case unavailable
    /// The key is not there: never made, deleted, or gone because the passcode or biometrics were reset.
    case keyMissing
    /// The person dismissed the Face ID prompt. Not a failed attempt.
    case cancelled
    /// Face ID did not recognise the person.
    case authFailed
    case failed
}

/// The hardware-bound key `QK` of quick unlock (ADR-0025, ADR-0041 decision 6). It does the one step the chip
/// allows, an ECDH agreement that asks for the person; the wrap and unwrap around it are Rust (`figlock-core`).
public protocol QuickUnlockChip: Sendable {
    var isAvailable: Bool { get }
    /// A fresh key, replacing any earlier one. Returns its public point (65 bytes, uncompressed). No prompt.
    func createKey() throws -> Data
    /// The public point of the existing key. No prompt.
    func publicPoint() throws -> Data
    /// ECDH with the blob's ephemeral public point. Asks for Face ID (or the passcode) and returns the 32 byte
    /// shared secret. `reason` is the sentence in the system prompt.
    func agree(ephemeralPublic: Data, reason: String) async throws -> Data
    func deleteKey() throws
    /// What the system says about the enrolled biometrics now, so a change can be noticed. Nil when unknown.
    func biometricState() -> Data?
}

/// `QK` as a permanent Secure Enclave key in the shared keychain access group, bound to the passcode or biometrics
/// of this device and never synced (`ThisDeviceOnly`, `.userPresence`). The simulator has no Secure Enclave, so
/// quick unlock is unavailable there and the master password is used.
public struct SecureEnclaveChip: QuickUnlockChip {
    private let tag: Data
    private let accessGroup: String?

    public init(accessGroup: String?, tag: String = "com.figlock.ios.quick-unlock.qk") {
        self.accessGroup = accessGroup
        self.tag = Data(tag.utf8)
    }

    public var isAvailable: Bool {
        #if targetEnvironment(simulator)
            return false
        #else
            var error: NSError?
            return LAContext().canEvaluatePolicy(.deviceOwnerAuthentication, error: &error)
        #endif
    }

    private func baseQuery() -> [String: Any] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassKey,
            kSecAttrApplicationTag as String: tag,
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
        ]
        if let accessGroup { query[kSecAttrAccessGroup as String] = accessGroup }
        return query
    }

    public func createKey() throws -> Data {
        guard isAvailable else { throw ChipError.unavailable }
        try deleteKey()
        var accessError: Unmanaged<CFError>?
        guard
            let access = SecAccessControlCreateWithFlags(
                nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, [.privateKeyUsage, .userPresence], &accessError)
        else { throw ChipError.failed }
        var privateAttributes = baseQuery()
        privateAttributes[kSecClass as String] = nil
        privateAttributes[kSecAttrKeyType as String] = nil
        privateAttributes[kSecAttrIsPermanent as String] = true
        privateAttributes[kSecAttrAccessControl as String] = access
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeySizeInBits as String: 256,
            kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
            kSecPrivateKeyAttrs as String: privateAttributes,
        ]
        var error: Unmanaged<CFError>?
        guard let key = SecKeyCreateRandomKey(attributes as CFDictionary, &error) else { throw ChipError.failed }
        return try publicPoint(of: key)
    }

    public func publicPoint() throws -> Data {
        try publicPoint(of: loadKey(context: nil))
    }

    private func publicPoint(of key: SecKey) throws -> Data {
        guard let publicKey = SecKeyCopyPublicKey(key),
            let external = SecKeyCopyExternalRepresentation(publicKey, nil) as Data?
        else { throw ChipError.failed }
        return external
    }

    private func loadKey(context: LAContext?) throws -> SecKey {
        var query = baseQuery()
        query[kSecReturnRef as String] = true
        if let context { query[kSecUseAuthenticationContext as String] = context }
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        switch status {
        case errSecSuccess:
            // swiftlint:disable:next force_cast
            return item as! SecKey
        case errSecItemNotFound: throw ChipError.keyMissing
        default: throw ChipError.failed
        }
    }

    public func agree(ephemeralPublic: Data, reason: String) async throws -> Data {
        let tag = self.tag
        let accessGroup = self.accessGroup
        // The system call blocks while the person looks at the phone, so it stays off the main thread.
        return try await Task.detached {
            let chip = SecureEnclaveChip(accessGroup: accessGroup, tag: String(bytes: tag, encoding: .utf8) ?? "")
            let context = LAContext()
            context.localizedReason = reason
            let privateKey = try chip.loadKey(context: context)
            let peerAttributes: [String: Any] = [
                kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
                kSecAttrKeyClass as String: kSecAttrKeyClassPublic,
                kSecAttrKeySizeInBits as String: 256,
            ]
            guard let peer = SecKeyCreateWithData(ephemeralPublic as CFData, peerAttributes as CFDictionary, nil)
            else { throw ChipError.failed }
            var error: Unmanaged<CFError>?
            guard
                let shared = SecKeyCopyKeyExchangeResult(
                    privateKey, .ecdhKeyExchangeStandard, peer, [:] as CFDictionary, &error)
            else { throw Self.map(error?.takeRetainedValue()) }
            return shared as Data
        }.value
    }

    private static func map(_ error: CFError?) -> ChipError {
        guard let error else { return .failed }
        let code = CFErrorGetCode(error)
        let domain = CFErrorGetDomain(error) as String
        if domain == LAError.errorDomain {
            switch LAError.Code(rawValue: Int(code)) {
            case .userCancel, .systemCancel, .appCancel: return .cancelled
            case .authenticationFailed: return .authFailed
            default: return .failed
            }
        }
        switch OSStatus(code) {
        case errSecUserCanceled: return .cancelled
        case errSecAuthFailed: return .authFailed
        case errSecItemNotFound: return .keyMissing
        default: return .failed
        }
    }

    public func deleteKey() throws {
        let status = SecItemDelete(baseQuery() as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw ChipError.failed }
    }

    public func biometricState() -> Data? {
        let context = LAContext()
        var error: NSError?
        _ = context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error)
        return context.evaluatedPolicyDomainState
    }
}

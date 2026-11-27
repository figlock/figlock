import Foundation
import Security

/// Small secrets kept in the keychain: the Secret Key text and this device's keys (ADR-0041 decision 6).
public protocol SecretStore: Sendable {
    func read(_ account: String) throws -> Data?
    func write(_ account: String, _ data: Data) throws
    func delete(_ account: String) throws
}

public struct SecretStoreError: Error, Equatable, Sendable {
    public let status: Int32
}

/// Items in the keychain access group shared with the AutoFill extension. They are `ThisDeviceOnly` and never
/// `kSecAttrSynchronizable`, so they cannot reach iCloud Keychain or another phone through a backup. They carry no
/// access control, so reading one never shows a Face ID prompt: a password unlock stays prompt-free
/// (ADR-0037), and the hardware-bound step is `QK` alone.
public struct KeychainSecretStore: SecretStore {
    private let service: String
    private let accessGroup: String?

    public init(accessGroup: String?, service: String = "com.figlock.ios") {
        self.accessGroup = accessGroup
        self.service = service
    }

    private func query(_ account: String) -> [String: Any] {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecAttrSynchronizable as String: false,
        ]
        if let accessGroup { query[kSecAttrAccessGroup as String] = accessGroup }
        return query
    }

    public func read(_ account: String) throws -> Data? {
        var request = query(account)
        request[kSecReturnData as String] = true
        request[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let status = SecItemCopyMatching(request as CFDictionary, &item)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = item as? Data else { throw SecretStoreError(status: status) }
        return data
    }

    public func write(_ account: String, _ data: Data) throws {
        var item = query(account)
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        var status = SecItemAdd(item as CFDictionary, nil)
        if status == errSecDuplicateItem {
            let update: [String: Any] = [
                kSecValueData as String: data,
                kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
            ]
            status = SecItemUpdate(query(account) as CFDictionary, update as CFDictionary)
        }
        guard status == errSecSuccess else { throw SecretStoreError(status: status) }
    }

    public func delete(_ account: String) throws {
        let status = SecItemDelete(query(account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw SecretStoreError(status: status) }
    }
}

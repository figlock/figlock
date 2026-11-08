// Secure Enclave key agreement for FigLock quick unlock (docs/adr/0025-secure-enclave-quick-unlock.md).
//
// A bare command-line binary cannot keep a Secure Enclave key in the keychain (it needs a restricted
// entitlement), so the key is made with CryptoKit and the caller stores its opaque `dataRepresentation`. The blob
// is useless without this Mac's chip and a successful user-presence check, which the chip itself enforces every
// time the key is used.
//
// Plain C interface for Rust. Every function returns 0 on success and a code below otherwise. Nothing is logged.

import CryptoKit
import Foundation
import LocalAuthentication
import Security

private let codeOk: Int32 = 0
private let codeUnavailable: Int32 = 1
private let codeDeclined: Int32 = 2
private let codeFailed: Int32 = 3
private let codeBadInput: Int32 = 4
private let codeTooSmall: Int32 = 5

private let pointLength = 65

@_cdecl("figlock_se_available")
public func figlock_se_available() -> Int32 {
    SecureEnclave.isAvailable ? 1 : 0
}

/// Touch ID, the device password, or a paired Apple Watch. Older systems without the watch constraint get user
/// presence alone.
private func makeAccessControl() -> SecAccessControl? {
    var error: Unmanaged<CFError>?
    let ac = SecAccessControlCreateWithFlags(
        nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
        [.privateKeyUsage, .or, .userPresence, .watch], &error)
    if let ac = ac { return ac }
    error = nil
    return SecAccessControlCreateWithFlags(
        nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly, [.privateKeyUsage, .userPresence], &error)
}

/// Create a key. `blob` receives the opaque key data (`blobLen` its length) and `publicKey` the 65 byte
/// uncompressed P-256 point.
@_cdecl("figlock_se_create")
public func figlock_se_create(
    _ blob: UnsafeMutablePointer<UInt8>, _ blobCap: Int, _ blobLen: UnsafeMutablePointer<Int>,
    _ publicKey: UnsafeMutablePointer<UInt8>
) -> Int32 {
    guard SecureEnclave.isAvailable else { return codeUnavailable }
    guard let ac = makeAccessControl() else { return codeFailed }
    do {
        let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(accessControl: ac)
        let data = key.dataRepresentation
        let point = key.publicKey.x963Representation
        guard point.count == pointLength else { return codeFailed }
        guard data.count <= blobCap else { return codeTooSmall }
        data.copyBytes(to: blob, count: data.count)
        blobLen.pointee = data.count
        point.copyBytes(to: publicKey, count: pointLength)
        return codeOk
    } catch {
        return codeFailed
    }
}

/// The public point of a stored key. Needs no user presence.
@_cdecl("figlock_se_public")
public func figlock_se_public(
    _ blob: UnsafePointer<UInt8>, _ blobLen: Int, _ publicKey: UnsafeMutablePointer<UInt8>
) -> Int32 {
    guard SecureEnclave.isAvailable else { return codeUnavailable }
    do {
        let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(
            dataRepresentation: Data(bytes: blob, count: blobLen))
        let point = key.publicKey.x963Representation
        guard point.count == pointLength else { return codeFailed }
        point.copyBytes(to: publicKey, count: pointLength)
        return codeOk
    } catch {
        return codeFailed
    }
}

/// ECDH between the stored key and `peer` (a 65 byte point). The chip asks the person first, with `reason` as the
/// text of the system sheet. `shared` receives the 32 byte secret.
@_cdecl("figlock_se_agree")
public func figlock_se_agree(
    _ blob: UnsafePointer<UInt8>, _ blobLen: Int, _ peer: UnsafePointer<UInt8>,
    _ reason: UnsafePointer<CChar>, _ shared: UnsafeMutablePointer<UInt8>
) -> Int32 {
    guard SecureEnclave.isAvailable else { return codeUnavailable }
    let context = LAContext()
    context.localizedReason = String(cString: reason)
    do {
        let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(
            dataRepresentation: Data(bytes: blob, count: blobLen), authenticationContext: context)
        let peerKey: P256.KeyAgreement.PublicKey
        do {
            peerKey = try P256.KeyAgreement.PublicKey(
                x963Representation: Data(bytes: peer, count: pointLength))
        } catch {
            return codeBadInput
        }
        let secret = try key.sharedSecretFromKeyAgreement(with: peerKey)
        return secret.withUnsafeBytes { raw -> Int32 in
            guard raw.count == 32 else { return codeFailed }
            raw.copyBytes(to: UnsafeMutableRawBufferPointer(start: shared, count: 32))
            return codeOk
        }
    } catch {
        if let la = error as? LAError {
            switch la.code {
            case .userCancel, .appCancel, .systemCancel, .authenticationFailed, .userFallback, .notInteractive:
                return codeDeclined
            default:
                return codeFailed
            }
        }
        return codeFailed
    }
}

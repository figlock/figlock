// Drives the generated Swift bindings against the real library. Built and run by tests/bindings.rs when
// `swiftc` is installed. Uses generated test values only.
import Foundation

func check(_ ok: Bool, _ what: String) {
    if !ok {
        FileHandle.standardError.write(Data("FAILED: \(what)\n".utf8))
        exit(1)
    }
}

enum Kind { case failed, locked, invalidInput }

func kind(_ e: FigLockError) -> Kind {
    switch e {
    case .Failed: return .failed
    case .Locked: return .locked
    case .InvalidInput: return .invalidInput
    }
}

func expect(_ want: Kind, _ what: String, _ body: () throws -> Void) {
    do {
        try body()
        check(false, "\(what): expected \(want), got success")
    } catch let e as FigLockError {
        check(kind(e) == want, "\(what): expected \(want), got \(kind(e))")
    } catch {
        check(false, "\(what): unexpected error type")
    }
}

let password = "swift-smoke-password-\(UInt32.random(in: 0...UInt32.max))"
let secretValue = "swift-smoke-secret-\(UInt32.random(in: 0...UInt32.max))"
let now: UInt64 = 1_800_000_000_000

do {
    let created = try createVault(
        password: password, deviceName: "smoke", kdf: minimumKdfSettings(), nowMs: now)
    let vaultJson = try created.vaultJson()
    let record = try created.deviceRecord()
    let deviceBytes = try created.deviceKeys().toBytes()
    let secretKey = try created.secretKeyText()

    let pdf = try created.renderKit(vaultName: "Smoke", storage: "Local folder", created: "2026-11-05")
    check(pdf.starts(with: Array("%PDF".utf8)), "kit is a PDF")

    let session = try created.takeSession()
    let id = try session.addItem(
        itemType: "login", vault: "personal",
        fields: [
            FieldInput(name: "title", value: "Cloudflare"),
            FieldInput(name: "url/a1", value: "https://dash.cloudflare.com"),
            FieldInput(name: "password", value: secretValue),
        ], nowMs: now + 1)
    guard let op = try session.commit() else {
        check(false, "commit returned a file")
        exit(1)
    }

    var listing = try session.items(includeDeleted: false)
    check(listing.items.count == 1, "one item listed")
    check(listing.items[0].fields["title"] == "Cloudflare", "title listed")
    check(listing.items[0].fields["password"] == nil, "secret not in listing")
    check(listing.items[0].secretFields == ["password"], "secret field named")
    check(try session.revealField(itemId: id, field: "password") == secretValue, "reveal returns secret")
    check(try session.search(query: "cldflr", nowMs: now + 2, limit: 5).first?.id == id, "typo search finds it")
    check(try session.search(query: secretValue, nowMs: now + 2, limit: 5).isEmpty, "secret not searchable")

    session.lock()
    check(session.isLocked(), "locked")
    expect(.locked, "items on locked session") { _ = try session.items(includeDeleted: false) }

    // Unlock again from the stored files with the saved device keys.
    let device = try DeviceKeysHandle.fromBytes(bytes: deviceBytes)
    let outcome = try unlockVault(
        vaultJson: vaultJson, password: password, secretKey: secretKey, device: device,
        deviceName: "smoke", deviceRecords: [record], nowMs: now + 3)
    let again = outcome.session
    try again.applyFile(bytes: op.bytes, deviceId: try again.deviceId(), seq: 1, nowMs: now + 4)
    listing = try again.items(includeDeleted: false)
    check(listing.items.count == 1, "item restored after unlock")

    expect(.failed, "wrong password") {
        _ = try unlockVault(
            vaultJson: vaultJson, password: password + "x", secretKey: secretKey, device: device,
            deviceName: "smoke", deviceRecords: [record], nowMs: now + 5)
    }
    expect(.invalidInput, "malformed id") { _ = try again.revealField(itemId: "nope", field: "password") }
    print("OK")
} catch {
    FileHandle.standardError.write(Data("FAILED: unexpected throw \(error)\n".utf8))
    exit(1)
}

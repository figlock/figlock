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

    // Calls from many threads take turns; none corrupts the session, and locking mid-flight is safe.
    let busy = try createVault(
        password: password, deviceName: "busy", kdf: minimumKdfSettings(), nowMs: now).takeSession()
    DispatchQueue.concurrentPerform(iterations: 64) { i in
        do {
            _ = try busy.addItem(
                itemType: "note", vault: "personal",
                fields: [FieldInput(name: "title", value: "note \(i)")], nowMs: now + UInt64(i))
            _ = try busy.items(includeDeleted: false)
            _ = try busy.search(query: "note", nowMs: now, limit: 5)
        } catch {
            check(false, "concurrent call failed: \(error)")
        }
    }
    check(try busy.items(includeDeleted: false).items.count == 64, "all concurrent adds landed")
    DispatchQueue.concurrentPerform(iterations: 16) { i in
        if i == 8 { busy.lock() }
        do { _ = try busy.items(includeDeleted: false) } catch let e as FigLockError {
            check(kind(e) == .locked, "only Locked once locking races a call")
        } catch { check(false, "unexpected error type") }
    }
    check(busy.isLocked(), "locked after the race")

    // Joining: a wrong code and a garbage invite file fail with the one generic error.
    expect(.failed, "join with a garbage invite") {
        _ = try startJoin(
            code: "QCC4-AS5K-BX31-A7XH-M6AK", inviteBytes: Data("nope".utf8), device: device,
            deviceName: "phone", nowMs: now)
    }
    check(!inviteCodeIsValid(code: "QCC4-AS5K"), "short invite code is invalid")
    check(try inviteCodeCanonical(code: "qcc4 as5k bx31 a7xh m6ak") == "QCC4-AS5K-BX31-A7XH-M6AK", "canonical code")

    try runVectors(directory: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "")
    print("OK")
} catch {
    FileHandle.standardError.write(Data("FAILED: unexpected throw \(error)\n".utf8))
    exit(1)
}

// The published vectors of docs/spec/vectors, for every rule the exported API can decide on its own. The
// derivations that need fixed nonces run against the Rust core (it has the vector-only hooks); here the
// accept and reject outcomes and the encodings must agree with the same files.
func vectorCases(_ directory: String, _ file: String) throws -> [[String: Any]] {
    let url = URL(fileURLWithPath: directory).appendingPathComponent(file)
    let obj = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [String: Any]
    check(obj["format"] as? String == "figlock-vectors-1", "\(file) format")
    return obj["cases"] as! [[String: Any]]
}

func runVectors(directory: String) throws {
    check(!directory.isEmpty, "vectors directory passed as the first argument")
    var ran = 0

    for c in try vectorCases(directory, "secret-key.json") {
        let name = c["name"] as! String
        let inputs = c["inputs"] as! [String: Any]
        guard let text = inputs["text"] as? String ?? (c["expected"] as? [String: Any])?["text"] as? String
        else { continue }
        let rejects = (c["expected"] as? String) == "reject"
        check(secretKeyIsValid(text: text) == !rejects, "secret-key \(name)")
        ran += 1
    }

    for c in try vectorCases(directory, "pairing.json") {
        let name = c["name"] as! String
        let inputs = c["inputs"] as! [String: Any]
        let expected = c["expected"]
        switch name {
        case "invite-code-format":
            // The canonical spelling of the code is the vector's code.
            let code = (expected as! [String: Any])["code"] as! String
            check(try inviteCodeCanonical(code: code) == code, "pairing \(name)")
        case "invite-code-parse":
            check(try inviteCodeCanonical(code: inputs["code"] as! String) == "QCC4-AS5K-BX31-A7XH-M6AK",
                  "pairing \(name)")
        case "device-fingerprint":
            let want = (expected as! [String: Any])["fingerprint"] as! String
            check(try fingerprintOf(signPubHex: inputs["sign_pub"] as! String) == want, "pairing \(name)")
        case "invite-code-too-short", "invite-code-bad-char":
            check(!inviteCodeIsValid(code: inputs["code"] as! String), "pairing \(name)")
        default: continue
        }
        ran += 1
    }

    // Argon2id profile limits: the floor works, everything outside floor and ceiling is refused.
    for c in try vectorCases(directory, "kdf.json") {
        let name = c["name"] as! String
        let inputs = c["inputs"] as! [String: Any]
        guard let m = inputs["m_kib"] as? Int, let t = inputs["t"] as? Int, let p = inputs["p"] as? Int
        else { continue }
        let settings = KdfSettings(mKib: UInt32(m), t: UInt32(t), p: UInt32(p))
        if (c["expected"] as? String) == "reject" {
            expect(.failed, "kdf \(name)") {
                _ = try createVault(password: "vector-kdf", deviceName: "v", kdf: settings, nowMs: 1)
            }
            ran += 1
        } else if name == "argon2id-floor" {
            _ = try createVault(password: "vector-kdf", deviceName: "v", kdf: settings, nowMs: 1)
            ran += 1
        }
    }
    check(ran >= 12, "ran the published vectors (ran \(ran))")
}

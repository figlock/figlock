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

    try runSyncChecks()
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


// ---- sync through Swift-provided parts (ADR-0041 decision 3) ----
// The store, the clock and the sync state below are plain Swift: what the app supplies from NSFileCoordinator,
// its keychain and its container. The Rust engine does the syncing.

final class SwiftStore: BlobStoreFfi, @unchecked Sendable {
    private let lock = NSLock()
    private var files: [String: Data] = [:]
    var failing: StoreFailure?

    func put(_ path: String, _ bytes: Data) { lock.lock(); files[path] = bytes; lock.unlock() }
    func raw(_ path: String) -> Data? { lock.lock(); defer { lock.unlock() }; return files[path] }
    func names() -> [String] { lock.lock(); defer { lock.unlock() }; return files.keys.sorted() }
    func clear() { lock.lock(); files = [:]; lock.unlock() }

    func get(path: String) throws -> Data {
        if let f = failing { throw f }
        guard let d = raw(path) else { throw StoreFailure.NotFound }
        return d
    }
    func putNew(path: String, bytes: Data) throws {
        if let f = failing { throw f }
        lock.lock(); defer { lock.unlock() }
        if files[path] != nil { throw StoreFailure.Exists }
        files[path] = bytes
    }
    func putReplace(path: String, bytes: Data) throws {
        if let f = failing { throw f }
        put(path, bytes)
    }
    func list(prefix: String) throws -> [BlobEntry] {
        if let f = failing { throw f }
        lock.lock(); defer { lock.unlock() }
        return files.filter { $0.key.hasPrefix(prefix) }.sorted { $0.key < $1.key }.map {
            BlobEntry(path: $0.key, size: UInt64($0.value.count), mtimeMs: nil, available: true)
        }
    }
    func delete(path: String) throws {
        if let f = failing { throw f }
        lock.lock(); files[path] = nil; lock.unlock()
    }
}

final class SwiftClock: SyncClock, @unchecked Sendable {
    func nowMs() -> UInt64 { 1_800_000_000_123 }
}

final class SwiftState: SyncStateStore, @unchecked Sendable {
    private let lock = NSLock()
    private var synced = false
    private var last: UInt64?
    private var refused = false
    func syncedBefore() -> Bool { lock.lock(); defer { lock.unlock() }; return synced }
    func lastSyncMs() -> UInt64? { lock.lock(); defer { lock.unlock() }; return last }
    func signInRefused() -> Bool { lock.lock(); defer { lock.unlock() }; return refused }
    func markSynced(nowMs: UInt64) { lock.lock(); synced = true; last = nowMs; lock.unlock() }
    func markSignInRefused() { lock.lock(); refused = true; lock.unlock() }
    func clearSignInRefused() { lock.lock(); refused = false; lock.unlock() }
    func forget() { lock.lock(); synced = false; last = nil; lock.unlock() }
}

func expectSync(_ want: SyncFailure, _ what: String, _ body: () throws -> Void) {
    do {
        try body()
        check(false, "\(what): expected \(want), got success")
    } catch let e as SyncFailure {
        check(e == want, "\(what): expected \(want), got \(e)")
    } catch {
        check(false, "\(what): unexpected error type")
    }
}

func vaultJson(_ id: String, _ revision: Int, _ tag: String) -> Data {
    Data("{\"vault_id\":\"\(id)\",\"revision\":\(revision),\"tag\":\"\(tag)\"}".utf8)
}

func runSyncChecks() throws {
    let remote = SwiftStore()
    let aLocal = SwiftStore(), bLocal = SwiftStore()
    let aState = SwiftState(), bState = SwiftState()
    let a = SyncEngine(local: aLocal, remote: remote, state: aState, clock: SwiftClock(), ownDevice: "aa")
    let b = SyncEngine(local: bLocal, remote: remote, state: bState, clock: SwiftClock(), ownDevice: "bb")

    aLocal.put("vault.json", vaultJson("v1", 1, "x"))
    aLocal.put("devices/aa.json", Data("record-a".utf8))
    aLocal.put("log/aa/00000001.op", Data("op-a".utf8))

    // A round against the Swift store: up from A, down to B, then nothing left to do.
    let up = try a.run(confirmReupload: false)
    check(up.pushed == 3 && up.pulled == 0, "first sync pushes everything")
    check(aState.lastSyncMs() == 1_800_000_000_123, "the Swift clock and state were used")
    check(remote.names() == aLocal.names(), "the Swift remote holds A's files")
    let down = try b.run(confirmReupload: false)
    check(down.pulled == 3 && down.pushed == 0, "B pulls everything")
    check(bLocal.raw("log/aa/00000001.op") == Data("op-a".utf8), "bytes arrive unchanged")
    let quiet = try b.run(confirmReupload: false)
    check(quiet.pulled == 0 && quiet.pushed == 0, "a second sync changes nothing")

    // Remote lost: asks first, uploads only after the yes.
    remote.clear()
    expectSync(.RemoteLost, "remote lost") { _ = try a.run(confirmReupload: false) }
    check(remote.names().isEmpty, "nothing uploaded unasked")
    check(try a.run(confirmReupload: true).pushed == 3, "uploaded after the yes")

    // Another vault: never merged, never deleted.
    let other = SwiftStore()
    other.put("vault.json", vaultJson("someone-else", 9, "r"))
    let c = SyncEngine(local: aLocal, remote: other, state: SwiftState(), clock: SwiftClock(), ownDevice: "aa")
    expectSync(.ForeignVault, "other vault") { _ = try c.run(confirmReupload: true) }
    expectSync(.ForeignVault, "other vault is not deleted") { _ = try c.deleteRemote(typed: deleteRemotePhrase()) }
    check(other.names() == ["vault.json"], "the other vault is untouched")

    // A damaged vault.json stops the sync.
    let broken = SwiftStore()
    broken.put("vault.json", Data("{ not json".utf8))
    let d = SyncEngine(local: aLocal, remote: broken, state: SwiftState(), clock: SwiftClock(), ownDevice: "aa")
    expectSync(.Damaged, "damaged vault.json") { _ = try d.run(confirmReupload: true) }

    // A revocation: the authority's new vault.json arrives and the revoked device's record is removed.
    aLocal.put("devices/bb.json", Data("record-b".utf8))
    _ = try a.run(confirmReupload: false)
    _ = try b.run(confirmReupload: false)
    check(bLocal.raw("devices/bb.json") != nil, "B has its record")
    aLocal.put("vault.json", vaultJson("v1", 2, "revoked-b"))
    try aLocal.delete(path: "devices/bb.json")
    _ = try a.run(confirmReupload: false)
    _ = try b.run(confirmReupload: false)
    check(bLocal.raw("devices/bb.json") == nil, "the revoked device's record is gone")
    check(bLocal.raw("vault.json") == aLocal.raw("vault.json"), "the new vault.json arrived")

    // A refused sign-in and an unreachable remote come through as their own cases, with no text.
    remote.failing = .Unauthorized
    expectSync(.SignIn, "refused sign-in") { _ = try a.run(confirmReupload: false) }
    check(aState.signInRefused(), "the Swift state remembers the refusal")
    remote.failing = .Unavailable
    expectSync(.Offline, "offline") { _ = try a.run(confirmReupload: false) }
    remote.failing = nil

    // Only the typed phrase deletes the remote copy, and the device forgets it synced.
    expectSync(.NotConfirmed, "no phrase") { _ = try a.deleteRemote(typed: "yes") }
    check(try a.deleteRemote(typed: deleteRemotePhrase()) > 0, "deleted with the phrase")
    check(remote.names().isEmpty && !aState.syncedBefore(), "remote empty, state forgotten")

    // The engine is called from several threads: the Swift store must hold up under the parallel copies.
    let bulk = SwiftStore(), bulkRemote = SwiftStore()
    bulk.put("vault.json", vaultJson("v2", 1, "x"))
    for i in 0..<40 { bulk.put("log/aa/\(String(format: "%08d", i)).op", Data("op \(i)".utf8)) }
    let bulkEngine = SyncEngine(local: bulk, remote: bulkRemote, state: SwiftState(), clock: SwiftClock(), ownDevice: "aa")
    check(try bulkEngine.run(confirmReupload: false).pushed == 41, "all files copied in parallel")
    check(bulkRemote.names() == bulk.names(), "the same files on both sides")
    check(syncIntervalSecs() == 15 && syncBackoffSecs(failures: 9) == 300, "timing rules")
}

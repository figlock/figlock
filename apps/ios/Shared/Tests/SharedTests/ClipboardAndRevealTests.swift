import FigLockFFI
import Foundation
import Testing

@testable import Shared

@MainActor
@Test func copyingKeepsTheValueLocalOnlyAndClearsItAfter30Seconds() async throws {
    let board = FakePasteboard()
    let gate = SleepGate()
    let now = Date(timeIntervalSince1970: 1_800_000_000)
    let copier = ClipboardCopier(pasteboard: board, now: { now }, sleep: { await gate.sleep($0) })

    copier.copy("generated-secret", key: "password")
    #expect(board.text == "generated-secret")
    #expect(board.localOnly)
    #expect(board.expiry == now.addingTimeInterval(30))
    #expect(copier.copiedKey == "password")

    await waitFor { await gate.sleeping > 0 }
    #expect(await gate.requested == [30])
    await gate.fire()
    await waitFor { copier.copiedKey == nil }
    #expect(board.text == nil)
}

@MainActor
@Test func theTimerLeavesAClipboardSomeoneElseFilledAlone() async throws {
    let board = FakePasteboard()
    let gate = SleepGate()
    let copier = ClipboardCopier(pasteboard: board, sleep: { await gate.sleep($0) })

    copier.copy("generated-secret", key: "password")
    board.foreignCopy("something the person copied later")
    await waitFor { await gate.sleeping > 0 }
    await gate.fire()
    await waitFor { copier.copiedKey == nil }
    #expect(board.text == "something the person copied later")
}

@MainActor
@Test func aSecondCopyRestartsTheTimerAndLockingClearsNow() async throws {
    let board = FakePasteboard()
    let gate = SleepGate()
    let copier = ClipboardCopier(pasteboard: board, sleep: { await gate.sleep($0) })

    copier.copy("first-generated", key: "password")
    copier.copy("second-generated", key: "token")
    #expect(copier.copiedKey == "token")
    copier.clearNow()
    #expect(board.text == nil)
    #expect(copier.copiedKey == nil)
}

@MainActor
@Test func lockingLeavesAClipboardSomeoneElseFilledAlone() async throws {
    let board = FakePasteboard()
    let copier = ClipboardCopier(pasteboard: board, sleep: { _ in try? await Task.sleep(for: .seconds(60)) })

    copier.copy("generated-secret", key: "password")
    board.foreignCopy("something the person copied later")
    copier.clearNow()
    #expect(board.text == "something the person copied later")
    #expect(copier.copiedKey == nil)
}

@MainActor
@Test func aFaceIDResultThatArrivesAfterHideAllShowsNothing() async throws {
    let seeded = try SeededVault.make([("login", [("title", "Fastmail"), ("password", "generated-pass-42")])])
    defer { seeded.cleanup() }
    _ = await seeded.browser()
    let authenticator = GatedAuthenticator()
    let reveal = SecretReveal(
        session: seeded.session, itemId: seeded.itemIds[0], authenticator: authenticator, sleep: { _ in })

    let pending = Task { await reveal.show("password", label: "Password") }
    await waitFor { await authenticator.pending > 0 }
    reveal.hideAll()
    await authenticator.release()
    #expect(await pending.value == false)
    #expect(reveal.shown.isEmpty)
}

@MainActor
@Test func theScenePausingForFaceIDDoesNotThrowAwayTheReveal() async throws {
    // The Face ID sheet takes the scene to inactive while it is up; the reveal it asks for must still show (FIG-223).
    let seeded = try SeededVault.make([("login", [("title", "Fastmail"), ("password", "generated-pass-42")])])
    defer { seeded.cleanup() }
    _ = await seeded.browser()
    let authenticator = GatedAuthenticator()
    let reveal = SecretReveal(
        session: seeded.session, itemId: seeded.itemIds[0], authenticator: authenticator, sleep: { _ in })

    let pending = Task { await reveal.show("password", label: "Password") }
    await waitFor { await authenticator.pending > 0 }
    reveal.hideForScenePhase(isBackground: false)
    await authenticator.release()
    #expect(await pending.value)
    #expect(reveal.shown["password"] == "generated-pass-42")

    // With nothing pending, leaving the foreground masks it again.
    reveal.hideForScenePhase(isBackground: false)
    #expect(reveal.shown.isEmpty)
}

@MainActor
@Test func theAppGoingToTheBackgroundDuringFaceIDStillCancelsTheReveal() async throws {
    let seeded = try SeededVault.make([("login", [("title", "Fastmail"), ("password", "generated-pass-42")])])
    defer { seeded.cleanup() }
    _ = await seeded.browser()
    let authenticator = GatedAuthenticator()
    let reveal = SecretReveal(
        session: seeded.session, itemId: seeded.itemIds[0], authenticator: authenticator, sleep: { _ in })

    let pending = Task { await reveal.show("password", label: "Password") }
    await waitFor { await authenticator.pending > 0 }
    reveal.hideForScenePhase(isBackground: true)
    await authenticator.release()
    #expect(await pending.value == false)
    #expect(reveal.shown.isEmpty)
}

@MainActor
@Test func aFieldMaskingItselfDoesNotCancelAPromptForAnotherField() async throws {
    let seeded = try SeededVault.make([
        ("login", [("title", "Fastmail"), ("password", "generated-pass-42"), ("totp", "generated-totp-7")])
    ])
    defer { seeded.cleanup() }
    _ = await seeded.browser()
    let authenticator = GatedAuthenticator()
    let reveal = SecretReveal(
        session: seeded.session, itemId: seeded.itemIds[0], authenticator: authenticator, sleep: { _ in })

    let first = Task { await reveal.show("password", label: "Password") }
    await waitFor { await authenticator.pending > 0 }
    await authenticator.release()
    #expect(await first.value)
    reveal.hide("password")

    let second = Task { await reveal.show("totp", label: "Code") }
    await waitFor { await authenticator.pending > 0 }
    reveal.hide("password")
    await authenticator.release()
    #expect(await second.value)
    #expect(reveal.shown["totp"] == "generated-totp-7")
}

@MainActor
private func revealSetup(allow: Bool, gate: SleepGate) async throws -> (SeededVault, SecretReveal) {
    let seeded = try SeededVault.make([
        ("login", [("title", "Fastmail"), ("username", "a@example.com"), ("password", "generated-pass-42")])
    ])
    _ = await seeded.browser()  // applies the op files to the session
    let reveal = SecretReveal(
        session: seeded.session, itemId: seeded.itemIds[0], authenticator: FakeAuthenticator(allow: allow),
        sleep: { await gate.sleep($0) })
    return (seeded, reveal)
}

@MainActor
@Test func aSecretStaysMaskedWhenFaceIDIsRefused() async throws {
    let (seeded, reveal) = try await revealSetup(allow: false, gate: SleepGate())
    defer { seeded.cleanup() }

    #expect(await reveal.show("password", label: "Password") == false)
    #expect(reveal.shown.isEmpty)
    #expect(!reveal.isShown("password"))
}

@MainActor
@Test func aSecretShowsAfterFaceIDAndMasksAgainAfter10Seconds() async throws {
    let gate = SleepGate()
    let (seeded, reveal) = try await revealSetup(allow: true, gate: gate)
    defer { seeded.cleanup() }

    #expect(await reveal.show("password", label: "Password"))
    #expect(reveal.shown["password"] == "generated-pass-42")
    await waitFor { await gate.sleeping > 0 }
    #expect(await gate.requested == [10])
    await gate.fire()
    await waitFor { !reveal.isShown("password") }
    #expect(reveal.shown.isEmpty)
}

@MainActor
@Test func hidingEarlyAndHidingEverythingForgetTheValues() async throws {
    let (seeded, reveal) = try await revealSetup(allow: true, gate: SleepGate())
    defer { seeded.cleanup() }

    _ = await reveal.show("password", label: "Password")
    reveal.hide("password")
    #expect(reveal.shown.isEmpty)
    _ = await reveal.show("password", label: "Password")
    reveal.hideAll()
    #expect(reveal.shown.isEmpty)
}

@MainActor
@Test func aFieldThatDoesNotExistFailsWithoutShowingAnything() async throws {
    let (seeded, reveal) = try await revealSetup(allow: true, gate: SleepGate())
    defer { seeded.cleanup() }

    #expect(await reveal.show("token", label: "Token") == false)
    #expect(reveal.failed == ["token"])
    #expect(reveal.shown.isEmpty)
}

@MainActor
@Test func aLockedSessionRevealsNothing() async throws {
    let (seeded, reveal) = try await revealSetup(allow: true, gate: SleepGate())
    defer { seeded.cleanup() }
    seeded.session.lock()

    #expect(await reveal.show("password", label: "Password") == false)
    #expect(reveal.valueForCopy("password") == nil)
}

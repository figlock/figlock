import Testing

@testable import Shared

@Test func defaultKdfIsTheMemoryHardProfile() {
    let kdf = FigLockCore.defaultKdf
    #expect(kdf.memoryKiB >= 32 * 1024)
    #expect(kdf.iterations >= 2)
}

@Test func secretKeyValidationRejectsGarbage() {
    #expect(!FigLockCore.isValidSecretKey("not a secret key"))
}

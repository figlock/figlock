import Testing

@testable import Shared

@Test func aSevenCharacterPasswordIsTooShort() {
    #expect(!VaultPasswordRule.isLongEnough("1234567"))
    #expect(!VaultPasswordRule.isLongEnough(""))
}

@Test func anEightCharacterPasswordIsLongEnough() {
    #expect(VaultPasswordRule.isLongEnough("12345678"))
    #expect(VaultPasswordRule.isLongEnough("a much longer pass phrase"))
}

@Test func lengthCountsCharactersNotBytes() {
    // Eight characters, more than eight bytes.
    #expect(VaultPasswordRule.isLongEnough("pässwörd"))
    #expect(!VaultPasswordRule.isLongEnough("pässwör"))
}

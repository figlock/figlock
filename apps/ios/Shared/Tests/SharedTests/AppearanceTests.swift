import Foundation
import Testing

@testable import Shared

// The Appearance choice (light, dark, system) kept in the device settings.

@Test func theAppearanceDefaultsToSystemSurvivesARestartAndIgnoresUnknownValues() throws {
    let phone = try Phone.make()
    defer { phone.cleanup() }
    let first = DeviceSettings(container: phone.container)
    #expect(first.appearance == .system)
    try first.setAppearance(.dark)
    try first.setAutoLock(.oneMinute)
    let second = DeviceSettings(container: phone.container)
    #expect(second.appearance == .dark, "setting another value keeps the appearance")
    #expect(AppearanceOption.allCases.map(\.title) == ["System", "Light", "Dark"])
    #expect(AppearanceOption.from(stored: "sepia") == .system)
    second.clear()
    #expect(DeviceSettings(container: phone.container).appearance == .system)
}

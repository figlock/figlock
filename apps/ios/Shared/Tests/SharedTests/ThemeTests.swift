import SwiftUI
import Testing
import UIKit

@testable import Shared

private func resolved(_ color: Color, _ style: UIUserInterfaceStyle) -> FLRGBA {
    var red: CGFloat = 0
    var green: CGFloat = 0
    var blue: CGFloat = 0
    var alpha: CGFloat = 0
    UIColor(color).resolvedColor(with: UITraitCollection(userInterfaceStyle: style))
        .getRed(&red, green: &green, blue: &blue, alpha: &alpha)
    return FLRGBA(
        red: Int((red * 255).rounded()), green: Int((green * 255).rounded()), blue: Int((blue * 255).rounded()),
        alpha: Double(alpha))
}

@Test func colorsFollowTheSystemAppearance() {
    #expect(resolved(FLColor.bg, .light) == FLRGBA(red: 250, green: 249, blue: 249, alpha: 1))
    #expect(resolved(FLColor.bg, .dark) == FLRGBA(red: 18, green: 17, blue: 20, alpha: 1))
    #expect(resolved(FLColor.primary, .light) == FLRGBA(red: 59, green: 90, blue: 254, alpha: 1))
    #expect(resolved(FLColor.primary, .dark) == FLRGBA(red: 159, green: 176, blue: 255, alpha: 1))
}

@Test func theOverlayKeepsItsAlpha() {
    #expect(resolved(FLColor.overlay, .light).alpha == 0.5)
    #expect(resolved(FLColor.overlay, .dark).alpha == 0.6)
}

@Test func controlsAreAtLeastFortyFourPointsAndSizesFollowTheTokens() {
    #expect(FLSize.minTouchTarget == 44)
    #expect(FLSize.controlHeightLg == 44)
    #expect(FLSize.controlHeightMd == 36)
    #expect(FLSize.focusRingWidth == 2)
}

@Test func spacingAndRadiusUseTheFourPointGrid() {
    #expect(FLSpace.s4 == 16)
    #expect(FLSpace.s1 == 4)
    #expect(FLSpace.s16 == 64)
    #expect(FLRadius.md == 14)
    #expect(FLText.md == 16)
    #expect(FLText.xl2 == 24)
}

@Test func motionKeepsTheTokenDurationUnlessReduceMotionIsOn() {
    #expect(FLMotion.duration(FLDuration.base, reduceMotion: false) == 0.2)
    #expect(FLMotion.duration(FLDuration.base, reduceMotion: true) == 0)
    #expect(FLMotion.animation(reduceMotion: false) != nil)
    #expect(FLMotion.animation(reduceMotion: true) == nil)
    #expect(FLEase.standard == FLCubicBezier(0.2, 0, 0, 1))
}

@Test func shadowsCarryTheirGeometryAndAColorPerAppearance() {
    #expect(FLShadow.md.offsetY == 4)
    #expect(FLShadow.md.blur == 12)
    #expect(resolved(FLShadow.md.color, .light).alpha == 0.1)
    #expect(resolved(FLShadow.md.color, .dark).alpha == 0.5)
}

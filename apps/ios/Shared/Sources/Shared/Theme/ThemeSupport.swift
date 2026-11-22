import SwiftUI
import UIKit

/// An sRGB color with 8 bit channels, as written in `design/tokens.css`.
public struct FLRGBA: Equatable, Sendable {
    public let red: Int
    public let green: Int
    public let blue: Int
    public let alpha: Double

    public init(red: Int, green: Int, blue: Int, alpha: Double) {
        self.red = red
        self.green = green
        self.blue = blue
        self.alpha = alpha
    }

    var uiColor: UIColor {
        UIColor(
            red: CGFloat(red) / 255, green: CGFloat(green) / 255, blue: CGFloat(blue) / 255, alpha: CGFloat(alpha))
    }
}

/// A color that follows the system appearance: `light` in light mode, `dark` in dark mode.
public func flDynamicColor(light: FLRGBA, dark: FLRGBA) -> Color {
    Color(
        uiColor: UIColor { traits in
            traits.userInterfaceStyle == .dark ? dark.uiColor : light.uiColor
        })
}

/// A shadow token: offset and blur in points, with a color for each appearance.
public struct FLShadowSpec: Sendable {
    public let offsetX: CGFloat
    public let offsetY: CGFloat
    public let blur: CGFloat
    public let light: FLRGBA
    public let dark: FLRGBA

    public init(offsetX: CGFloat, offsetY: CGFloat, blur: CGFloat, light: FLRGBA, dark: FLRGBA) {
        self.offsetX = offsetX
        self.offsetY = offsetY
        self.blur = blur
        self.light = light
        self.dark = dark
    }

    public var color: Color { flDynamicColor(light: light, dark: dark) }
}

/// A cubic Bezier easing curve, as in CSS `cubic-bezier(x1, y1, x2, y2)`.
public struct FLCubicBezier: Equatable, Sendable {
    public let firstX: Double
    public let firstY: Double
    public let secondX: Double
    public let secondY: Double

    public init(_ firstX: Double, _ firstY: Double, _ secondX: Double, _ secondY: Double) {
        self.firstX = firstX
        self.firstY = firstY
        self.secondX = secondX
        self.secondY = secondY
    }
}

extension FLSize {
    /// The smallest touch target on iOS: the large control height, 44 pt (DESIGN.md 5).
    public static let minTouchTarget: CGFloat = controlHeightLg
}

/// Motion that honours Reduce Motion: with it on, movement and fades are dropped and the state change is
/// immediate (DESIGN.md 6).
public enum FLMotion {
    /// The duration to use: the token's, or zero when the person asked to reduce motion.
    public static func duration(_ token: TimeInterval, reduceMotion: Bool) -> TimeInterval {
        reduceMotion ? 0 : token
    }

    /// The animation to use, or `nil` (no animation) when the person asked to reduce motion or the duration
    /// is zero.
    public static func animation(
        duration token: TimeInterval = FLDuration.base,
        curve: FLCubicBezier = FLEase.standard,
        reduceMotion: Bool
    ) -> Animation? {
        let seconds = duration(token, reduceMotion: reduceMotion)
        guard seconds > 0 else { return nil }
        return .timingCurve(curve.firstX, curve.firstY, curve.secondX, curve.secondY, duration: seconds)
    }
}

/// Fonts: SF Pro for the UI and SF Mono for secrets, scaled with the person's Dynamic Type setting.
public enum FLFont {
    /// The UI font at a token size, scaled like `style` so it follows Dynamic Type.
    public static func ui(
        _ size: CGFloat, weight: Font.Weight = FLWeight.regular, relativeTo style: UIFont.TextStyle = .body
    ) -> Font {
        .system(size: UIFontMetrics(forTextStyle: style).scaledValue(for: size), weight: weight)
    }

    /// The monospaced font for secrets, tokens and codes, scaled like `style`.
    public static func mono(
        _ size: CGFloat, weight: Font.Weight = FLWeight.regular, relativeTo style: UIFont.TextStyle = .body
    ) -> Font {
        .system(
            size: UIFontMetrics(forTextStyle: style).scaledValue(for: size), weight: weight, design: .monospaced)
    }
}

extension View {
    /// Keeps the view at least 44 by 44 pt, the iOS touch target, whatever its visible size.
    public func flMinTouchTarget() -> some View {
        frame(minWidth: FLSize.minTouchTarget, minHeight: FLSize.minTouchTarget)
            .contentShape(Rectangle())
    }

    /// Applies a shadow token.
    public func flShadow(_ spec: FLShadowSpec) -> some View {
        shadow(color: spec.color, radius: spec.blur / 2, x: spec.offsetX, y: spec.offsetY)
    }

    /// Animates `value` changes with a motion token, and not at all when Reduce Motion is on.
    public func flAnimation<Value: Equatable>(
        _ duration: TimeInterval = FLDuration.base, curve: FLCubicBezier = FLEase.standard, value: Value
    ) -> some View {
        modifier(FLAnimationModifier(duration: duration, curve: curve, value: value))
    }
}

private struct FLAnimationModifier<Value: Equatable>: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let duration: TimeInterval
    let curve: FLCubicBezier
    let value: Value

    func body(content: Content) -> some View {
        content.animation(
            FLMotion.animation(duration: duration, curve: curve, reduceMotion: reduceMotion), value: value)
    }
}

import SwiftUI

/// The FigLock app icon from `design/logo` (the iOS 1024 icon, exported at 1x to 3x of 96 pt), clipped to the
/// icon's own corner radius. Use it wherever the app shows its logo; never redraw it.
struct FigLockLogo: View {
    var size: CGFloat = 72

    var body: some View {
        Image("FigLockLogo")
            .resizable()
            .interpolation(.high)
            .frame(width: size, height: size)
            .clipShape(RoundedRectangle(cornerRadius: size * 228 / 1024, style: .continuous))
            .accessibilityHidden(true)
    }
}

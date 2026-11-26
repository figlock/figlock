import Shared
import SwiftUI

/// The look of the Settings screens (Files "Browse" in iOS): rounded grouped tiles on the page background, bold
/// section titles above them and roomy rows. One modifier so Settings, Auto-lock, Devices, Sync and About agree.
extension View {
    func flTileList() -> some View {
        listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(FLColor.bg)
            .environment(\.defaultMinListRowHeight, 56)
    }
}

/// A section title above a tile: larger and bolder than the system's small gray caps.
struct FLSectionHeader: View {
    let title: String

    init(_ title: String) { self.title = title }

    var body: some View {
        Text(title)
            .font(FLFont.ui(FLText.xl, weight: FLWeight.semibold, relativeTo: .title3))
            .foregroundStyle(FLColor.text)
            .textCase(nil)
            .padding(.top, FLSpace.s2)
            .accessibilityAddTraits(.isHeader)
    }
}

/// The sizes of a tile row: a title at 17 pt, and a subtitle under it at 15 pt (the size of "Location Services" and
/// its line in iOS Settings).
enum FLRowText {
    static let title: CGFloat = 17
    static let subtitle: CGFloat = 15
}

import Shared
import SwiftUI

/// The look of the Settings screens (Files "Browse" in iOS): rounded grouped tiles on the page background, bold
/// section titles above them and standard-height rows. One modifier so Settings, Auto-lock, Devices, Sync and About agree.
extension View {
    func flTileList() -> some View {
        listStyle(.insetGrouped)
            .scrollContentBackground(.hidden)
            .background(FLColor.bg)
            .environment(\.defaultMinListRowHeight, FLRow.minHeight)
    }
}

extension View {
    /// The rows of one section: the tile surface, and no vertical row padding, because the system's default is
    /// large next to the content and the minimum row height (`FLRow.minHeight`, set in `flTileList`) is what sets the height.
    /// Put it on the `Section`; a `listRowInsets` on the `List` itself does not reach the rows.
    func flTileRows() -> some View {
        listRowBackground(FLColor.surface)
            .listRowInsets(EdgeInsets(top: 0, leading: 20, bottom: 0, trailing: 20))
    }
}

/// Row sizes, taken from the password generator's option rows: 16 pt above and below a 31 pt control is 63 pt. A row
/// with a subtitle or more lines grows from the same 16 pt padding, so every row is in proportion.
enum FLRow {
    static let minHeight: CGFloat = 63
    static let verticalPadding: CGFloat = FLSpace.s4
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
            // Room between the title and its tile, as in the Files app.
            .padding(.bottom, FLSpace.s3)
            .accessibilityAddTraits(.isHeader)
    }
}

/// The sizes of a tile row: a title at 17 pt, and a subtitle under it at 15 pt (the size of "Location Services" and
/// its line in iOS Settings).
enum FLRowText {
    static let title: CGFloat = 17
    static let subtitle: CGFloat = 15
}

/// The rounded icon square at the start of a settings row: 29 pt with a 7 pt corner and a 17 pt glyph, the size of the
/// icons in iOS Settings, so it sits inside a row without stretching it.
struct FLIconTile: View {
    let symbol: String
    var color: Color = FLColor.primary

    var body: some View {
        Image(systemName: symbol)
            .font(.system(size: 16, weight: .medium))
            .foregroundStyle(color)
            .frame(width: 29, height: 29)
            .background(FLColor.primarySubtle, in: RoundedRectangle(cornerRadius: 7, style: .continuous))
            .accessibilityHidden(true)
    }
}

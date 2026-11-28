import Shared
import SwiftUI

/// One remote in the list: name, state, and a colored badge so the problem rows stand out.
struct RemoteRow: View {
    let row: SyncModel.Row
    /// The signed-in account (Google Drive), so the person can tell which one holds the copy.
    var account: String?

    var body: some View {
        HStack(spacing: FLSpace.s3) {
            FLIconTile(symbol: row.entry.symbol)
            VStack(alignment: .leading, spacing: 2) {
                Text(row.entry.name).font(FLFont.ui(FLRowText.title)).foregroundStyle(FLColor.text)
                if let account {
                    Text(account).font(FLFont.ui(FLRowText.subtitle, relativeTo: .subheadline))
                        .foregroundStyle(FLColor.textSecondary).lineLimit(1).truncationMode(.middle)
                        .accessibilityIdentifier("drive-account-email")
                }
                HStack(spacing: FLSpace.s1) {
                    Image(systemName: row.state.symbol).font(.system(size: 12))
                    Text(row.state.title)
                }
                .font(FLFont.ui(FLRowText.subtitle, relativeTo: .subheadline)).foregroundStyle(row.state.tone.color)
            }
            Spacer(minLength: 0)
        }
        .padding(.vertical, 6)
        .frame(minHeight: FLSize.controlHeightLg)
        .accessibilityElement(children: .combine)
    }
}

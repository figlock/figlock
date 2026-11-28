import SwiftUI
import UIKit

/// The system share sheet for the Emergency Kit PDF, without Copy (FIG-218). The kit holds the Secret Key, and
/// Copy would put the whole PDF on the general pasteboard, where other apps and Universal Clipboard can read it.
/// Save to Files, Print, AirDrop and the other destinations stay.
struct KitShareSheet: UIViewControllerRepresentable {
    let url: URL
    var onFinish: () -> Void = {}

    func makeUIViewController(context: Context) -> UIActivityViewController {
        let sheet = UIActivityViewController(activityItems: [url], applicationActivities: nil)
        sheet.excludedActivityTypes = [.copyToPasteboard]
        sheet.completionWithItemsHandler = { _, _, _, _ in onFinish() }
        return sheet
    }

    func updateUIViewController(_ controller: UIActivityViewController, context: Context) {}
}

/// A button that opens the kit share sheet. The label is whatever the screen wants; the styling comes from the
/// caller's `buttonStyle`.
struct KitShareButton<Label: View>: View {
    let url: URL
    var onOpen: () -> Void = {}
    @ViewBuilder var label: () -> Label
    @State private var sharing = false

    var body: some View {
        Button {
            onOpen()
            sharing = true
        } label: {
            label()
        }
        .sheet(isPresented: $sharing) {
            KitShareSheet(url: url).presentationDetents([.medium, .large]).presentationSizing(.form).ignoresSafeArea()
        }
    }
}

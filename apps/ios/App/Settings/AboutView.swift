import SafariServices
import Shared
import SwiftUI

/// A page on figlock.com the About screen opens inside the app. App Store guideline 5.1.1 wants the privacy policy
/// reachable inside the app as well as in the store listing. Only https pages on figlock.com are ever opened this
/// way, so the in-app browser cannot be pointed anywhere else.
struct AboutLink: Identifiable {
    let id: String
    let title: String
    let symbol: String
    let url: URL

    static let host = "figlock.com"

    static let all: [AboutLink] = [
        AboutLink(id: "protects", title: "How FigLock protects your data", symbol: "checkmark.shield", path: "faq"),
        AboutLink(id: "privacy", title: "Privacy policy", symbol: "doc", path: "privacy"),
        AboutLink(id: "terms", title: "Terms", symbol: "doc.text", path: "terms"),
        AboutLink(id: "support", title: "Support", symbol: "lifepreserver", path: "support"),
    ]

    private init(id: String, title: String, symbol: String, path: String) {
        self.id = id
        self.title = title
        self.symbol = symbol
        self.url = URL(string: "https://\(Self.host)/\(path)/")!
    }

    /// Whether `url` may be opened in the in-app browser.
    static func isAllowed(_ url: URL) -> Bool {
        url.scheme == "https" && url.host() == host
    }

    static var versionText: String {
        let info = Bundle.main.infoDictionary
        let version = info?["CFBundleShortVersionString"] as? String ?? "?"
        let build = info?["CFBundleVersion"] as? String ?? "?"
        return "Version \(version) (\(build))"
    }
}

/// Settings > About FigLock: the app mark, its version and the pages on figlock.com, opened in an in-app browser.
struct AboutView: View {
    @State private var opened: AboutLink?

    var body: some View {
        ScrollView {
            VStack(spacing: FLSpace.s6) {
                VStack(spacing: FLSpace.s2) {
                    FigLockLogo(size: 88)
                    Text("FigLock").font(FLFont.ui(FLText.xl, weight: FLWeight.bold, relativeTo: .title2))
                        .foregroundStyle(FLColor.text).accessibilityAddTraits(.isHeader)
                    Text(AboutLink.versionText).font(FLFont.ui(FLText.base, relativeTo: .callout))
                        .foregroundStyle(FLColor.textSecondary)
                }
                .padding(.top, FLSpace.s6)
                VStack(spacing: 0) {
                    ForEach(AboutLink.all) { link in
                        Button {
                            opened = link
                        } label: {
                            HStack {
                                SettingsRow(symbol: link.symbol, title: link.title)
                                Image(systemName: "chevron.right").font(.footnote.weight(.semibold))
                                    .foregroundStyle(FLColor.textTertiary).accessibilityHidden(true)
                            }
                            .padding(.horizontal, FLSpace.s4)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("about-\(link.id)")
                        .accessibilityHint("Opens figlock.com in the app")
                        if link.id != AboutLink.all.last?.id {
                            Divider().padding(.leading, FLSpace.s4 + 32 + FLSpace.s3)
                        }
                    }
                }
                .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.md))
            }
            .padding(.horizontal, FLSpace.s4)
        }
        .background(FLColor.bg.ignoresSafeArea())
        .navigationTitle("About FigLock")
        .navigationBarTitleDisplayMode(.inline)
        .fullScreenCover(item: $opened) { link in
            if AboutLink.isAllowed(link.url) { InAppBrowser(url: link.url).ignoresSafeArea() }
        }
    }
}

/// The system in-app browser (it shares Safari's look, shows the address and has a Done button).
private struct InAppBrowser: UIViewControllerRepresentable {
    let url: URL

    func makeUIViewController(context: Context) -> SFSafariViewController {
        let controller = SFSafariViewController(url: url)
        controller.preferredControlTintColor = UIColor(FLColor.primary)
        return controller
    }

    func updateUIViewController(_ controller: SFSafariViewController, context: Context) {}
}

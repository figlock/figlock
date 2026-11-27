import Shared
import SwiftUI
import WebKit

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
        List {
            VStack(spacing: FLSpace.s2) {
                FigLockLogo(size: 88)
                Text("FigLock").font(FLFont.ui(FLText.xl, weight: FLWeight.bold, relativeTo: .title2))
                    .foregroundStyle(FLColor.text).accessibilityAddTraits(.isHeader)
                Text(AboutLink.versionText).font(FLFont.ui(FLRowText.subtitle, relativeTo: .subheadline))
                    .foregroundStyle(FLColor.textSecondary)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, FLSpace.s4)
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
            Section {
                ForEach(AboutLink.all) { link in
                    Button {
                        opened = link
                    } label: {
                        HStack {
                            SettingsRow(symbol: link.symbol, title: link.title)
                            Image(systemName: "chevron.right").font(.footnote.weight(.semibold))
                                .foregroundStyle(FLColor.textTertiary).accessibilityHidden(true)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityIdentifier("about-\(link.id)")
                    .accessibilityHint("Opens figlock.com in the app")
                }
            } header: {
                FLSectionHeader("Learn more")
            }
            .listRowBackground(FLColor.surface)
        }
        .flTileList()
        .navigationTitle("About FigLock")
        .navigationBarTitleDisplayMode(.inline)
        .sheet(item: $opened) { link in
            if AboutLink.isAllowed(link.url) { InAppBrowser(link: link) }
        }
    }
}

/// An in-app page viewer for the figlock.com pages: a web view with a Done button, the page title, a progress bar and
/// a way out to Safari when the page cannot load. (SFSafariViewController stayed blank when presented from here.)
/// Navigation is limited to figlock.com over https; any other link goes to the system browser.
private struct InAppBrowser: View {
    let link: AboutLink
    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL
    @State private var model = WebPageModel()

    var body: some View {
        NavigationStack {
            ZStack {
                FLColor.bg.ignoresSafeArea()
                WebPage(url: link.url, model: model)
                    .ignoresSafeArea(edges: .bottom)
                    .accessibilityIdentifier("about-webview")
                if model.failed {
                    VStack(spacing: FLSpace.s3) {
                        Text("This page did not load").font(FLFont.ui(FLText.lg, weight: FLWeight.semibold, relativeTo: .headline))
                            .foregroundStyle(FLColor.text)
                        Text("Check your connection, or open it in Safari.")
                            .font(FLFont.ui(FLRowText.subtitle, relativeTo: .subheadline))
                            .foregroundStyle(FLColor.textSecondary)
                        Button("Open in Safari") { openURL(link.url) }.buttonStyle(.borderedProminent)
                            .tint(FLColor.primary)
                    }
                    .padding(FLSpace.s4)
                    .multilineTextAlignment(.center)
                }
            }
            .overlay(alignment: .top) {
                if model.loading { ProgressView(value: model.progress).tint(FLColor.primary) }
            }
            .navigationTitle(model.title.isEmpty ? link.title : model.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }.accessibilityIdentifier("about-browser-done")
                }
            }
        }
    }
}

@MainActor @Observable
private final class WebPageModel {
    var title = ""
    var progress = 0.0
    var loading = true
    var failed = false
}

private struct WebPage: UIViewRepresentable {
    let url: URL
    let model: WebPageModel

    func makeCoordinator() -> Coordinator { Coordinator(model) }

    func makeUIView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        let view = WKWebView(frame: .zero, configuration: configuration)
        view.navigationDelegate = context.coordinator
        view.isOpaque = false
        view.backgroundColor = .clear
        context.coordinator.observe(view)
        view.load(URLRequest(url: url))
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) {}

    final class Coordinator: NSObject, WKNavigationDelegate {
        let model: WebPageModel
        private var observations: [NSKeyValueObservation] = []

        init(_ model: WebPageModel) { self.model = model }

        func observe(_ view: WKWebView) {
            observations = [
                view.observe(\.estimatedProgress, options: [.new]) { [model] view, _ in
                    let value = view.estimatedProgress
                    Task { @MainActor in model.progress = value }
                },
                view.observe(\.title, options: [.new]) { [model] view, _ in
                    let value = view.title ?? ""
                    Task { @MainActor in model.title = value }
                },
            ]
        }

        func webView(
            _ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
            decisionHandler: @escaping @MainActor (WKNavigationActionPolicy) -> Void
        ) {
            guard let url = action.request.url else { return decisionHandler(.cancel) }
            if AboutLink.isAllowed(url) { return decisionHandler(.allow) }
            // Only the top frame navigates away; anything off figlock.com opens in the system browser.
            if action.targetFrame?.isMainFrame ?? true, url.scheme == "https" { UIApplication.shared.open(url) }
            decisionHandler(.cancel)
        }

        func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
            model.loading = true
            model.failed = false
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { model.loading = false }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            fail(error)
        }

        func webView(
            _ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error
        ) {
            fail(error)
        }

        private func fail(_ error: Error) {
            if (error as NSError).code == NSURLErrorCancelled { return }
            model.loading = false
            model.failed = true
        }
    }
}

import SwiftUI
import UIKit

/// Keeps what is on screen out of screenshots, screen recordings and the app switcher (`DESIGN.md` 8: secret screens
/// are blanked in the task switcher and screenshots are blocked where the platform allows).
///
/// iOS has no switch for "do not capture this". The system does leave content of a secure text field out of
/// captures, so a view hosted inside the layer of one is blank in a screenshot or recording. That layer is private
/// UIKit structure: if a future iOS changes it and the layer is not found, the content is shown normally and
/// `CaptureCover` (below) still covers the screen while it is being recorded or mirrored, so a change in iOS can
/// not blank the app, it can only make screenshots possible again.
public struct SecretScreen<Content: View>: UIViewRepresentable {
    public typealias Host = SecretHostView<Content>
    private let content: Content

    public init(@ViewBuilder content: () -> Content) {
        self.content = content()
    }

    public func makeUIView(context: Context) -> SecretHostView<Content> {
        SecretHostView(rootView: content)
    }

    public func updateUIView(_ view: SecretHostView<Content>, context: Context) {
        view.host.rootView = content
    }

    public func sizeThatFits(_ proposal: ProposedViewSize, uiView: Host, context: Context) -> CGSize? {
        uiView.host.sizeThatFits(in: CGSize(width: proposal.width ?? 390, height: proposal.height ?? 844))
    }
}

public final class SecretHostView<Content: View>: UIView {
    let host: UIHostingController<Content>
    private let field = UITextField()

    init(rootView: Content) {
        host = UIHostingController(rootView: rootView)
        super.init(frame: .zero)
        host.view.backgroundColor = .clear
        field.isSecureTextEntry = true
        field.backgroundColor = .clear
        field.translatesAutoresizingMaskIntoConstraints = false
        addSubview(field)
        NSLayoutConstraint.activate([
            field.leadingAnchor.constraint(equalTo: leadingAnchor),
            field.trailingAnchor.constraint(equalTo: trailingAnchor),
            field.topAnchor.constraint(equalTo: topAnchor),
            field.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        // The first subview of a secure field is the layer the system leaves out of captures.
        let canvas = field.subviews.first ?? self
        canvas.isUserInteractionEnabled = true
        host.view.translatesAutoresizingMaskIntoConstraints = false
        canvas.addSubview(host.view)
        NSLayoutConstraint.activate([
            host.view.leadingAnchor.constraint(equalTo: canvas.leadingAnchor),
            host.view.trailingAnchor.constraint(equalTo: canvas.trailingAnchor),
            host.view.topAnchor.constraint(equalTo: canvas.topAnchor),
            host.view.bottomAnchor.constraint(equalTo: canvas.bottomAnchor),
        ])
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError("not used") }
}

/// True while the screen is being recorded, mirrored or AirPlayed.
@MainActor
@Observable
public final class CaptureMonitor {
    public private(set) var isCaptured: Bool

    @ObservationIgnored private nonisolated(unsafe) var observer: (any NSObjectProtocol)?

    public init() {
        isCaptured = UIScreen.main.isCaptured
        observer = NotificationCenter.default.addObserver(
            forName: UIScreen.capturedDidChangeNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.isCaptured = UIScreen.main.isCaptured }
        }
    }

    deinit {
        if let observer { NotificationCenter.default.removeObserver(observer) }
    }
}

/// A plain cover that replaces the app's content in the app switcher and while the screen is captured.
public struct PrivacyCover: View {
    public init() {}

    public var body: some View {
        ZStack {
            FLColor.bg.ignoresSafeArea()
            Image(systemName: "lock.fill")
                .font(.system(size: 44, weight: .semibold))
                .foregroundStyle(FLColor.textTertiary)
                .accessibilityHidden(true)
        }
    }
}

import Shared
import SwiftUI

@main
struct FigLockApp: App {
    @State private var environment = AppEnvironment()
    @State private var capture = CaptureMonitor()
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            RootView(environment: environment)
                .overlay {
                    // The app switcher takes its snapshot while the app is inactive, and a screen recording or
                    // mirror shows everything: cover both (DESIGN.md 8).
                    if scenePhase != .active || capture.isCaptured { PrivacyCover() }
                }
                .onChange(of: scenePhase) { _, phase in
                    switch phase {
                    case .background: environment.lock.appDidEnterBackground()
                    case .active: environment.lock.appWillEnterForeground()
                    default: break
                    }
                }
        }
    }
}

/// Where the app is: unlock screen, or what a locked or unlocked vault shows. The vault screens (FIG-162) and
/// onboarding (FIG-161) replace the two placeholders.
struct RootView: View {
    let environment: AppEnvironment

    var body: some View {
        SecretScreen {
            Group {
                if let error = environment.startupError {
                    Placeholder(title: "Storage unavailable", detail: error)
                } else {
                    switch environment.vault.status() {
                    case .ready:
                        if environment.lock.isUnlocked {
                            UnlockedPlaceholder(lock: environment.lock)
                        } else {
                            LockScreen(lock: environment.lock)
                        }
                    case .noVault:
                        Placeholder(
                            title: "Welcome to FigLock", detail: "Setting up a vault on this iPhone comes next.")
                    case .missing:
                        Placeholder(title: "Vault folder missing", detail: "The vault on this iPhone cannot be found.")
                    case .damaged, .unreadable:
                        Placeholder(
                            title: "Vault cannot be read",
                            detail: "The vault file on this iPhone is damaged or unavailable.")
                    }
                }
            }
        }
        .ignoresSafeArea()
    }
}

private struct Placeholder: View {
    let title: String
    let detail: String

    var body: some View {
        VStack(spacing: FLSpace.s3) {
            Text(title).font(FLFont.ui(FLText.xl2, weight: FLWeight.bold, relativeTo: .title2))
                .foregroundStyle(FLColor.text)
                .accessibilityAddTraits(.isHeader)
            Text(detail).font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary).multilineTextAlignment(
                .center)
        }
        .padding(FLSpace.s6)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(FLColor.bg)
    }
}

private struct UnlockedPlaceholder: View {
    let lock: AppLock

    var body: some View {
        VStack(spacing: FLSpace.s4) {
            Placeholder(title: "Vault unlocked", detail: "Your items appear here.")
            Button("Lock now") { lock.lock() }
                .font(FLFont.ui(FLText.md, weight: FLWeight.semibold))
                .flMinTouchTarget()
        }
        .background(FLColor.bg)
    }
}

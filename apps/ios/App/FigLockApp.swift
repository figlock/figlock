import Shared
import SwiftUI

@main
struct FigLockApp: App {
    @State private var environment = AppEnvironment()
    @State private var capture = CaptureMonitor()
    @Environment(\.scenePhase) private var scenePhase

    init() {
        BackgroundSync.register()
    }

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
                    case .background:
                        environment.lock.appDidEnterBackground()
                        environment.sync.stopForeground()
                        BackgroundSync.schedule()
                    case .active:
                        environment.lock.appWillEnterForeground()
                        environment.sync.startForeground()
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
                            VaultRootView(
                                lock: environment.lock, vault: environment.vault, sync: environment.sync,
                                identitySync: environment.identitySync
                            )
                            .environment(\.settingsContext, environment.settings)
                        } else {
                            LockScreen(lock: environment.lock, environment: environment)
                        }
                    case .noVault:
                        OnboardingRoot(environment: environment)
                    case .missing:
                        RecoveryScreen(
                            service: environment.onboarding, drive: environment.drive, lock: environment.lock)
                    case .damaged, .unreadable:
                        Placeholder(
                            title: "Vault cannot be read",
                            detail: "The vault file on this iPhone is damaged or unavailable.")
                    }
                }
            }
            // Onboarding and recovery change the vault under this view; a new version redraws it.
            .id(environment.lock.vaultVersion)
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

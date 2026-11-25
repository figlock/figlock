import Shared
import SwiftUI

/// The unlock screen (`design/prototypes/ios`, "unlock"): Face ID first, the master password as the fallback.
struct LockScreen: View {
    let lock: AppLock
    let environment: AppEnvironment

    @State private var password = ""
    @State private var secretKey = ""
    @State private var showPassword = false
    @State private var revealPassword = false
    @State private var working = false
    @State private var forgot = false
    @FocusState private var focus: Field?

    private enum Field { case password, secretKey }

    private var quick: QuickUnlock.Status { lock.quickUnlockStatus }
    private var faceIDReady: Bool { quick == .ready }
    private var needsSecretKey: Bool { lock.needsSecretKeyTyped() }
    private var pausedUntil: Date? { lock.passwordPausedUntil }
    private var showPasswordFields: Bool { showPassword || !faceIDReady || pausedUntil != nil }

    var body: some View {
        ScrollView {
            VStack(spacing: FLSpace.s6) {
                header
                VStack(spacing: FLSpace.s4) {
                    if let message { Notice(text: message.text, isError: message.isError) }
                    if faceIDReady { faceIDButton }
                    if showPasswordFields {
                        passwordFields
                    } else {
                        Button("Use password") {
                            showPassword = true
                            focus = .password
                        }
                        .buttonStyle(PlainActionStyle())
                    }
                    Button("Forgot password?") { forgot = true }
                        .buttonStyle(PlainActionStyle())
                }
            }
            .padding(.horizontal, FLSpace.s4)
            .padding(.top, FLSpace.s8 * 2)
        }
        .scrollDismissesKeyboard(.interactively)
        .background(FLColor.bg.ignoresSafeArea())
        .sheet(isPresented: $forgot) { ForgotPasswordView(environment: environment) }
        .task {
            // Face ID is offered the moment the screen shows, as the system apps do.
            if faceIDReady, lock.lastOutcome == nil { await faceID() }
        }
    }

    private var header: some View {
        VStack(spacing: FLSpace.s3) {
            FigLockLogo(size: 88)
            Text("FigLock")
                .font(FLFont.ui(FLText.xl2, weight: FLWeight.bold, relativeTo: .title2))
                .foregroundStyle(FLColor.text)
                .accessibilityAddTraits(.isHeader)
        }
    }

    private var faceIDButton: some View {
        Button {
            Task { await faceID() }
        } label: {
            Label("Unlock with Face ID", systemImage: "faceid")
        }
        .buttonStyle(PrimaryActionStyle())
        .disabled(working)
    }

    private var passwordFields: some View {
        VStack(spacing: FLSpace.s4) {
            if needsSecretKey {
                VStack(alignment: .leading, spacing: FLSpace.s1) {
                    Text("Secret Key").font(FLFont.ui(FLText.sm, weight: FLWeight.medium)).foregroundStyle(
                        FLColor.textSecondary)
                    TextField("FL1-...", text: $secretKey)
                        .textInputAutocapitalization(.characters)
                        .autocorrectionDisabled()
                        .font(FLFont.mono(FLText.md))
                        .focused($focus, equals: .secretKey)
                        .padding(FLSpace.s3)
                        .background(Color(uiColor: .tertiarySystemFill), in: RoundedRectangle(cornerRadius: FLRadius.md))
                                    }
            }
            VStack(alignment: .leading, spacing: FLSpace.s1) {
                Text("Master password").font(FLFont.ui(FLText.sm, weight: FLWeight.medium)).foregroundStyle(
                    FLColor.textSecondary)
                HStack {
                    Group {
                        if revealPassword {
                            TextField("", text: $password)
                        } else {
                            SecureField("", text: $password)
                        }
                    }
                    .textContentType(.password)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .focused($focus, equals: .password)
                    .submitLabel(.go)
                    .onSubmit { Task { await unlock() } }
                    Button {
                        revealPassword.toggle()
                    } label: {
                        Image(systemName: revealPassword ? "eye.slash" : "eye").foregroundStyle(FLColor.textSecondary)
                    }
                    .flMinTouchTarget()
                    .accessibilityLabel(revealPassword ? "Hide master password" : "Show master password")
                }
                .padding(.horizontal, FLSpace.s3)
                .background(Color(uiColor: .tertiarySystemFill), in: RoundedRectangle(cornerRadius: FLRadius.md))
            }
            Button("Unlock") { Task { await unlock() } }
                .buttonStyle(SecondaryActionStyle())
                .disabled(working || password.isEmpty || pausedUntil != nil)
        }
    }

    // MARK: Messages

    private struct Message {
        let text: String
        let isError: Bool
    }

    private var message: Message? {
        if let until = pausedUntil {
            return Message(
                text: "Too many tries. Try again at "
                    + until.roundedUpToMinute.formatted(date: .omitted, time: .shortened)
                    + (faceIDReady ? ", or use Face ID." : "."),
                isError: true)
        }
        switch lock.lastOutcome {
        case .wrongCredentials(let left):
            let tries = left.map { " \($0) \($0 == 1 ? "try" : "tries") left." } ?? ""
            return Message(text: "That master password isn't right." + tries, isError: true)
        case .secretKeyNeeded:
            return Message(text: "Enter your Secret Key to continue.", isError: true)
        case .faceIDNotRecognised(let left):
            return Message(
                text: "Face ID did not work. \(left) \(left == 1 ? "try" : "tries") left.",
                isError: true)
        case .passwordNeeded(let reason):
            return Message(text: Self.reasonText(reason), isError: false)
        case .notSetUp:
            return Message(text: "This iPhone is not set up for this vault.", isError: true)
        case .failed:
            return Message(text: "Could not unlock. Try again.", isError: true)
        default:
            if case .needsPassword(let reason) = quick { return Message(text: Self.reasonText(reason), isError: false) }
            return nil
        }
    }

    private static func reasonText(_ reason: QuickUnlockPolicy.Reason) -> String {
        switch reason {
        case .phoneRestarted: "Enter your master password after a restart."
        case .idleTooLong: "Enter your master password to unlock."
        case .fullUnlockTooOld: "Enter your master password to keep using Face ID."
        case .tooManyFailures: "Face ID failed too often. Enter your master password."
        case .biometricsChanged: "Face ID changed. Enter your master password."
        case .lockedByUser, .passwordChanged: "Enter your master password to unlock."
        }
    }

    // MARK: Actions

    private func faceID() async {
        working = true
        defer { working = false }
        let outcome = await lock.unlockWithFaceID()
        if case .passwordNeeded = outcome { showPassword = true }
        if case .faceIDNotRecognised = outcome { showPassword = true }
    }

    private func unlock() async {
        working = true
        defer { working = false }
        let outcome = await lock.unlock(password: password, secretKey: needsSecretKey ? secretKey : nil)
        if outcome == .unlocked {
            password = ""
            secretKey = ""
        } else {
            // A rejected password never stays in the field (FIG-216), including the attempt that starts the pause
            // (FIG-220). A Secret Key typed beside it is kept: it is long to retype and the message already tells
            // the person which part to check.
            switch outcome {
            case .wrongCredentials, .paused: password = ""
            default: break
            }
            focus = outcome == .secretKeyNeeded ? .secretKey : .password
        }
    }
}

// MARK: Pieces

struct Notice: View {
    let text: String
    let isError: Bool

    var body: some View {
        HStack(alignment: .top, spacing: FLSpace.s3) {
            Image(systemName: isError ? "exclamationmark.triangle.fill" : "info.circle.fill")
                .foregroundStyle(isError ? FLColor.danger : FLColor.primary)
                .accessibilityHidden(true)
            Text(text).font(FLFont.ui(FLText.base, relativeTo: .callout)).foregroundStyle(FLColor.text)
            Spacer(minLength: 0)
        }
        .padding(FLSpace.s3)
        .background(
            isError ? FLColor.dangerSubtle : FLColor.primarySubtle, in: RoundedRectangle(cornerRadius: FLRadius.sm)
        )
        .accessibilityElement(children: .combine)
    }
}

/// Dimmed look shared by the action styles. A `ButtonStyle` cannot read the environment in `makeBody`, so the
/// label is wrapped in a view that reads `\.isEnabled`.
private struct ActionBody<Background: View>: View {
    @Environment(\.isEnabled) private var isEnabled
    let configuration: ButtonStyle.Configuration
    let foreground: Color
    let background: Background

    var body: some View {
        configuration.label
            .font(FLFont.ui(FLText.md, weight: FLWeight.semibold))
            .foregroundStyle(foreground)
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
            .background(background)
            .clipShape(Capsule())
            .opacity(isEnabled ? 1 : 0.4)
            .scaleEffect(configuration.isPressed ? 0.97 : 1)
            .animation(.easeOut(duration: 0.12), value: configuration.isPressed)
            .sensoryFeedback(.impact(weight: .light), trigger: configuration.isPressed) { _, pressed in pressed }
    }
}

struct PrimaryActionStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        ActionBody(
            configuration: configuration, foreground: FLColor.textOnPrimary,
            background: Capsule().fill(FLColor.primary))
    }
}

struct SecondaryActionStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        ActionBody(
            configuration: configuration, foreground: FLColor.text,
            background: Capsule().fill(Color(uiColor: .tertiarySystemFill)))
    }
}

struct PlainActionStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(FLFont.ui(FLText.md, weight: FLWeight.semibold))
            .foregroundStyle(FLColor.textLink)
            .multilineTextAlignment(.center)
            .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
            .opacity(configuration.isPressed ? 0.7 : 1)
    }
}

/// A Label whose icon and title share one colour. In a list row a plain `Label` tints its icon with the accent
/// colour while the title keeps its own, so link rows and destructive rows get a mismatched pair.
struct RowLabelStyle: LabelStyle {
    let color: Color

    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: FLSpace.s3) {
            configuration.icon
            configuration.title
        }
        .foregroundStyle(color)
    }
}

extension Label {
    /// The row look for tappable list rows: link colour, or the danger colour for destructive ones.
    func rowLabel(destructive: Bool = false) -> some View {
        labelStyle(RowLabelStyle(color: destructive ? FLColor.danger : FLColor.textLink))
    }
}

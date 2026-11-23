import Shared
import SwiftUI

/// The unlock screen (`design/prototypes/ios`, "unlock"): Face ID first, the master password as the fallback.
struct LockScreen: View {
    let lock: AppLock

    @State private var password = ""
    @State private var secretKey = ""
    @State private var showPassword = false
    @State private var revealPassword = false
    @State private var working = false
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
                        Button("Use master password") {
                            showPassword = true
                            focus = .password
                        }
                        .buttonStyle(PlainActionStyle())
                    }
                }
            }
            .padding(.horizontal, FLSpace.s4)
            .padding(.top, FLSpace.s8 * 2)
        }
        .scrollDismissesKeyboard(.interactively)
        .background(FLColor.bg.ignoresSafeArea())
        .task {
            // Face ID is offered the moment the screen shows, as the system apps do.
            if faceIDReady, lock.lastOutcome == nil { await faceID() }
        }
    }

    private var header: some View {
        VStack(spacing: FLSpace.s3) {
            Image(systemName: "lock.fill")
                .font(.system(size: 34, weight: .semibold))
                .foregroundStyle(FLColor.textOnPrimary)
                .frame(width: 72, height: 72)
                .background(FLColor.primary, in: RoundedRectangle(cornerRadius: 72 * 0.22))
                .accessibilityHidden(true)
            Text("FigLock is locked")
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
                        .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.sm))
                        .overlay(RoundedRectangle(cornerRadius: FLRadius.sm).stroke(FLColor.border))
                    Text("From your Emergency Kit. This iPhone asks for it once.")
                        .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textTertiary)
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
                .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.sm))
                .overlay(RoundedRectangle(cornerRadius: FLRadius.sm).stroke(FLColor.border))
            }
            Button("Unlock with master password") { Task { await unlock() } }
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
                text: "Password entry is paused. Too many wrong passwords. You can try again at "
                    + until.formatted(date: .omitted, time: .shortened)
                    + (faceIDReady ? ". Face ID still works." : "."),
                isError: true)
        }
        switch lock.lastOutcome {
        case .wrongCredentials(let left):
            let tries =
                left.map { " \($0) \($0 == 1 ? "try" : "tries") left before FigLock pauses password entry." } ?? ""
            return Message(text: "That master password isn't right. Check it and try again." + tries, isError: true)
        case .secretKeyNeeded:
            return Message(text: "Enter your Secret Key from the Emergency Kit to continue.", isError: true)
        case .faceIDNotRecognised(let left):
            return Message(
                text:
                    "Face ID did not recognise you. \(left) \(left == 1 ? "try" : "tries") left, then FigLock asks for your master password.",
                isError: true)
        case .passwordNeeded(let reason):
            return Message(text: Self.reasonText(reason), isError: false)
        case .notSetUp:
            return Message(text: "This iPhone is not set up for this vault yet.", isError: true)
        case .failed:
            return Message(text: "FigLock could not unlock. Try again.", isError: true)
        default:
            if case .needsPassword(let reason) = quick { return Message(text: Self.reasonText(reason), isError: false) }
            return nil
        }
    }

    private static func reasonText(_ reason: QuickUnlockPolicy.Reason) -> String {
        switch reason {
        case .phoneRestarted: "Your iPhone restarted. Enter your master password to unlock."
        case .idleTooLong: "FigLock has not been unlocked for a while. Enter your master password."
        case .fullUnlockTooOld: "It has been a week. Enter your master password to keep Face ID on."
        case .tooManyFailures: "Face ID did not work 5 times. Enter your master password."
        case .biometricsChanged: "Face ID changed on this iPhone. Enter your master password to use it again."
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
            .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
            .background(background)
            .opacity(isEnabled ? (configuration.isPressed ? 0.85 : 1) : 0.4)
    }
}

struct PrimaryActionStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        ActionBody(
            configuration: configuration, foreground: FLColor.textOnPrimary,
            background: RoundedRectangle(cornerRadius: FLRadius.md).fill(FLColor.primary))
    }
}

struct SecondaryActionStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        ActionBody(
            configuration: configuration, foreground: FLColor.text,
            background: RoundedRectangle(cornerRadius: FLRadius.md).fill(FLColor.surfaceSunken)
                .overlay(RoundedRectangle(cornerRadius: FLRadius.md).stroke(FLColor.border)))
    }
}

struct PlainActionStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .font(FLFont.ui(FLText.md, weight: FLWeight.semibold))
            .foregroundStyle(FLColor.textLink)
            .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
            .opacity(configuration.isPressed ? 0.7 : 1)
    }
}

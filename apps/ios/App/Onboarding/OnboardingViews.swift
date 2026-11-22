import Shared
import SwiftUI

/// Entry of onboarding: the welcome choices and every screen behind them (`design/prototypes/ios`).
struct OnboardingRoot: View {
    @State private var model: OnboardingModel

    init(environment: AppEnvironment) {
        _model = State(
            initialValue: OnboardingModel(
                service: environment.onboarding, lock: environment.lock, drive: environment.drive,
                deviceName: UIDevice.current.name))
    }

    var body: some View {
        NavigationStack(path: $model.path) {
            WelcomeView(model: model)
                .navigationDestination(for: OnboardingModel.Route.self) { route in
                    switch route {
                    case .create: CreatePasswordView(model: model)
                    case .secretKey: SecretKeyView(model: model)
                    case .kit: KitView(model: model)
                    case .verify: VerifyView(model: model)
                    case .pairCode: PairCodeView(model: model)
                    case .pairWait: PairWaitView(model: model)
                    case .pairApproved: PairApprovedView(model: model)
                    case .drive: DriveOpenView(model: model)
                    }
                }
        }
        .tint(FLColor.primary)
    }
}

// MARK: Welcome

struct WelcomeView: View {
    let model: OnboardingModel

    var body: some View {
        ScrollView {
            VStack(spacing: FLSpace.s4) {
                Image(systemName: "lock.fill")
                    .font(.system(size: 34, weight: .semibold)).foregroundStyle(FLColor.textOnPrimary)
                    .frame(width: 72, height: 72)
                    .background(FLColor.primary, in: RoundedRectangle(cornerRadius: 72 * 0.22))
                    .accessibilityHidden(true)
                Text("Welcome to FigLock").font(FLFont.ui(FLText.xl3, weight: FLWeight.bold, relativeTo: .title1))
                    .foregroundStyle(FLColor.text).accessibilityAddTraits(.isHeader)
                Text("Passwords, API tokens and files in a vault that only you can open.")
                    .font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary).multilineTextAlignment(.center)
                VStack(spacing: FLSpace.s3) {
                    Choice(
                        icon: "link", title: "Join my vault by pairing",
                        detail: "Pair this iPhone with a device that already has your vault."
                    ) { model.path.append(.pairCode) }
                    Choice(
                        icon: "plus", title: "Create a new vault",
                        detail: "Start fresh with a master password and a Secret Key."
                    ) { model.path.append(.create) }
                    Choice(
                        icon: "cloud", title: "Open my vault from Google Drive",
                        detail: "Sign in to Google Drive and pick the vault you saved there."
                    ) { model.path.append(.drive) }
                }
                .padding(.top, FLSpace.s2)
                Text("You can change where your vault syncs later in Settings.")
                    .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textTertiary)
            }
            .padding(FLSpace.s4)
            .padding(.top, FLSpace.s8)
        }
        .background(FLColor.bg.ignoresSafeArea())
    }
}

private struct Choice: View {
    let icon: String
    let title: String
    let detail: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: FLSpace.s3) {
                Image(systemName: icon).font(.system(size: 22)).foregroundStyle(FLColor.primary)
                    .frame(width: 44, height: 44)
                    .background(FLColor.primarySubtle, in: RoundedRectangle(cornerRadius: FLRadius.sm))
                    .accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(FLFont.ui(FLText.md, weight: FLWeight.semibold)).foregroundStyle(FLColor.text)
                    Text(detail).font(FLFont.ui(FLText.base, relativeTo: .callout)).foregroundStyle(
                        FLColor.textSecondary
                    )
                    .multilineTextAlignment(.leading)
                }
                Spacer(minLength: 0)
                Image(systemName: "chevron.right").foregroundStyle(FLColor.textTertiary).accessibilityHidden(true)
            }
            .padding(FLSpace.s3)
            .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.md))
            .overlay(RoundedRectangle(cornerRadius: FLRadius.md).stroke(FLColor.border))
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .combine)
    }
}

// MARK: Create

struct CreatePasswordView: View {
    @Bindable var model: OnboardingModel
    @State private var reveal = false

    var body: some View {
        Screen(
            title: "Choose a master password", step: "Step 1 of 4",
            subtitle: "It unlocks your vault. FigLock can't reset it, so pick something long that you can remember."
        ) {
            LabeledField(label: "Master password") {
                HStack {
                    Group {
                        if reveal {
                            TextField("", text: $model.password)
                        } else {
                            SecureField("", text: $model.password)
                        }
                    }
                    .textContentType(.newPassword).textInputAutocapitalization(.never).autocorrectionDisabled()
                    Button {
                        reveal.toggle()
                    } label: {
                        Image(systemName: reveal ? "eye.slash" : "eye").foregroundStyle(FLColor.textSecondary)
                    }
                    .flMinTouchTarget()
                    .accessibilityLabel(reveal ? "Hide master password" : "Show master password")
                }
            }
            Text(model.password.count >= 12 ? "Long enough." : "Use at least 12 characters.")
                .font(FLFont.ui(FLText.sm, relativeTo: .footnote))
                .foregroundStyle(model.password.count >= 12 ? FLColor.success : FLColor.textTertiary)
            LabeledField(label: "Confirm master password") {
                SecureField("", text: $model.confirmation).textContentType(.newPassword)
            }
            if !model.confirmation.isEmpty, !model.passwordsMatch {
                Notice(text: "The two passwords don't match. Type the same password in both fields.", isError: true)
            }
            ErrorNotice(model: model)
            Busy(label: "Making your vault", busy: model.busy)
            Button("Continue") { Task { await model.startCreation() } }
                .buttonStyle(PrimaryActionStyle()).disabled(!model.canContinuePassword)
        }
    }
}

struct SecretKeyView: View {
    @Bindable var model: OnboardingModel

    var body: some View {
        Screen(
            title: "Your Secret Key", step: "Step 2 of 4",
            subtitle: "With your master password, it protects your vault. You need both to add a new device."
        ) {
            Notice(text: "Keep this safe, it is shown only once. FigLock can't recover it for you.", isError: false)
            Text(model.secretKeyShown ? model.secretKeyText() : String(repeating: "•", count: 31))
                .font(FLFont.mono(FLText.lg, weight: FLWeight.medium)).foregroundStyle(FLColor.text)
                .frame(maxWidth: .infinity, alignment: .leading).padding(FLSpace.s3)
                .background(FLColor.surfaceSunken, in: RoundedRectangle(cornerRadius: FLRadius.sm))
                .accessibilityLabel(model.secretKeyShown ? "Secret Key" : "Secret Key, hidden")
            Button(model.secretKeyShown ? "Hide Secret Key" : "Show Secret Key") { model.secretKeyShown.toggle() }
                .buttonStyle(SecondaryActionStyle())
            Button("Continue to Emergency Kit") {
                model.secretKeyShown = false
                model.path.append(.kit)
            }
            .buttonStyle(PrimaryActionStyle())
        }
    }
}

struct KitView: View {
    @Bindable var model: OnboardingModel

    var body: some View {
        Screen(
            title: "Save your Emergency Kit", step: "Step 3 of 4",
            subtitle: "A one page PDF with your Secret Key. Store it printed or offline, away from this iPhone."
        ) {
            if let url = model.kitURL {
                ShareLink(item: url) {
                    Label("Save Emergency Kit", systemImage: "square.and.arrow.up")
                }
                .buttonStyle(PrimaryActionStyle())
                .simultaneousGesture(TapGesture().onEnded { model.kitOpened = true })
                if model.kitOpened {
                    Button("I saved it, continue") { model.path.append(.verify) }
                        .buttonStyle(SecondaryActionStyle())
                }
            } else {
                Notice(text: "FigLock could not make the Emergency Kit. Go back and try again.", isError: true)
            }
        }
        .task { model.prepareKit() }
    }
}

struct VerifyView: View {
    @Bindable var model: OnboardingModel

    var body: some View {
        Screen(
            title: "Check that you saved it", step: "Step 4 of 4",
            subtitle:
                "Open your saved Emergency Kit and type the last two groups of the Secret Key, the eight characters after the last but one dash."
        ) {
            LabeledField(label: "Last two groups of your Secret Key") {
                TextField("XXXX-XXXX", text: $model.verification)
                    .textInputAutocapitalization(.characters).autocorrectionDisabled()
                    .font(FLFont.mono(FLText.md))
            }
            FaceIDToggle(model: model)
            ErrorNotice(model: model)
            Busy(label: "Creating vault", busy: model.busy)
            Button("Create vault") { Task { await model.finishCreation() } }
                .buttonStyle(PrimaryActionStyle()).disabled(model.verification.count < 8 || model.busy)
        }
        .onChange(of: model.verification) { _, _ in model.clearError() }
    }
}

// MARK: Pairing

struct PairCodeView: View {
    @Bindable var model: OnboardingModel

    var body: some View {
        Screen(
            title: "Pair this iPhone", step: "Join an existing vault",
            subtitle:
                "On your other device open FigLock, then Settings and Devices, and choose Add device. Type the code it shows."
        ) {
            if model.drive.isSignedIn {
                LabeledField(label: "Pairing code", hint: "20 letters and numbers, like ABCD-EFGH-JKMN-PQRS-TVWX.") {
                    TextField("", text: $model.code)
                        .textInputAutocapitalization(.characters).autocorrectionDisabled()
                        .font(FLFont.mono(FLText.md))
                }
                ErrorNotice(model: model)
                Busy(label: "Sending the request", busy: model.busy)
                Button("Continue") { Task { await model.beginPairing() } }
                    .buttonStyle(PrimaryActionStyle()).disabled(model.code.isEmpty || model.busy)
            } else {
                DriveSignInBlock(model: model)
            }
        }
        .onChange(of: model.code) { _, _ in model.clearError() }
    }
}

struct DriveSignInBlock: View {
    let model: OnboardingModel

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s4) {
            Text("Pairing goes through the Google Drive where your vault is saved. Sign in to the same Google account.")
                .font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary)
            if !model.drive.isConfigured {
                Notice(text: "Google sign-in is not set up in this build yet.", isError: true)
            }
            ErrorNotice(model: model)
            Button {
                Task { await model.signInToDrive() }
            } label: {
                Label("Sign in to Google Drive", systemImage: "person.crop.circle")
            }
            .buttonStyle(PrimaryActionStyle()).disabled(model.busy || !model.drive.isConfigured)
        }
    }
}

struct PairWaitView: View {
    let model: OnboardingModel

    var body: some View {
        Screen(
            title: "Waiting for approval",
            subtitle:
                "On your other device, open FigLock and approve this iPhone. Check that it shows the same code as below."
        ) {
            ProgressView().frame(maxWidth: .infinity)
            Text(model.fingerprint).font(FLFont.mono(FLText.xl, weight: FLWeight.semibold)).foregroundStyle(
                FLColor.text
            )
            .frame(maxWidth: .infinity).padding(FLSpace.s4)
            .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.md))
            .accessibilityLabel("Code to check: \(model.fingerprint)")
            Text("There is no time limit. You can leave this screen and pair later.")
                .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textTertiary)
            Button("Cancel pairing") { model.cancelPairing() }.buttonStyle(SecondaryActionStyle())
        }
        .navigationBarBackButtonHidden()
    }
}

struct PairApprovedView: View {
    @Bindable var model: OnboardingModel

    var body: some View {
        Screen(
            title: "This iPhone is approved",
            subtitle: "Type your master password to open the vault. Your Secret Key came with the approval."
        ) {
            LabeledField(label: "Master password") {
                SecureField("", text: $model.password).textContentType(.password)
            }
            FaceIDToggle(model: model)
            ErrorNotice(model: model)
            Busy(label: "Opening your vault", busy: model.busy)
            Button("Open my vault") { Task { await model.finishPairing() } }
                .buttonStyle(PrimaryActionStyle()).disabled(model.password.isEmpty || model.busy)
        }
        .navigationBarBackButtonHidden()
    }
}

// MARK: Open from Drive

struct DriveOpenView: View {
    @Bindable var model: OnboardingModel

    var body: some View {
        Screen(title: "Open your vault") {
            if !model.drive.isSignedIn {
                DriveSignInBlock(model: model)
            } else if let found = model.foundVault {
                Text("FigLock found one vault in your Google Drive. It was last changed at revision \(found.revision).")
                    .font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary)
                LabeledField(label: "Secret Key", hint: "From your Emergency Kit. Needed once on each new device.") {
                    TextField("FL1-...", text: $model.typedSecretKey)
                        .textInputAutocapitalization(.characters).autocorrectionDisabled()
                        .font(FLFont.mono(FLText.md))
                }
                LabeledField(label: "Master password") {
                    SecureField("", text: $model.password).textContentType(.password)
                }
                FaceIDToggle(model: model)
                ErrorNotice(model: model)
                Busy(label: "Opening your vault", busy: model.busy)
                Button("Open vault") { Task { await model.openFromDrive() } }
                    .buttonStyle(PrimaryActionStyle())
                    .disabled(model.typedSecretKey.isEmpty || model.password.isEmpty || model.busy)
            } else {
                ErrorNotice(model: model)
                Busy(label: "Looking in Google Drive", busy: model.busy)
            }
        }
        .task(id: model.drive.isSignedIn) {
            if model.drive.isSignedIn, model.foundVault == nil { await model.lookForVault() }
        }
    }
}

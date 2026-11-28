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
                FigLockLogo(size: 88)
                Text("Welcome to FigLock").font(FLFont.ui(FLText.xl3, weight: FLWeight.bold, relativeTo: .title1))
                    .foregroundStyle(FLColor.text).accessibilityAddTraits(.isHeader)
                Text("Passwords, tokens and files. Only you can open them.")
                    .font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary).multilineTextAlignment(.center)
                VStack(spacing: FLSpace.s3) {
                    Choice(
                        icon: "link", title: "Join my vault by pairing"
                    ) { model.path.append(.pairCode) }
                    Choice(
                        icon: "plus", title: "Create a new vault"
                    ) { model.path.append(.create) }
                    Choice(
                        icon: "cloud", title: "Open my vault from Google Drive"
                    ) {
                        model.source = .drive
                        model.path.append(.drive)
                    }
                    Choice(
                        icon: "icloud", title: "Open my vault from iCloud Drive"
                    ) {
                        model.source = .icloud
                        model.path.append(.drive)
                    }
                }
                .padding(.top, FLSpace.s2)
            }
            .padding(FLSpace.s4)
            .padding(.top, FLSpace.s8)
            .flFormColumn()
        }
        .background(FLColor.bg.ignoresSafeArea())
    }
}

private struct Choice: View {
    let icon: String
    let title: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: FLSpace.s3) {
                Image(systemName: icon).font(.system(size: 22)).foregroundStyle(FLColor.primary)
                    .frame(width: 44, height: 44)
                    .background(FLColor.primarySubtle, in: RoundedRectangle(cornerRadius: FLRadius.sm))
                    .accessibilityHidden(true)
                Text(title).font(FLFont.ui(FLText.md, weight: FLWeight.semibold)).foregroundStyle(FLColor.text)
                    .multilineTextAlignment(.leading)
                Spacer(minLength: 0)
                Image(systemName: "chevron.right").foregroundStyle(FLColor.textTertiary).accessibilityHidden(true)
            }
            .padding(FLSpace.s3)
            .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.md))
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
            title: "Choose a password", step: "Step 1 of 4",
            subtitle: "FigLock can't reset it. Make it long and memorable."
        ) {
            LabeledField(label: "Password") {
                Group {
                    if reveal {
                        TextField("", text: $model.password)
                    } else {
                        SecureField("", text: $model.password)
                    }
                }
                .textContentType(.newPassword).textInputAutocapitalization(.never).autocorrectionDisabled()
            } trailing: {
                Button {
                    reveal.toggle()
                } label: {
                    Image(systemName: reveal ? "eye.slash" : "eye").foregroundStyle(FLColor.textSecondary)
                }
                .flMinTouchTarget()
                .accessibilityLabel(reveal ? "Hide password" : "Show password")
            }
            Text(
                VaultPasswordRule.isLongEnough(model.password)
                    ? "Long enough." : "Use at least \(VaultPasswordRule.minimumLength) characters."
            )
            .font(FLFont.ui(FLText.sm, relativeTo: .footnote))
            .foregroundStyle(
                VaultPasswordRule.isLongEnough(model.password)
                    ? FLColor.success : model.password.isEmpty ? FLColor.textTertiary : FLColor.danger)
            LabeledField(label: "Confirm password") {
                SecureField("", text: $model.confirmation).textContentType(.newPassword)
            }
            if !model.confirmation.isEmpty, !model.passwordsMatch {
                Notice(text: "The passwords don't match.", isError: true)
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
            subtitle: "You need it, with your password, to add a device."
        ) {
            Notice(text: "Shown once. FigLock can't recover it.", isError: false)
            Text(model.secretKeyShown ? model.secretKeyText() : String(repeating: "•", count: 31))
                .font(FLFont.mono(FLText.lg, weight: FLWeight.medium)).foregroundStyle(FLColor.text)
                .frame(maxWidth: .infinity, alignment: .leading).padding(FLSpace.s3)
                .background(FLColor.surfaceSunken, in: RoundedRectangle(cornerRadius: FLRadius.sm))
                .accessibilityLabel(model.secretKeyShown ? "Secret Key" : "Secret Key, hidden")
            Button(model.secretKeyShown ? "Hide Secret Key" : "Show Secret Key") { model.secretKeyShown.toggle() }
                .buttonStyle(SecondaryActionStyle())
            Button {
                model.copySecretKey()
            } label: {
                Label("Copy Secret Key", systemImage: "doc.on.doc")
            }
            .buttonStyle(SecondaryActionStyle())
            .accessibilityHint("Clears from the clipboard in 30 seconds")
            if model.copier.copiedKey == OnboardingModel.secretKeyCopyKey {
                Text("Copied. The clipboard clears in 30 s")
                    .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.success)
            }
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
            subtitle: "Print it or keep it offline."
        ) {
            if let url = model.kitURL {
                KitShareButton(
                    url: url, onOpen: { model.kitOpened = true },
                    label: { Label("Save Emergency Kit", systemImage: "square.and.arrow.up") }
                )
                .buttonStyle(PrimaryActionStyle())
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
                "Type the last two groups of your Secret Key from your Emergency Kit."
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
                "On your other device, choose Settings, Devices, Add device. Enter the code."
        ) {
            SourcePicker(model: model)
            if model.hasRemote {
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
                RemoteChooser(model: model, purpose: "Pairing goes through")
            }
        }
        .onChange(of: model.code) { _, _ in model.clearError() }
    }
}

struct PairWaitView: View {
    let model: OnboardingModel

    var body: some View {
        Screen(
            title: "Waiting for approval",
            subtitle:
                "Approve this iPhone on your other device. Check the codes match."
        ) {
            ProgressView().frame(maxWidth: .infinity)
            Text(model.fingerprint).font(FLFont.mono(FLText.xl, weight: FLWeight.semibold)).foregroundStyle(
                FLColor.text
            )
            .frame(maxWidth: .infinity).padding(FLSpace.s4)
            .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.md))
            .accessibilityLabel("Code to check: \(model.fingerprint)")
            Text("No time limit. You can pair later.")
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
            subtitle: "Enter your password."
        ) {
            LabeledField(label: "Password") {
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
            if !model.hasRemote {
                RemoteChooser(model: model, purpose: "Your vault is read from")
            } else if let found = model.foundVault {
                Text(
                    "FigLock found one vault in \(model.source == .drive ? "your Google Drive" : "that folder"). It was last changed at revision \(found.revision)."
                )
                .font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary)
                LabeledField(label: "Secret Key", hint: "From your Emergency Kit. Needed once on each new device.") {
                    TextField("FL1-...", text: $model.typedSecretKey)
                        .textInputAutocapitalization(.characters).autocorrectionDisabled()
                        .font(FLFont.mono(FLText.md))
                }
                LabeledField(label: "Password") {
                    SecureField("", text: $model.password).textContentType(.password)
                }
                FaceIDToggle(model: model)
                ErrorNotice(model: model)
                Busy(label: "Opening your vault", busy: model.busy)
                Button("Open vault") { Task { await model.openFromRemote() } }
                    .buttonStyle(PrimaryActionStyle())
                    .disabled(model.typedSecretKey.isEmpty || model.password.isEmpty || model.busy)
            } else {
                ErrorNotice(model: model)
                Busy(
                    label: model.source == .drive ? "Looking in Google Drive" : "Looking in the folder",
                    busy: model.busy)
            }
        }
        .task(id: model.hasRemote) {
            if model.hasRemote, model.foundVault == nil { await model.lookForVault() }
        }
    }
}

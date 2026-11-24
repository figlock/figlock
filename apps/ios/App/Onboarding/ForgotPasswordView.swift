import Shared
import SwiftUI

/// The sheet behind "I forgot my password or Secret Key" (`design/prototypes/ios`, unlock). The desktop's Forgot
/// flow with the Recovery Key as a code: key, new master password, new Emergency Kit, check.
struct ForgotPasswordView: View {
    @State private var model: ForgotPasswordModel
    @Environment(\.dismiss) private var dismiss

    init(environment: AppEnvironment) {
        _model = State(
            initialValue: ForgotPasswordModel(
                service: environment.onboarding, lock: environment.lock, deviceName: UIDevice.current.name))
    }

    var body: some View {
        NavigationStack {
            Group {
                switch model.step {
                case .key: keyStep
                case .password: passwordStep
                case .kit: kitStep
                case .verify: verifyStep
                }
            }
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        model.cancel()
                        dismiss()
                    }
                }
            }
        }
        .interactiveDismissDisabled(model.step == .kit || model.step == .verify)
        .onDisappear { model.cancel() }
    }

    private func notice() -> some View {
        Group { if let text = model.errorText { Notice(text: text, isError: true) } }
    }

    private var keyStep: some View {
        Screen(
            title: "Enter your Recovery Key", step: "Step 1 of 4",
            subtitle:
                "The Recovery Key you saved when you turned it on, on the desktop app. It opens this vault without the master password or the Secret Key."
        ) {
            LabeledField(label: "Recovery Key", hint: "Starts with FLR1, then 14 groups of four characters.") {
                TextField("FLR1-...", text: $model.recoveryKey, axis: .vertical)
                    .textInputAutocapitalization(.characters).autocorrectionDisabled()
                    .font(FLFont.mono(FLText.md)).lineLimit(1...4)
                    .accessibilityLabel("Recovery Key")
            }
            notice()
            Busy(label: "Checking the Recovery Key", busy: model.busy)
            Button("Continue") { Task { await model.findVault() } }
                .buttonStyle(PrimaryActionStyle()).disabled(model.recoveryKey.isEmpty || model.busy)
        }
        .onChange(of: model.recoveryKey) { _, _ in model.clearError() }
    }

    private var passwordStep: some View {
        Screen(
            title: "Choose a new master password", step: "Step 2 of 4",
            subtitle: "The Recovery Key opened your vault. Your items are untouched. Pick a new master password."
        ) {
            LabeledField(label: "New master password") {
                SecureField("", text: $model.password).textContentType(.newPassword)
            }
            Text(
                VaultPasswordRule.isLongEnough(model.password)
                    ? "Long enough." : "Use at least \(VaultPasswordRule.minimumLength) characters."
            )
            .font(FLFont.ui(FLText.sm, relativeTo: .footnote))
            .foregroundStyle(
                VaultPasswordRule.isLongEnough(model.password)
                    ? FLColor.success : model.password.isEmpty ? FLColor.textTertiary : FLColor.danger)
            LabeledField(label: "Confirm new master password") {
                SecureField("", text: $model.confirmation).textContentType(.newPassword)
            }
            if !model.confirmation.isEmpty, !model.passwordsMatch {
                Notice(text: "The two passwords don't match. Type the same password in both fields.", isError: true)
            }
            notice()
            Busy(label: "Making your new Secret Key", busy: model.busy)
            Button("Continue") { Task { await model.choosePassword() } }
                .buttonStyle(PrimaryActionStyle()).disabled(!model.canContinuePassword)
        }
    }

    private var kitStep: some View {
        Screen(
            title: "Save your new Emergency Kit", step: "Step 3 of 4",
            subtitle:
                "Your Secret Key changed too. A one page PDF holds the new one. Store it printed or offline, away from this iPhone. The old kit no longer opens this vault."
        ) {
            if let url = model.kitURL {
                KitShareButton(
                    url: url, onOpen: { model.kitOpened = true },
                    label: { Label("Save Emergency Kit", systemImage: "square.and.arrow.up") }
                )
                .buttonStyle(PrimaryActionStyle())
                if model.kitOpened {
                    Button("I saved it, continue") { model.step = .verify }
                        .buttonStyle(SecondaryActionStyle())
                }
            } else {
                Notice(text: "FigLock could not make the Emergency Kit. Cancel and try again.", isError: true)
            }
        }
    }

    private var verifyStep: some View {
        Screen(
            title: "Check that you saved it", step: "Step 4 of 4",
            subtitle:
                "Open your saved Emergency Kit and type the last two groups of the new Secret Key, the eight characters after the last but one dash. The vault changes only after this check."
        ) {
            LabeledField(label: "Last two groups of your new Secret Key") {
                TextField("XXXX-XXXX", text: $model.verification)
                    .textInputAutocapitalization(.characters).autocorrectionDisabled()
                    .font(FLFont.mono(FLText.md))
            }
            #if DEBUG
                if let groups = model.debugKitGroups {
                    Text(groups).font(.caption2).accessibilityIdentifier("debug-new-key-groups")
                }
            #endif
            notice()
            Busy(label: "Changing your vault", busy: model.busy)
            Button("Change master password and Secret Key") {
                Task { if await model.finish() { dismiss() } }
            }
            .buttonStyle(PrimaryActionStyle()).disabled(model.verification.count < 8 || model.busy)
        }
        .onChange(of: model.verification) { _, _ in model.clearError() }
    }
}

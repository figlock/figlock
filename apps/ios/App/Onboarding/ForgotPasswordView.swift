import Shared
import SwiftUI

/// The sheet behind "I forgot my password or Secret Key" (`design/prototypes/ios`, unlock). The desktop's Forgot
/// flow with the Recovery Key as a code: key, new password, new Emergency Kit, check.
struct ForgotPasswordView: View {
    @State private var model: ForgotPasswordModel
    @State private var confirmingRemoval = false
    @State private var removalFailed = false
    @Environment(\.dismiss) private var dismiss
    private let environment: AppEnvironment

    init(environment: AppEnvironment) {
        self.environment = environment
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
        .alert("Remove this vault from this iPhone?", isPresented: $confirmingRemoval) {
            Button("Cancel", role: .cancel) {}
            Button("Remove vault", role: .destructive) { removeVault() }
        } message: {
            Text(
                "This removes the vault, its keys and Face ID from this iPhone. Your synced copies are not deleted. To add it again you need your Secret Key and password."
            )
        }
        .alert("Could not remove the vault", isPresented: $removalFailed) {
            Button("OK", role: .cancel) {}
        } message: {
            Text("Try again.")
        }
    }

    /// Same as Settings: forget the vault on this iPhone, never touch the copies in the sync places. The app then
    /// shows the vault setup screen.
    private func removeVault() {
        do {
            try environment.onboarding.removeFromThisPhone()
            environment.drive.signOut()
            environment.settings.settings.clear()
            environment.lock.vaultDidChange()
            dismiss()
        } catch {
            removalFailed = true
        }
    }

    private func notice() -> some View {
        Group { if let text = model.errorText { Notice(text: text, isError: true) } }
    }

    private var keyStep: some View {
        Screen(
            title: "Enter your Recovery Key", step: "Step 1 of 4",
            subtitle:
                "The code you saved when you turned it on."
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
            VStack(spacing: FLSpace.s2) {
                Text("I lost my Recovery Key")
                    .font(FLFont.ui(FLText.base, weight: FLWeight.semibold, relativeTo: .callout))
                    .foregroundStyle(FLColor.text)
                Text(
                    "No Recovery Key? You can remove this vault from this iPhone and set up again."
                )
                .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textSecondary)
                .multilineTextAlignment(.center)
                Button("Remove this vault from this iPhone") { confirmingRemoval = true }
                    .buttonStyle(PlainActionStyle()).foregroundStyle(FLColor.danger).disabled(model.busy)
            }
            .padding(.top, FLSpace.s4)
        }
        .onChange(of: model.recoveryKey) { _, _ in model.clearError() }
    }

    private var passwordStep: some View {
        Screen(
            title: "Choose a new password", step: "Step 2 of 4",
            subtitle: "Choose a new password."
        ) {
            LabeledField(label: "New password") {
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
            LabeledField(label: "Confirm new password") {
                SecureField("", text: $model.confirmation).textContentType(.newPassword)
            }
            if !model.confirmation.isEmpty, !model.passwordsMatch {
                Notice(text: "The passwords don't match.", isError: true)
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
                "Your Secret Key changed. Print the new Emergency Kit or keep it offline."
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
                "Type the last two groups of the new Secret Key from your Emergency Kit."
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
            Button("Change password and Secret Key") {
                Task { if await model.finish() { dismiss() } }
            }
            .buttonStyle(PrimaryActionStyle()).disabled(model.verification.count < 8 || model.busy)
        }
        .onChange(of: model.verification) { _, _ in model.clearError() }
    }
}

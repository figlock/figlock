import FigLockFFI
import Shared
import SwiftUI

/// The password, and the Secret Key when this iPhone does not keep it, for a change to the Recovery Key.
struct CredentialsSection: View {
    let model: RecoveryKeyModel

    var body: some View {
        @Bindable var model = model
        Section("Confirm it is you") {
            // No `.password` content type: it makes iOS offer "Save Password?" when the sheet closes, over the list.
            SecureField("Password", text: $model.password)
            if model.needsSecretKey {
                TextField("Secret Key (FL1-...)", text: $model.secretKey)
                    .textInputAutocapitalization(.characters)
                    .autocorrectionDisabled()
            }
        }
        .flTileRows()
    }
}

/// Two steps: show the new code and let the person save it, then check it, ask for the password (and the
/// Secret Key when this iPhone does not keep it) and write it into the vault.
struct SetUpRecoveryKeySheet: View {
    let model: RecoveryKeyModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        @Bindable var model = model
        NavigationStack {
            Form {
                switch model.step {
                case .show: showStep
                case .confirm: confirmStep(verification: $model.verification)
                }
            }
            .scrollContentBackground(.hidden)
            .background(FLColor.bg)
            .navigationTitle(model.hasKey ? "Replace Recovery Key" : "Set up a Recovery Key")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }.disabled(model.busy)
                }
            }
        }
        .interactiveDismissDisabled(model.busy)
    }

    @ViewBuilder private var showStep: some View {
        Section {
            Text(
                model.hasKey
                    ? "Your new Recovery Key. The old one stops working when you finish."
                    : "Your Recovery Key. It opens your vault if you forget your password."
            )
            .font(FLFont.ui(FLText.base, relativeTo: .callout))
            Text(model.code)
                .font(FLFont.mono(FLText.base))
                .foregroundStyle(FLColor.text)
                .textSelection(.disabled)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityLabel("Recovery Key")
        } header: {
            Text("Step 1 of 2: save it").textCase(nil)
        } footer: {
            Text(
                "Shown once. Store it offline, not in this vault. Anyone with it can open your vault."
            )
        }
        .flTileRows()
        Section {
            if let url = model.kitURL {
                KitShareButton(
                    url: url, onOpen: { model.kitOpened = true },
                    label: {
                        Label("Save the Recovery Key page", systemImage: "square.and.arrow.up")
                            .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
                    }
                )
                .buttonStyle(PrimaryActionStyle())
            }
            Button("I saved it, continue") { model.step = .confirm }
                .buttonStyle(SecondaryActionStyle())
                .disabled(model.kitURL != nil && !model.kitOpened)
        }
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
        .listRowInsets(EdgeInsets())
        if let errorText = model.errorText {
            Section { Notice(text: errorText, isError: true).listRowInsets(EdgeInsets()) }
                .listRowBackground(Color.clear)
        }
    }

    @ViewBuilder private func confirmStep(verification: Binding<String>) -> some View {
        @Bindable var model = model
        Section {
            TextField("XXXX-XXXX", text: verification)
                .textInputAutocapitalization(.characters)
                .autocorrectionDisabled()
                .font(FLFont.mono(FLText.md))
                .accessibilityLabel("Last two groups of the Recovery Key")
            #if DEBUG
                if let tail = model.debugTail {
                    Text(tail).font(.caption2).accessibilityIdentifier("debug-recovery-tail")
                }
            #endif
        } header: {
            Text("Step 2 of 2: check it").textCase(nil)
        } footer: {
            Text(
                "Type the last two groups of your Recovery Key."
            )
        }
        .flTileRows()
        CredentialsSection(model: model)
        if let errorText = model.errorText {
            Section { Notice(text: errorText, isError: true).listRowInsets(EdgeInsets()) }
                .listRowBackground(Color.clear)
        }
        Section {
            Button {
                Task { if await model.apply() { dismiss() } }
            } label: {
                HStack {
                    if model.busy { ProgressView() }
                    Text(model.hasKey ? "Replace Recovery Key" : "Turn on Recovery Key")
                }
                .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
            }
            .buttonStyle(PrimaryActionStyle())
            .disabled(!model.canApply)
            Button("Back") { model.step = .show }
                .buttonStyle(PlainActionStyle())
                .disabled(model.busy)
        }
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
        .listRowInsets(EdgeInsets())
    }
}

/// Turning it off asks for the password (and the Secret Key when this iPhone does not keep it): it is a
/// downgrade, and removing the key needs no secret of the vault, so the vault checks the credentials first.
struct RemoveRecoveryKeySheet: View {
    let model: RecoveryKeyModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        @Bindable var model = model
        NavigationStack {
            Form {
                Section {
                    Text(
                        "Without a Recovery Key, a forgotten password cannot be recovered. The Recovery Key you saved opens nothing from now on."
                    )
                    .font(FLFont.ui(FLText.base, relativeTo: .callout))
                }
                CredentialsSection(model: model)
                if let errorText = model.errorText {
                    Section { Notice(text: errorText, isError: true).listRowInsets(EdgeInsets()) }
                        .listRowBackground(Color.clear)
                }
                Section {
                    Button(role: .destructive) {
                        Task { if await model.remove() { dismiss() } }
                    } label: {
                        HStack {
                            if model.busy { ProgressView() }
                            Text("Turn off the Recovery Key")
                        }
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
                    }
                    .disabled(model.password.isEmpty || (model.needsSecretKey && model.secretKey.isEmpty) || model.busy)
                }
            }
            .scrollContentBackground(.hidden)
            .background(FLColor.bg)
            .navigationTitle("Turn off Recovery Key")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }.disabled(model.busy)
                }
            }
        }
        .interactiveDismissDisabled(model.busy)
    }
}

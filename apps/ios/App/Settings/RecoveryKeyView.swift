import FigLockFFI
import Shared
import SwiftUI

/// The Recovery Key in Settings (ADR-0029): whether the vault has one, turning it on or replacing it, and turning it
/// off. The code is made here, shown once, checked by typing its last two groups, and only then written into the
/// vault, so backing out at any point leaves the vault as it was. FigLock never keeps it and never shows an old one.
@MainActor
@Observable
final class RecoveryKeyModel {
    enum Step { case show, confirm }

    let context: SettingsContext
    private(set) var hasKey = false
    private(set) var draft: RecoveryDraft?
    private(set) var kitURL: URL?
    private(set) var busy = false
    var step = Step.show
    var kitOpened = false
    var verification = ""
    var password = ""
    var secretKey = ""
    var errorText: String?
    /// What the last change said, for the list to show once.
    var notice: String?

    init(context: SettingsContext) {
        self.context = context
        reload()
    }

    var needsSecretKey: Bool { context.service.keptSecretKey() == nil }

    func reload() {
        guard let session = context.lock.session else { return }
        hasKey = context.service.hasRecoveryKey(session: session)
    }

    // MARK: Turn on or replace

    /// Make the new code to show. Nothing about the vault changes yet.
    func begin() {
        cancel()
        draft = try? newRecoveryDraft()
        guard let draft else {
            errorText = "FigLock could not make a Recovery Key. Try again."
            return
        }
        if let pdf = try? context.service.recoveryKitPage(draft, storage: context.storageLabel) {
            let url = FileManager.default.temporaryDirectory.appendingPathComponent("FigLock Recovery Key.pdf")
            try? pdf.write(to: url, options: [.atomic, .completeFileProtectionUntilFirstUserAuthentication])
            kitURL = url
        }
    }

    var code: String { draft?.code() ?? "" }

    var tailMatches: Bool { draft?.verifyTail(typed: verification) ?? false }

    var canApply: Bool {
        tailMatches && !password.isEmpty && (!needsSecretKey || !secretKey.isEmpty) && !busy
    }

    /// Write the confirmed code into the vault. Returns whether the sheet can close.
    func apply() async -> Bool {
        guard let draft, let session = context.lock.session else { return false }
        guard tailMatches else {
            errorText =
                "That doesn't match the last two groups of the Recovery Key. Check what you saved and try again."
            return false
        }
        let replacing = hasKey
        return await change(
            done: replacing
                ? "Your Recovery Key was replaced. The old one no longer works." : "Your Recovery Key is on."
        ) {
            try await context.service.setRecoveryKey(
                draft, password: password, typedSecretKey: needsSecretKey ? secretKey : nil, session: session)
        }
    }

    // MARK: Turn off

    func remove() async -> Bool {
        guard let session = context.lock.session else { return false }
        return await change(done: "Your Recovery Key is off. The old code opens nothing from now on.") {
            try await context.service.removeRecoveryKey(
                password: password, typedSecretKey: needsSecretKey ? secretKey : nil, session: session)
        }
    }

    private func change(done message: String, _ work: () async throws -> Void) async -> Bool {
        busy = true
        errorText = nil
        defer { busy = false }
        do {
            try await work()
        } catch let failure as SettingsService.Failure {
            switch failure {
            case .wrongCredentials: errorText = "That master password or Secret Key is not right."
            case .secretKeyNeeded: errorText = "Type your Secret Key. It starts with FL1-."
            case .changeUncertain:
                // The vault took the change but the write failed: lock, so the next unlock reads what is stored.
                cancel()
                context.lock.lock()
            default: errorText = "FigLock could not change the Recovery Key. Nothing was changed."
            }
            return false
        } catch {
            errorText = "FigLock could not change the Recovery Key. Nothing was changed."
            return false
        }
        cancel()
        reload()
        notice = message
        await context.sync.syncNow(force: true)
        return true
    }

    /// Drop the code, the typed secrets and the page file.
    func cancel() {
        draft = nil
        step = .show
        kitOpened = false
        verification = ""
        password = ""
        secretKey = ""
        errorText = nil
        if let kitURL { try? FileManager.default.removeItem(at: kitURL) }
        kitURL = nil
    }

    #if DEBUG
        /// For the UI test only (`-figlock-expose-new-key`): the last two groups of the code being shown.
        var debugTail: String? {
            guard ProcessInfo.processInfo.arguments.contains("-figlock-expose-new-key"), draft != nil else {
                return nil
            }
            return code.split(separator: "-").suffix(2).joined(separator: "-")
        }
    #endif
}

struct RecoveryKeyView: View {
    @State private var model: RecoveryKeyModel
    @State private var settingUp = false
    @State private var removing = false

    init(context: SettingsContext) {
        _model = State(initialValue: RecoveryKeyModel(context: context))
    }

    var body: some View {
        List {
            if let notice = model.notice {
                Notice(text: notice, isError: false).listRowBackground(Color.clear).listRowInsets(EdgeInsets())
            }
            Section {
                SettingsRow(
                    symbol: model.hasKey ? "checkmark.shield" : "shield.slash", title: model.hasKey ? "On" : "Off",
                    detail: model.hasKey
                        ? "A Recovery Key can open this vault if you forget your master password or lose your Secret Key."
                        : "Without one, a forgotten master password cannot be recovered.")
            } header: {
                Text("Recovery Key").textCase(nil)
            } footer: {
                Text(
                    "FigLock does not keep your Recovery Key and cannot show it again. If you lost it, replace it with a new one."
                )
            }
            .listRowBackground(FLColor.surface)

            Section {
                Button {
                    model.notice = nil
                    model.begin()
                    settingUp = true
                } label: {
                    Label(
                        model.hasKey ? "Replace Recovery Key" : "Set up a Recovery Key",
                        systemImage: "arrow.triangle.2.circlepath"
                    )
                    .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                }
                if model.hasKey {
                    Button(role: .destructive) {
                        model.notice = nil
                        removing = true
                    } label: {
                        Label("Turn off the Recovery Key", systemImage: "xmark.shield")
                            .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                    }
                    .tint(FLColor.danger)
                }
            }
            .listRowBackground(FLColor.surface)
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(FLColor.bg)
        .navigationTitle("Recovery Key")
        .navigationBarTitleDisplayMode(.inline)
        .onAppear { model.reload() }
        .sheet(
            isPresented: $settingUp, onDismiss: { model.cancel() },
            content: { SetUpRecoveryKeySheet(model: model) }
        )
        .sheet(
            isPresented: $removing, onDismiss: { model.cancel() },
            content: { RemoveRecoveryKeySheet(model: model) })
    }
}

/// The master password, and the Secret Key when this iPhone does not keep it, for a change to the Recovery Key.
private struct CredentialsSection: View {
    let model: RecoveryKeyModel

    var body: some View {
        @Bindable var model = model
        Section("Confirm it is you") {
            SecureField("Master password", text: $model.password).textContentType(.password)
            if model.needsSecretKey {
                TextField("Secret Key (FL1-...)", text: $model.secretKey)
                    .textInputAutocapitalization(.characters)
                    .autocorrectionDisabled()
            }
        }
        .listRowBackground(FLColor.surface)
    }
}

/// Two steps: show the new code and let the person save it, then check it, ask for the master password (and the
/// Secret Key when this iPhone does not keep it) and write it into the vault.
private struct SetUpRecoveryKeySheet: View {
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
                    ? "This is your new Recovery Key. It replaces the old one, which stops working once you finish."
                    : "This is your Recovery Key. It opens your vault if you forget your master password."
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
                "FigLock shows it only now and does not keep it. Store it printed or offline, away from this iPhone and not in this vault. Anyone with it can open your vault."
            )
        }
        .listRowBackground(FLColor.surface)
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
                "Open what you saved and type the last two groups of the Recovery Key, the eight characters after the last but one dash. The vault changes only after this check."
            )
        }
        .listRowBackground(FLColor.surface)
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

/// Turning it off asks for the master password (and the Secret Key when this iPhone does not keep it): it is a
/// downgrade, and removing the key needs no secret of the vault, so the vault checks the credentials first.
private struct RemoveRecoveryKeySheet: View {
    let model: RecoveryKeyModel
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        @Bindable var model = model
        NavigationStack {
            Form {
                Section {
                    Text(
                        "Without a Recovery Key, a forgotten master password cannot be recovered. The Recovery Key you saved opens nothing from now on."
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

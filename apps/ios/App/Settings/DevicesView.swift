import Shared
import SwiftUI

/// What the Devices screens hold: the rows, and the one revocation that may be running.
@MainActor
@Observable
final class DevicesModel {
    let context: SettingsContext
    private(set) var rows: [DeviceRow] = []
    private(set) var busy = false
    /// What the last revocation said, for the screen to show once.
    var revokedNotice: String?

    init(context: SettingsContext) {
        self.context = context
        reload()
    }

    func reload() {
        guard let session = context.lock.session, let entries = try? session.devices() else {
            rows = []
            return
        }
        rows = DeviceList.rows(entries: entries, nicknames: context.settings.nicknames)
    }

    func row(id: String) -> DeviceRow? { rows.first { $0.id == id } }

    /// Name a device on this iPhone; an empty name takes the nickname away.
    func rename(_ id: String, to name: String) {
        try? context.settings.setNickname(name, for: id)
        reload()
    }

    /// Revoke `id`. Returns the words for a failure, or nil when it worked.
    func revoke(_ id: String, password: String, secretKey: String?) async -> String? {
        guard let session = context.lock.session else { return "FigLock is locked. Unlock it and try again." }
        busy = true
        defer { busy = false }
        do {
            _ = try await context.service.revoke(
                deviceId: id, password: password, typedSecretKey: secretKey, session: session,
                remote: nil)
            // Each remote gets the new vault.json on its own, so one failing never holds back another; the notice says so
            // unless every one took it.
            await context.sync.syncNow(force: true)
            let rows = context.sync.rows
            let pushed = !rows.isEmpty && rows.allSatisfy { $0.state.phase == .synced }
            reload()
            let name = row(id: id)?.displayName ?? "The device"
            revokedNotice =
                pushed
                ? "\(name) is revoked."
                : "\(name) is revoked on this iPhone. It loses access everywhere once this iPhone syncs."
            return nil
        } catch let failure as SettingsService.Failure {
            switch failure {
            case .wrongCredentials: return "That password or Secret Key is not right."
            case .secretKeyNeeded: return "Type your Secret Key. It starts with FL1-."
            case .notAllowed: return "This device cannot be revoked from here."
            default: return "FigLock could not revoke the device. Nothing was changed."
            }
        } catch {
            return "FigLock could not revoke the device. Nothing was changed."
        }
    }
}

struct DevicesView: View {
    @State private var model: DevicesModel

    init(context: SettingsContext) {
        _model = State(initialValue: DevicesModel(context: context))
    }

    var body: some View {
        List {
            if let notice = model.revokedNotice {
                Notice(text: notice, isError: false).listRowBackground(Color.clear).listRowInsets(EdgeInsets())
            }
            Section {
                ForEach(model.rows.filter { !$0.isRevoked }) { row in
                    NavigationLink {
                        DeviceDetailView(model: model, id: row.id)
                    } label: {
                        DeviceRowView(row: row)
                    }
                }
            } header: {
                FLSectionHeader("Devices that can open your vault")
            } footer: {
                Text("To add a device, choose Join my vault by pairing on it.")
            }
            .flTileRows()

            let revoked = model.rows.filter(\.isRevoked)
            if !revoked.isEmpty {
                Section {
                    ForEach(revoked) { row in DeviceRowView(row: row) }
                } header: {
                    FLSectionHeader("Revoked")
                }
                .flTileRows()
            }
        }
        .flTileList()
        .navigationTitle("Devices")
        .onAppear { model.reload() }
    }
}

private struct DeviceRowView: View {
    let row: DeviceRow

    var body: some View {
        HStack(spacing: FLSpace.s3) {
            FLIconTile(symbol: "iphone", color: row.isRevoked ? FLColor.textTertiary : FLColor.primary)
            VStack(alignment: .leading, spacing: 2) {
                Text(row.displayName)
                    .font(FLFont.ui(FLRowText.title))
                    .foregroundStyle(row.isRevoked ? FLColor.textSecondary : FLColor.text)
                if let subtitle {
                    Text(subtitle)
                        .font(FLFont.ui(FLRowText.subtitle, relativeTo: .subheadline))
                        .foregroundStyle(FLColor.textSecondary)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: FLSpace.s2)
            if row.isThisDevice {
                Text("This iPhone")
                    .font(FLFont.ui(FLRowText.subtitle, weight: FLWeight.medium, relativeTo: .subheadline))
                    .foregroundStyle(FLColor.success)
            }
        }
        .padding(.vertical, 6)
        .frame(minHeight: FLSize.controlHeightLg)
        .accessibilityElement(children: .combine)
    }

    /// The name is what a person goes by; the fingerprint is for the detail screen, not the list.
    private var subtitle: String? {
        guard let revokedAt = row.revokedAt else { return nil }
        return "Revoked \(revokedAt.formatted(date: .abbreviated, time: .omitted))"
    }
}

struct DeviceDetailView: View {
    let model: DevicesModel
    let id: String

    @State private var renaming = false
    @State private var newName = ""
    @State private var revoking = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        Group {
            if let row = model.row(id: id), !row.isRevoked {
                List {
                    Section {
                        LabeledContent("Name", value: row.displayName)
                        if let vaultName = row.vaultName, row.nickname != nil {
                            LabeledContent("Name in the vault", value: vaultName)
                        }
                        LabeledContent("Fingerprint") {
                            Text(row.fingerprint).font(FLFont.mono(FLText.sm)).textSelection(.enabled)
                        }
                    }
                    .flTileRows()

                    if row.isThisDevice {
                        Section {
                            Label(
                                "This is the iPhone you are holding. To take it off your vault, use Remove this vault from this iPhone in Settings.",
                                systemImage: "info.circle"
                            )
                            .font(FLFont.ui(FLText.base, relativeTo: .callout))
                            .foregroundStyle(FLColor.textSecondary)
                        }
                        .flTileRows()
                    }

                    Section {
                        Button {
                            newName = row.displayName
                            renaming = true
                        } label: {
                            Label("Rename", systemImage: "pencil")
                                .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                        }
                        if !row.isThisDevice {
                            Button(role: .destructive) {
                                revoking = true
                            } label: {
                                Label("Revoke access", systemImage: "xmark.shield")
                                    .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                            }
                            .tint(FLColor.danger)
                        }
                    } footer: {
                        Text("The new name shows on this iPhone only.")
                    }
                    .flTileRows()
                }
                .listStyle(.insetGrouped)
                .scrollContentBackground(.hidden)
                .alert("Rename device", isPresented: $renaming) {
                    TextField("Device name", text: $newName)
                    Button("Cancel", role: .cancel) {}
                    Button("Save") { model.rename(id, to: newName) }
                } message: {
                    Text("Leave empty to use the original name.")
                }
                .sheet(isPresented: $revoking) {
                    RevokeSheet(model: model, row: row)
                }
            } else {
                // Revoked from here, or gone: nothing left to manage.
                Text("This device is no longer in the list.")
                    .font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .background(FLColor.bg)
        // A revoked device has nothing left to manage: go back to the list, which shows it as revoked.
        .onChange(of: model.row(id: id)?.isRevoked ?? true) { _, gone in
            if gone { dismiss() }
        }
        .navigationTitle(model.row(id: id)?.displayName ?? "Device")
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// The confirmation for a revocation. It asks for the password (and the Secret Key when this iPhone does not
/// keep it), because a revocation rewrites the vault's keys and the vault checks them (ADR-0018).
private struct RevokeSheet: View {
    let model: DevicesModel
    let row: DeviceRow

    @Environment(\.dismiss) private var dismiss
    @State private var password = ""
    @State private var secretKey = ""
    @State private var errorText: String?

    private var needsSecretKey: Bool { model.context.service.keptSecretKey() == nil }
    private var canRevoke: Bool {
        !password.isEmpty && (!needsSecretKey || !secretKey.isEmpty) && !model.busy
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text(
                        "\(row.displayName) will lose access to \(SettingsService.vaultName) the next time it syncs. Items already on it cannot be erased remotely. It may still know your password and Secret Key."
                    )
                    .font(FLFont.ui(FLText.base, relativeTo: .callout))
                }
                Section("Confirm it is you") {
                    SecureField("Password", text: $password)
                        .textContentType(.password)
                    if needsSecretKey {
                        TextField("Secret Key (FL1-...)", text: $secretKey)
                            .textInputAutocapitalization(.characters)
                            .autocorrectionDisabled()
                    }
                }
                if let errorText {
                    Section { Notice(text: errorText, isError: true).listRowInsets(EdgeInsets()) }
                        .listRowBackground(Color.clear)
                }
                Section {
                    Button(role: .destructive) {
                        Task {
                            errorText = await model.revoke(
                                row.id, password: password, secretKey: needsSecretKey ? secretKey : nil)
                            if errorText == nil { dismiss() }
                        }
                    } label: {
                        HStack {
                            if model.busy { ProgressView() }
                            Text("Revoke \(row.displayName)")
                        }
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
                    }
                    .disabled(!canRevoke)
                }
            }
            .scrollContentBackground(.hidden)
            .background(FLColor.bg)
            .navigationTitle("Revoke device")
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

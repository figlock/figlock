import Shared
import SwiftUI

/// Settings > Export (FIG-288): the vault as a JSON file for the Files app or the share sheet. Without secrets by
/// default; including them needs Face ID or the passcode every time, and the temporary file is removed when the
/// share sheet closes.
struct ExportView: View {
    let context: SettingsContext

    @State private var exporter = VaultExporter(authenticator: ExportView.authenticator())
    @State private var includeSecrets = false
    @State private var includeDeleted = false
    @State private var working = false
    @State private var message: Message?
    @State private var sharing = false
    @Environment(\.scenePhase) private var scenePhase

    /// Face ID or the passcode. Debug builds take `-figlock-allow-device-auth` or `-figlock-deny-device-auth` so the UI
    /// test does not depend on what the simulator says about biometrics.
    private static func authenticator() -> any Authenticating {
        #if DEBUG
            let arguments = ProcessInfo.processInfo.arguments
            if arguments.contains("-figlock-allow-device-auth") { return FixedAuthenticator(allow: true) }
            if arguments.contains("-figlock-deny-device-auth") { return FixedAuthenticator(allow: false) }
        #endif
        return DeviceAuthenticator()
    }

    private struct Message {
        let text: String
    }

    var body: some View {
        List {
            Section {
                Toggle("Include recently deleted", isOn: $includeDeleted)
                    .accessibilityIdentifier("export-include-deleted")
                Toggle("Include passwords and other secrets", isOn: $includeSecrets)
                    .accessibilityIdentifier("export-include-secrets")
            } header: {
                Text("What to include").textCase(nil)
            } footer: {
                Text(
                    includeSecrets
                        ? "Secrets are written in plain text. Anyone who gets the file can read them. Face ID or your passcode is asked every time."
                        : "Names, usernames, websites and notes. Passwords and other secrets are left out."
                )
            }
            .flTileRows()

            Section {
                Button {
                    Task { await export() }
                } label: {
                    Label(working ? "Exporting..." : "Export", systemImage: "square.and.arrow.up")
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
                }
                .buttonStyle(PrimaryActionStyle())
                .disabled(working)
                .accessibilityIdentifier("export-button")
            }
            .listRowBackground(Color.clear)
            .listRowInsets(EdgeInsets())

            if let message {
                Notice(text: message.text, isError: true)
                    .listRowBackground(Color.clear)
                    .accessibilityIdentifier("export-message")
            }
        }
        .flTileList()
        .navigationTitle("Export")
        .navigationBarTitleDisplayMode(.inline)
        .sheet(isPresented: $sharing) {
            if let url = exporter.file {
                KitShareSheet(
                    url: url,
                    onFinish: {
                        sharing = false
                        exporter.cleanUp()
                    }
                )
                .presentationDetents([.medium, .large]).ignoresSafeArea()
            }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .background { exporter.cleanUpForScenePhase(isBackground: true) }
        }
        .onDisappear { exporter.cleanUp() }
    }

    private func export() async {
        guard let session = context.lock.session else { return }
        message = nil
        working = true
        defer { working = false }
        let outcome = await exporter.prepare(
            session: session, includeSecrets: includeSecrets, includeDeleted: includeDeleted)
        switch outcome {
        case .ready: sharing = true
        case .notConfirmed:
            message = Message(text: "Nothing was exported because Face ID or your passcode was not confirmed.")
        case .failed: message = Message(text: "The export could not be made. Try again.")
        }
    }
}

#if DEBUG
    private struct FixedAuthenticator: Authenticating {
        let allow: Bool
        func authenticate(reason: String) async -> Bool { allow }
    }
#endif

import Shared
import SwiftUI

/// Turning Face ID on after onboarding, or off (ADR-0046). AutoFill needs it, because the extension unlocks only
/// with Face ID. Turning it on asks for the master password, since the vault key is wrapped to a new hardware key.
struct FaceIDView: View {
    let context: SettingsContext

    @State private var isOn: Bool
    @State private var askingForPassword = false

    init(context: SettingsContext) {
        self.context = context
        _isOn = State(initialValue: context.lock.quickUnlockIsWanted)
    }

    /// The value shown on the Settings row.
    static func summary(_ lock: AppLock) -> String {
        guard lock.quickUnlockIsAvailable else { return "Unavailable" }
        return lock.quickUnlockIsWanted ? "On" : "Off"
    }

    var body: some View {
        List {
            Section {
                if context.lock.quickUnlockIsAvailable {
                    Text(isOn ? "Face ID is on" : "Face ID is off").foregroundStyle(FLColor.textSecondary)
                    Button {
                        toggleFaceID()
                    } label: {
                        Text(isOn ? "Turn off Face ID" : "Turn on Face ID")
                            .font(FLFont.ui(FLText.md, weight: FLWeight.medium))
                            .frame(minHeight: FLSize.controlHeightLg, alignment: .leading)
                    }
                    .accessibilityIdentifier("face-id-button")
                } else {
                    Text("Face ID is unavailable").foregroundStyle(FLColor.textSecondary)
                }
            } footer: {
                Text(footer)
            }
            .listRowBackground(FLColor.surface)
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .navigationTitle("Face ID")
        .navigationBarTitleDisplayMode(.inline)
        .sheet(isPresented: $askingForPassword) {
            FaceIDPasswordSheet(context: context) { isOn = true }
        }
    }

    private var footer: String {
        guard context.lock.quickUnlockIsAvailable else {
            return
                "This iPhone has no Face ID or Touch ID set up. Set one up in the iOS Settings app, then come back here. AutoFill needs it."
        }
        return
            "Face ID unlocks FigLock and fills passwords from AutoFill. If a face is added or removed in iOS, you enter your master password once to turn it back on."
    }

    private func toggleFaceID() {
        if isOn {
            context.lock.turnOffFaceID()
            isOn = false
        } else {
            askingForPassword = true
        }
    }
}

/// Asks for the master password to make the new Face ID key.
private struct FaceIDPasswordSheet: View {
    let context: SettingsContext
    let turnedOn: () -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var password = ""
    @State private var failed = false
    @State private var working = false

    var body: some View {
        NavigationStack {
            List {
                Section {
                    // No `.password` content type: it makes iOS offer "Save Password?" when the sheet closes.
                    SecureField("Master password", text: $password)
                } header: {
                    Text("Confirm it is you")
                } footer: {
                    if failed { Text("That did not work. Check your master password and try again.") }
                }
                .listRowBackground(FLColor.surface)

                Section {
                    Button {
                        Task { await turnOn() }
                    } label: {
                        Text(working ? "Turning on" : "Turn on Face ID")
                    }
                    .disabled(password.isEmpty || working)
                }
                .listRowBackground(FLColor.surface)
            }
            .scrollContentBackground(.hidden)
            .navigationTitle("Turn on Face ID")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() } }
            }
        }
    }

    private func turnOn() async {
        working = true
        failed = false
        let done = await context.onboarding.turnOnFaceID(password: password)
        working = false
        password = ""
        if done {
            turnedOn()
            dismiss()
        } else {
            failed = true
        }
    }
}

import Shared
import SwiftUI

/// Sync settings: Google Drive sign-out and the separate, typed step that deletes the Drive copy (ADR-0035). Signing
/// out never deletes anything; the copy in Drive stays until the person types the phrase.
struct SyncSettingsView: View {
    let context: SettingsContext

    @State private var confirmingSignOut = false
    @State private var deleting = false
    @State private var notice: String?

    var body: some View {
        List {
            if let notice {
                Notice(text: notice, isError: false).listRowBackground(Color.clear).listRowInsets(EdgeInsets())
            }
            Section {
                SettingsRow(
                    symbol: "globe", title: "Google Drive",
                    value: context.drive.isSignedIn ? "Signed in" : "Not signed in")
                if context.drive.isSignedIn {
                    Button(role: .destructive) {
                        confirmingSignOut = true
                    } label: {
                        Label("Sign out of Google Drive", systemImage: "rectangle.portrait.and.arrow.right")
                            .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                    }
                    .tint(FLColor.danger)
                } else if !context.drive.isConfigured {
                    Text("Google sign-in is not set up in this build.")
                        .font(FLFont.ui(FLText.base, relativeTo: .callout)).foregroundStyle(FLColor.textSecondary)
                }
            } footer: {
                Text("FigLock uploads only the encrypted vault, to its own hidden folder in your Drive.")
            }
            .listRowBackground(FLColor.surface)

            if context.drive.isSignedIn {
                Section {
                    Button(role: .destructive) {
                        deleting = true
                    } label: {
                        Label("Delete from Google Drive", systemImage: "trash")
                            .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                    }
                    .tint(FLColor.danger)
                } footer: {
                    Text(
                        "Deletes this vault's encrypted copy from Google Drive. The vault on this iPhone and on your other devices is not touched."
                    )
                }
                .listRowBackground(FLColor.surface)
            }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(FLColor.bg)
        .navigationTitle("Sync")
        .navigationBarTitleDisplayMode(.inline)
        .alert("Sign out of Google Drive?", isPresented: $confirmingSignOut) {
            Button("Cancel", role: .cancel) {}
            Button("Sign out", role: .destructive) {
                context.drive.signOut()
                notice = "Signed out of Google Drive."
            }
        } message: {
            Text(
                "This iPhone stops syncing with Google Drive until you sign in again. Your vault stays on this iPhone, and the copy in Google Drive is not deleted."
            )
        }
        .sheet(isPresented: $deleting) {
            DeleteFromDriveSheet(context: context) { notice = $0 }
        }
    }
}

/// The typed confirmation of ADR-0035. The button stays off until the phrase is typed.
private struct DeleteFromDriveSheet: View {
    let context: SettingsContext
    let finished: (String) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var typed = ""
    @State private var busy = false
    @State private var errorText: String?

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Text(
                        "This deletes the encrypted copy of \(SettingsService.vaultName) from Google Drive. Your other devices keep their copies and will ask before they upload it again. The vault on this iPhone, your keys and your Google sign-in stay."
                    )
                    .font(FLFont.ui(FLText.base, relativeTo: .callout))
                }
                Section {
                    TextField(SettingsService.deleteFromDrivePhrase, text: $typed)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                } header: {
                    Text("Type \u{201C}\(SettingsService.deleteFromDrivePhrase)\u{201D} to continue").textCase(nil)
                }
                if let errorText {
                    Section { Notice(text: errorText, isError: true).listRowInsets(EdgeInsets()) }
                        .listRowBackground(Color.clear)
                }
                Section {
                    Button(role: .destructive) {
                        Task { await delete() }
                    } label: {
                        HStack {
                            if busy { ProgressView() }
                            Text("Delete from Google Drive")
                        }
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
                    }
                    .disabled(!SettingsService.isDeleteFromDrivePhrase(typed) || busy)
                }
            }
            .scrollContentBackground(.hidden)
            .background(FLColor.bg)
            .navigationTitle("Delete from Google Drive")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("Cancel") { dismiss() }.disabled(busy) }
            }
        }
        .interactiveDismissDisabled(busy)
    }

    private func delete() async {
        guard let remote = context.remote() else {
            errorText = "Sign in to Google Drive first."
            return
        }
        busy = true
        defer { busy = false }
        do {
            _ = try await context.service.deleteDriveCopy(typed: typed, remote: remote)
            finished("Deleted from Google Drive.")
            dismiss()
        } catch let failure as SettingsService.Failure {
            switch failure {
            case .signIn: errorText = "Google needs you to sign in again."
            case .offline: errorText = "FigLock could not reach Google Drive. Check your connection and try again."
            case .foreignVault: errorText = "Google Drive holds a different vault. Nothing was deleted."
            default: errorText = "FigLock could not delete the copy. Try again."
            }
        } catch {
            errorText = "FigLock could not delete the copy. Try again."
        }
    }
}

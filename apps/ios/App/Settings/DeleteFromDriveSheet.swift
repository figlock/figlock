import Shared
import SwiftUI

/// The typed confirmation of ADR-0035. The button stays off until the phrase is typed.
struct DeleteFromDriveSheet: View {
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
            try await context.sync.deleteDriveCopy(using: context.service, typed: typed, remote: remote)
            finished("Deleted from Google Drive.")
            dismiss()
        } catch let failure as SettingsService.Failure {
            switch failure {
            case .signIn: errorText = "Google needs you to sign in again."
            case .offline: errorText = "FigLock could not reach Google Drive. Check your connection and try again."
            case .foreignVault: errorText = "Google Drive holds a different vault. Nothing was deleted."
            case .damaged:
                errorText =
                    "The copy in Google Drive looks damaged, so FigLock cannot tell it is yours. Nothing was deleted."
            case .incomplete:
                errorText =
                    "FigLock could not finish deleting from Google Drive. Nothing of yours is lost. Tap Delete again."
            default: errorText = "FigLock could not delete the copy. Try again."
            }
        } catch {
            errorText = "FigLock could not delete the copy. Try again."
        }
    }
}

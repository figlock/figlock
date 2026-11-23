import Shared
import SwiftUI
import UniformTypeIdentifiers

/// Google Drive or an iCloud Drive folder, for the screens that read a vault from storage (ADR-0043).
struct SourcePicker: View {
    @Bindable var model: OnboardingModel

    var body: some View {
        Picker("Where is your vault saved?", selection: $model.source) {
            ForEach(OnboardingModel.Source.allCases) { Text($0.rawValue).tag($0) }
        }
        .pickerStyle(.segmented)
    }
}

/// What to do before a vault can be read: sign in to Google, or pick the iCloud Drive folder in the Files picker.
/// Nothing is created in the folder; it has to be the one that already holds the vault.
struct RemoteChooser: View {
    let model: OnboardingModel
    let purpose: String
    @State private var picking = false

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s4) {
            switch model.source {
            case .drive:
                Text("\(purpose) the Google Drive where your vault is saved. Sign in to the same Google account.")
                    .font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary)
                if !model.drive.isConfigured {
                    Notice(text: "Google sign-in is not set up in this build yet.", isError: true)
                }
                ErrorNotice(model: model)
                Button {
                    Task { await model.signInToDrive() }
                } label: {
                    Label("Sign in to Google Drive", systemImage: "person.crop.circle")
                }
                .buttonStyle(PrimaryActionStyle()).disabled(model.busy || !model.drive.isConfigured)
            case .icloud:
                Text(
                    "\(purpose) the folder in iCloud Drive where your vault is saved. Pick that folder; FigLock never creates one."
                )
                .font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary)
                ErrorNotice(model: model)
                Button {
                    picking = true
                } label: {
                    Label("Choose the folder", systemImage: "folder")
                }
                .buttonStyle(PrimaryActionStyle()).disabled(model.busy)
            }
        }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.folder]) { result in
            Task { await model.chooseFolder(result) }
        }
    }
}

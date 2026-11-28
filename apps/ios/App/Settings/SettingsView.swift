import Shared
import SwiftUI

/// The entry from the vault list: a gear in the navigation bar that opens Settings.
/// The Settings tab: the settings list once the context is in the environment.
struct SettingsTab: View {
    @Environment(\.settingsContext) private var context
    /// The loaded vault, for the screens that open an item (Password health). Nil until it has loaded.
    let browser: VaultBrowser?
    let copier: ClipboardCopier

    var body: some View {
        if let context { SettingsView(context: context, browser: browser, copier: copier) }
    }
}

/// Settings (`design/prototypes/ios`, "settings"): security, the vault's devices and sync, locking now, and removing
/// the vault from this iPhone.
struct SettingsView: View {
    let context: SettingsContext
    let browser: VaultBrowser?
    let copier: ClipboardCopier

    @State private var confirmingRemoval = false
    @State private var removalFailed = false

    var body: some View {
        List {
            Section {
                NavigationLink {
                    AutoLockView(context: context)
                } label: {
                    SettingsRow(
                        symbol: "clock", title: "Auto-lock",
                        value: AutoLockOption.from(
                            seconds: context.lock.autoLockAfter
                        ).title)
                }
                NavigationLink {
                    FaceIDView(context: context)
                } label: {
                    SettingsRow(symbol: "faceid", title: "Face ID", value: FaceIDView.summary(context.lock))
                }
                .accessibilityIdentifier("settings-face-id")
                if let browser {
                    NavigationLink {
                        PasswordHealthView(browser: browser, copier: copier)
                    } label: {
                        SettingsRow(symbol: "checkmark.shield", title: "Password health")
                    }
                    .accessibilityIdentifier("settings-password-health")
                }
                NavigationLink {
                    EmergencyKitView(context: context)
                } label: {
                    SettingsRow(symbol: "key", title: "Emergency Kit")
                }
                NavigationLink {
                    RecoveryKeyView(context: context)
                } label: {
                    SettingsRow(
                        symbol: "lifepreserver", title: "Recovery Key",
                        value: context.recoveryKeyIsOn ? "On" : "Off")
                }
            } header: {
                FLSectionHeader("Security")
            }
            .flTileRows()

            Section {
                NavigationLink {
                    AppearanceView(context: context)
                } label: {
                    SettingsRow(
                        symbol: "circle.lefthalf.filled", title: "Appearance",
                        value: context.appearance.option.title)
                }
                .accessibilityIdentifier("settings-appearance")
            } header: {
                FLSectionHeader("Display")
            }
            .flTileRows()

            Section {
                NavigationLink {
                    DevicesView(context: context)
                } label: {
                    SettingsRow(symbol: "laptopcomputer.and.iphone", title: "Devices")
                }
                NavigationLink {
                    SyncSettingsView(context: context)
                } label: {
                    SettingsRow(
                        symbol: "arrow.triangle.2.circlepath", title: "Sync",
                        value: context.sync.summary)
                }
            } header: {
                FLSectionHeader("Vault")
            }
            .flTileRows()

            Section {
                NavigationLink {
                    AboutView()
                } label: {
                    SettingsRow(symbol: "info.circle", title: "About FigLock")
                }
                .accessibilityIdentifier("about-figlock")
            } header: {
                FLSectionHeader("About")
            }
            .flTileRows()

            Section {
                Button {
                    context.lock.lock()
                } label: {
                    Label("Lock", systemImage: "lock")
                        .font(FLFont.ui(FLText.md, weight: FLWeight.medium))
                        .frame(minHeight: FLSize.controlHeightLg, alignment: .leading)
                }
            }
            .flTileRows()

            Section {
                Button(role: .destructive) {
                    confirmingRemoval = true
                } label: {
                    Label("Remove this vault from this iPhone", systemImage: "rectangle.portrait.and.arrow.right")
                        .rowLabel(destructive: true)
                        .font(FLFont.ui(FLText.md, weight: FLWeight.medium))
                        .frame(minHeight: FLSize.controlHeightLg, alignment: .leading)
                }
                .tint(FLColor.danger)
            } footer: {
                Text("Your items stay in your sync places and on your other devices.")
            }
            .flTileRows()
        }
        .flTileList()
        .navigationTitle("Settings")
        .alert(removalTitle, isPresented: $confirmingRemoval) {
            Button("Cancel", role: .cancel) {}
            Button("Remove vault", role: .destructive) { remove() }
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

    private var removalTitle: String {
        "Remove \u{201C}\(SettingsService.vaultName)\u{201D} from this iPhone?"
    }

    private func remove() {
        do {
            try context.onboarding.removeFromThisPhone()
            context.drive.signOut()
            context.settings.clear()
            context.lock.lock()
            context.lock.vaultDidChange()
        } catch {
            removalFailed = true
        }
    }
}

/// One row of a settings list: an icon tile, a title, and what it is set to or a line under it.
struct SettingsRow: View {
    let symbol: String
    let title: String
    var value: String?
    var detail: String?

    var body: some View {
        HStack(spacing: FLSpace.s3) {
            FLIconTile(symbol: symbol)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(FLFont.ui(FLRowText.title)).foregroundStyle(FLColor.text)
                if let detail {
                    Text(detail).font(FLFont.ui(FLRowText.subtitle, relativeTo: .subheadline))
                        .foregroundStyle(FLColor.textSecondary)
                }
            }
            Spacer(minLength: FLSpace.s2)
            if let value {
                Text(value).font(FLFont.ui(FLRowText.title, relativeTo: .body)).foregroundStyle(
                    FLColor.textSecondary)
            }
        }
        .padding(.vertical, FLRow.verticalPadding)
        .frame(minHeight: FLSize.controlHeightLg)
        .accessibilityElement(children: .combine)
    }
}

struct AutoLockView: View {
    let context: SettingsContext

    @State private var selected: AutoLockOption
    @State private var failed = false

    init(context: SettingsContext) {
        self.context = context
        _selected = State(initialValue: AutoLockOption.from(seconds: context.lock.autoLockAfter))
    }

    var body: some View {
        List {
            Section {
                ForEach(AutoLockOption.allCases) { option in
                    Button {
                        choose(option)
                    } label: {
                        HStack {
                            Text(option.title).font(FLFont.ui(FLRowText.title)).foregroundStyle(FLColor.text)
                            Spacer()
                            if option == selected {
                                Image(systemName: "checkmark").foregroundStyle(FLColor.primary)
                                    .accessibilityHidden(true)
                            }
                        }
                        .frame(minHeight: FLSize.controlHeightLg)
                    }
                    .accessibilityAddTraits(option == selected ? .isSelected : [])
                }
            } footer: {
                Text("Locks after you leave the app for this long.")
            }
            .flTileRows()
            if failed {
                Notice(text: "Could not save. Try again.", isError: true)
                    .listRowBackground(Color.clear)
            }
        }
        .font(FLFont.ui(FLText.md))
        .flTileList()
        .navigationTitle("Auto-lock")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func choose(_ option: AutoLockOption) {
        do {
            try context.settings.setAutoLock(option)
            context.lock.autoLockAfter = option.seconds
            selected = option
            failed = false
        } catch {
            failed = true
        }
    }
}

/// Appearance: follow the system, or always light or always dark.
struct AppearanceView: View {
    let context: SettingsContext
    @State private var failed = false

    var body: some View {
        List {
            Section {
                ForEach(AppearanceOption.allCases) { option in
                    Button {
                        failed = !context.appearance.choose(option)
                    } label: {
                        HStack {
                            Text(option.title).font(FLFont.ui(FLRowText.title)).foregroundStyle(FLColor.text)
                            Spacer()
                            if option == context.appearance.option {
                                Image(systemName: "checkmark").foregroundStyle(FLColor.primary)
                                    .accessibilityHidden(true)
                            }
                        }
                        .frame(minHeight: FLSize.controlHeightLg)
                    }
                    .accessibilityIdentifier("appearance-\(option.rawValue)")
                    .accessibilityAddTraits(option == context.appearance.option ? .isSelected : [])
                }
            } footer: {
                Text("System follows the light or dark setting of this iPhone.")
            }
            .flTileRows()
            if failed {
                Notice(text: "Could not save. Try again.", isError: true)
                    .listRowBackground(Color.clear)
            }
        }
        .flTileList()
        .navigationTitle("Appearance")
        .navigationBarTitleDisplayMode(.inline)
    }
}

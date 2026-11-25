import Shared
import SwiftUI

/// The entry from the vault list: a gear in the navigation bar that opens Settings.
struct SettingsLink: View {
    @Environment(\.settingsContext) private var context

    var body: some View {
        if let context {
            NavigationLink {
                SettingsView(context: context)
            } label: {
                Image(systemName: "gearshape")
            }
            .accessibilityLabel("Settings")
            .flMinTouchTarget()
        }
    }
}

/// Settings (`design/prototypes/ios`, "settings"): security, the vault's devices and sync, locking now, and removing
/// the vault from this iPhone.
struct SettingsView: View {
    let context: SettingsContext

    @State private var confirmingRemoval = false
    @State private var removalFailed = false

    var body: some View {
        List {
            Section("Security") {
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
            }
            .listRowBackground(FLColor.surface)

            Section("Vault") {
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
            }
            .listRowBackground(FLColor.surface)

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
            .listRowBackground(FLColor.surface)
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(FLColor.bg)
        .navigationTitle("Settings")
        .alert(removalTitle, isPresented: $confirmingRemoval) {
            Button("Cancel", role: .cancel) {}
            Button("Remove vault", role: .destructive) { remove() }
        } message: {
            Text(
                "This removes the vault, its keys and Face ID from this iPhone. Your synced copies are not deleted. To add it again you need your Secret Key and master password."
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
            Image(systemName: symbol)
                .font(.system(size: 18, weight: .medium))
                .foregroundStyle(FLColor.primary)
                .frame(width: 32, height: 32)
                .background(FLColor.primarySubtle, in: RoundedRectangle(cornerRadius: FLRadius.sm))
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(FLFont.ui(FLText.md, weight: FLWeight.medium)).foregroundStyle(FLColor.text)
                if let detail {
                    Text(detail).font(FLFont.ui(FLText.sm, relativeTo: .footnote))
                        .foregroundStyle(FLColor.textSecondary)
                }
            }
            Spacer(minLength: FLSpace.s2)
            if let value {
                Text(value).font(FLFont.ui(FLText.base, relativeTo: .callout)).foregroundStyle(
                    FLColor.textSecondary)
            }
        }
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
                            Text(option.title).foregroundStyle(FLColor.text)
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
            .listRowBackground(FLColor.surface)
            if failed {
                Notice(text: "Could not save. Try again.", isError: true)
                    .listRowBackground(Color.clear)
            }
        }
        .font(FLFont.ui(FLText.md))
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(FLColor.bg)
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

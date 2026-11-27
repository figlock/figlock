import Shared
import SwiftUI

/// The Secret Key and the Emergency Kit (`design/prototypes/ios`, "secret-key"). Showing the key and saving the kit
/// both need Face ID or the passcode; the key hides itself after 10 seconds.
struct EmergencyKitView: View {
    let context: SettingsContext

    @State private var reveal: SecretKeyReveal
    @State private var failed = false
    @Environment(\.scenePhase) private var scenePhase

    init(context: SettingsContext) {
        self.context = context
        _reveal = State(initialValue: SecretKeyReveal(service: context.service))
    }

    var body: some View {
        List {
            Section {
                if reveal.isKept {
                    keyRow
                } else {
                    Label(
                        "This iPhone does not keep your Secret Key. Find it on your saved Emergency Kit, or on another device that keeps it.",
                        systemImage: "info.circle"
                    )
                    .font(FLFont.ui(FLText.base, relativeTo: .callout))
                    .foregroundStyle(FLColor.textSecondary)
                }
            } header: {
                Text("Needed, with your password, to add a device.").textCase(nil)
            } footer: {
                if reveal.isKept { Text("It stays hidden until you choose Show. Showing it needs Face ID.") }
            }
            .listRowBackground(FLColor.surface)

            if reveal.isKept {
                Section {
                    if let url = reveal.kitFile {
                        KitShareButton(url: url) {
                            Label("Save the kit", systemImage: "square.and.arrow.up")
                                .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
                        }
                        .buttonStyle(PrimaryActionStyle())
                        Button("Done") { reveal.cleanUp() }
                            .buttonStyle(PlainActionStyle())
                    } else {
                        Button {
                            Task { failed = await reveal.prepareKit(storage: context.storageLabel) == nil }
                        } label: {
                            Label("Make a new copy", systemImage: "doc.badge.arrow.up")
                                .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg)
                        }
                        .buttonStyle(SecondaryActionStyle())
                    }
                } header: {
                    Text("Emergency Kit")
                } footer: {
                    Text(
                        "Keep it printed or offline, not in this vault."
                    )
                }
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets())
            }
            if failed {
                Notice(text: "The kit could not be made. Try again.", isError: true)
                    .listRowBackground(Color.clear)
            }
        }
        .listStyle(.insetGrouped)
        .scrollContentBackground(.hidden)
        .background(FLColor.bg)
        .navigationTitle("Secret Key")
        .navigationBarTitleDisplayMode(.inline)
        .onChange(of: scenePhase) { _, phase in
            if phase != .active { reveal.cleanUpForScenePhase(isBackground: phase == .background) }
        }
        .onDisappear { reveal.cleanUp() }
    }

    private var keyRow: some View {
        HStack {
            if let key = reveal.shown {
                Text(key)
                    .font(FLFont.mono(FLText.base))
                    .foregroundStyle(FLColor.text)
                    .textSelection(.disabled)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                Text(
                    "FL1-\u{2022}\u{2022}\u{2022}\u{2022}-\u{2022}\u{2022}\u{2022}\u{2022}-\u{2022}\u{2022}\u{2022}\u{2022}"
                )
                .font(FLFont.mono(FLText.base))
                .foregroundStyle(FLColor.textSecondary)
                .accessibilityLabel("Secret Key hidden")
            }
            Spacer(minLength: FLSpace.s2)
            Button(reveal.shown == nil ? "Show" : "Hide") {
                if reveal.shown == nil {
                    Task { await reveal.show() }
                } else {
                    reveal.hide()
                }
            }
            .buttonStyle(.borderless)
            .foregroundStyle(FLColor.textLink)
            .flMinTouchTarget()
        }
        .frame(minHeight: FLSize.controlHeightLg)
    }
}

import Shared
import SwiftUI

/// The password generator. Opened from the editor next to a password field (`onUse` fills it) or on its own from
/// the vault list (copy only). Values come from the Rust generator; nothing is kept after the sheet closes.
struct GeneratorView: View {
    let copier: ClipboardCopier
    var onUse: ((String) -> Void)?

    @State private var settings = GeneratorSettings()
    @State private var value = ""
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: FLSpace.s4) {
                    preview
                    Picker("Kind", selection: $settings.mode) {
                        ForEach(GeneratorSettings.Mode.allCases) { Text($0.title).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    VStack(spacing: 0) {
                        switch settings.mode {
                        case .password: passwordOptions
                        case .passphrase: passphraseOptions
                        }
                    }
                    .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.md))
                }
                .padding(FLSpace.s4)
            }
            .background(FLColor.bg)
            .navigationTitle("Generator")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(onUse == nil ? "Done" : "Cancel") { dismiss() }
                }
            }
        }
        .onAppear(perform: regenerate)
        .onChange(of: settings) { regenerate() }
        .presentationDetents([.large])
    }

    private func regenerate() { value = settings.generate() ?? "" }

    private var preview: some View {
        VStack(alignment: .leading, spacing: FLSpace.s3) {
            if value.isEmpty {
                Text("Choose at least one kind of character")
                    .font(FLFont.ui(FLText.md, relativeTo: .body))
                    .foregroundStyle(FLColor.textSecondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                Text(value)
                    .font(FLFont.mono(FLText.lg))
                    .foregroundStyle(FLColor.text)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityLabel("Generated \(settings.mode.title.lowercased())")
            }
            if let rating = GeneratorSettings.rating(of: value) {
                StrengthLabel(rating: rating)
            }
            HStack(spacing: FLSpace.s2) {
                Button {
                    regenerate()
                } label: {
                    Label("Regenerate", systemImage: "arrow.clockwise")
                }
                .buttonStyle(SecondaryActionStyle())
                .disabled(value.isEmpty)
                Button {
                    copier.copy(value, key: "generator")
                } label: {
                    Label(copier.copiedKey == "generator" ? "Copied" : "Copy", systemImage: "doc.on.doc")
                }
                .buttonStyle(SecondaryActionStyle())
                .disabled(value.isEmpty)
            }
            if let onUse {
                Button("Use this \(settings.mode.title.lowercased())") {
                    onUse(value)
                    dismiss()
                }
                .buttonStyle(PrimaryActionStyle())
                .disabled(value.isEmpty)
            }
            if copier.copiedKey == "generator" {
                Text("Copied. The clipboard clears in 30 s")
                    .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.success)
            }
        }
        .padding(FLSpace.s4)
        .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.md))
    }

    @ViewBuilder private var passwordOptions: some View {
        Stepper(value: $settings.length, in: GeneratorSettings.lengthRange) {
            Text("Length: \(settings.length)").font(FLFont.ui(FLText.md))
        }
        .padding(FLSpace.s4)
        Divider()
        OptionToggle(title: "Lowercase letters", isOn: $settings.lower)
        OptionToggle(title: "Uppercase letters", isOn: $settings.upper)
        OptionToggle(title: "Digits", isOn: $settings.digits)
        OptionToggle(title: "Symbols", isOn: $settings.symbols)
        OptionToggle(title: "Avoid look-alikes (l, 1, O, 0)", isOn: $settings.avoidLookalikes, last: true)
    }

    @ViewBuilder private var passphraseOptions: some View {
        Stepper(value: $settings.words, in: GeneratorSettings.wordsRange) {
            Text("Words: \(settings.words)").font(FLFont.ui(FLText.md))
        }
        .padding(FLSpace.s4)
        Divider()
        Picker("Separator", selection: $settings.separator) {
            ForEach(GeneratorSettings.Separator.allCases) { Text($0.title).tag($0) }
        }
        .padding(FLSpace.s4)
        Divider()
        OptionToggle(title: "Capitalize words", isOn: $settings.capitalize)
        OptionToggle(title: "Add a number", isOn: $settings.addNumber, last: true)
    }
}

private struct OptionToggle: View {
    let title: String
    @Binding var isOn: Bool
    var last = false

    var body: some View {
        VStack(spacing: 0) {
            Toggle(title, isOn: $isOn)
                .font(FLFont.ui(FLText.md))
                .tint(FLColor.primary)
                .padding(FLSpace.s4)
            if !last { Divider().padding(.leading, FLSpace.s4) }
        }
    }
}

/// "Weak", "Fair" or "Strong" with a matching colour; the word carries the meaning, the colour only backs it.
struct StrengthLabel: View {
    let rating: GeneratorSettings.Rating

    var body: some View {
        Label(
            "\(rating.rawValue) \(rating == .strong ? "" : "- add length or variety")".trimmingCharacters(
                in: .whitespaces), systemImage: symbol
        )
        .font(FLFont.ui(FLText.sm, weight: FLWeight.medium, relativeTo: .footnote))
        .foregroundStyle(color)
    }

    private var color: Color {
        switch rating {
        case .weak: FLColor.danger
        case .fair: FLColor.warning
        case .strong: FLColor.success
        }
    }

    private var symbol: String {
        switch rating {
        case .weak: "exclamationmark.triangle.fill"
        case .fair: "minus.circle.fill"
        case .strong: "checkmark.shield.fill"
        }
    }
}

import Shared
import SwiftUI

struct Screen<Content: View>: View {
    let title: String
    var step: String?
    var subtitle: String?
    @ViewBuilder var content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FLSpace.s4) {
                if let step {
                    Text(step).font(FLFont.ui(FLText.sm, weight: FLWeight.medium, relativeTo: .footnote))
                        .foregroundStyle(FLColor.textTertiary)
                }
                Text(title).font(FLFont.ui(FLText.xl2, weight: FLWeight.bold, relativeTo: .title2))
                    .foregroundStyle(FLColor.text).accessibilityAddTraits(.isHeader)
                if let subtitle {
                    Text(subtitle).font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary)
                }
                content
            }
            .padding(FLSpace.s4)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .scrollDismissesKeyboard(.interactively)
        .background(FLColor.bg.ignoresSafeArea())
        .navigationBarTitleDisplayMode(.inline)
    }
}

struct LabeledField<Field: View>: View {
    let label: String
    var hint: String?
    @ViewBuilder var field: Field

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s1) {
            Text(label).font(FLFont.ui(FLText.sm, weight: FLWeight.medium)).foregroundStyle(FLColor.textSecondary)
            field
                .padding(FLSpace.s3)
                .frame(minHeight: FLSize.controlHeightLg)
                .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.sm))
                .overlay(RoundedRectangle(cornerRadius: FLRadius.sm).stroke(FLColor.border))
            if let hint {
                Text(hint).font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textTertiary)
            }
        }
    }
}

struct ErrorNotice: View {
    let model: OnboardingModel

    var body: some View {
        if let text = model.errorText { Notice(text: text, isError: true) }
    }
}

struct FaceIDToggle: View {
    @Bindable var model: OnboardingModel

    var body: some View {
        if model.faceIDAvailable {
            Toggle(isOn: $model.faceID) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Unlock with Face ID").font(FLFont.ui(FLText.md, weight: FLWeight.medium))
                    Text("Use your face instead of typing the master password.")
                        .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textSecondary)
                }
            }
            .padding(FLSpace.s3)
            .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.sm))
        }
    }
}

struct Busy: View {
    let label: String
    let busy: Bool

    var body: some View {
        if busy { ProgressView(label).font(FLFont.ui(FLText.base)) }
    }
}

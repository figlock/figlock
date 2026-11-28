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
            .flFormColumn()
        }
        .scrollDismissesKeyboard(.interactively)
        .background(FLColor.bg.ignoresSafeArea())
        .navigationBarTitleDisplayMode(.inline)
    }
}

/// A labelled text box. Every box is the same height: `trailing` buttons (show or hide, generate) sit inside the
/// box next to the field, without the field's own padding around them, so a 44pt touch target does not make
/// the box taller than its neighbours (FIG-254).
struct LabeledField<Field: View, Trailing: View>: View {
    let label: String
    var hint: String?
    let hasTrailing: Bool
    @ViewBuilder var field: Field
    @ViewBuilder var trailing: Trailing

    init(
        label: String, hint: String? = nil, hasTrailing: Bool = true, @ViewBuilder field: () -> Field,
        @ViewBuilder trailing: () -> Trailing
    ) {
        self.label = label
        self.hint = hint
        self.hasTrailing = hasTrailing
        self.field = field()
        self.trailing = trailing()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s1) {
            Text(label).font(FLFont.ui(FLText.sm, weight: FLWeight.medium)).foregroundStyle(FLColor.textSecondary)
            HStack(spacing: hasTrailing ? FLSpace.s2 : 0) {
                field.padding(.vertical, FLSpace.s3)
                trailing
            }
            .padding(.leading, FLSpace.s3)
            .padding(.trailing, hasTrailing ? FLSpace.s1 : FLSpace.s3)
            .frame(minHeight: FLSize.controlHeightLg)
            .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.sm))
            .overlay(RoundedRectangle(cornerRadius: FLRadius.sm).stroke(FLColor.border))
            if let hint {
                Text(hint).font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textTertiary)
            }
        }
    }
}

extension LabeledField where Trailing == EmptyView {
    init(label: String, hint: String? = nil, @ViewBuilder field: () -> Field) {
        self.init(label: label, hint: hint, hasTrailing: false, field: field, trailing: { EmptyView() })
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
                    Text("Skip typing your password.")
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

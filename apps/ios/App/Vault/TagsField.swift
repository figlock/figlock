import Shared
import SwiftUI

/// The tags field of the item editor: each finished tag is a chip with a remove button, and the text after the
/// last comma, space or `#` is the one still being typed (`ItemDraft.finishedTags` / `pendingTag`). Same rules as
/// before; chips only make where one tag ends and the next begins visible (FIG-246).
struct TagsField: View {
    @Binding var draft: ItemDraft
    @FocusState private var typing: Bool
    /// What the field shows. The draft owns the text; this is written back after a separator so the field
    /// empties at once (a binding that answers differently from what was typed does not always refresh).
    @State private var typed = ""

    var body: some View {
        LabeledField(label: "Tags", hint: "Separate with commas. Lowercase letters, digits, - and _.") {
            TagFlow(spacing: FLSpace.s2) {
                ForEach(draft.finishedTags, id: \.self) { tag in
                    TagChip(tag: tag) { draft.removeTag(tag) }
                }
                TextField(draft.finishedTags.isEmpty ? "work, mail" : "Add tag", text: $typed)
                    .textInputAutocapitalization(.never).autocorrectionDisabled()
                    .submitLabel(.done)
                    .focused($typing)
                    .onAppear { typed = draft.pendingTag }
                    .onChange(of: typed) { _, text in
                        draft.pendingTag = text
                        if draft.pendingTag != text { typed = draft.pendingTag }
                    }
                    .onSubmit {
                        draft.finishPendingTag()
                        typed = draft.pendingTag
                        typing = true
                    }
                    .onChange(of: typing) { _, now in
                        if !now {
                            draft.finishPendingTag()
                            typed = draft.pendingTag
                        }
                    }
                    .accessibilityIdentifier("tags-input")
                    .tagFlowFillsRow()
            }
        }
    }
}

private struct TagChip: View {
    let tag: String
    let remove: () -> Void

    private var valid: Bool { ItemDraft.isValidTag(tag) }

    var body: some View {
        HStack(spacing: FLSpace.s1) {
            Text(tag).font(FLFont.ui(FLText.sm, weight: FLWeight.medium))
                .foregroundStyle(valid ? FLColor.text : FLColor.danger)
            Button(action: remove) {
                Image(systemName: "xmark").font(.system(size: 10, weight: .bold)).foregroundStyle(
                    FLColor.textSecondary
                )
                .frame(width: 20, height: 20).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Remove tag \(tag)")
        }
        .padding(.leading, FLSpace.s2).padding(.trailing, 2).frame(height: 28)
        .background(FLColor.surfaceSunken, in: RoundedRectangle(cornerRadius: FLRadius.xs))
        .overlay(RoundedRectangle(cornerRadius: FLRadius.xs).stroke(valid ? Color.clear : FLColor.danger))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("tag-chip-\(tag)")
    }
}

private struct FillsRowKey: LayoutValueKey {
    static let defaultValue = false
}

extension View {
    /// In a `TagFlow`, let this view take the rest of its row (the text field).
    fileprivate func tagFlowFillsRow() -> some View { layoutValue(key: FillsRowKey.self, value: true) }
}

/// Chips wrap onto new lines; the view marked `tagFlowFillsRow` takes what is left of its row, at least 100 pt.
private struct TagFlow: Layout {
    var spacing: CGFloat

    private func place(_ subviews: Subviews, width: CGFloat) -> (frames: [CGRect], height: CGFloat) {
        var frames: [CGRect] = []
        var left: CGFloat = 0
        var top: CGFloat = 0
        var rowHeight: CGFloat = 0
        for subview in subviews {
            let fills = subview[FillsRowKey.self]
            var size = subview.sizeThatFits(.unspecified)
            if fills { size.width = max(100, width - left) }
            size.width = min(size.width, width)
            if left > 0, left + size.width > width {
                left = 0
                top += rowHeight + spacing
                rowHeight = 0
                if fills { size.width = width }
            }
            frames.append(CGRect(origin: CGPoint(x: left, y: top), size: size))
            left += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
        return (frames, top + rowHeight)
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 320
        return CGSize(width: width, height: place(subviews, width: width).height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let (frames, _) = place(subviews, width: bounds.width)
        for (subview, frame) in zip(subviews, frames) {
            subview.place(
                at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY),
                proposal: ProposedViewSize(frame.size))
        }
    }
}

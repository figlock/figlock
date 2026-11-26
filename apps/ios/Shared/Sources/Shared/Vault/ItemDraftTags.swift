import Foundation

/// The tags field as chips: everything before the last separator is a finished tag, the text after it is the
/// one being typed. `tagsText` stays the single source of truth, so the rules of `ItemDraft.tags` (lowercase,
/// split at commas, spaces and `#`, letters, digits, `-` and `_`, at most 40 characters) are the same as before.
extension ItemDraft {
    static let tagSeparators: Set<Character> = [",", ";", " ", "#"]

    private var tagSplit: (head: Substring, tail: Substring) {
        guard let cut = tagsText.lastIndex(where: { Self.tagSeparators.contains($0) }) else {
            return ("", tagsText[...])
        }
        let end = tagsText.index(after: cut)
        return (tagsText[..<end], tagsText[end...])
    }

    private func tokens(in text: Substring) -> [String] {
        var seen = Set<String>()
        return text.lowercased().split(whereSeparator: { Self.tagSeparators.contains($0) }).map(String.init).filter {
            seen.insert($0).inserted
        }
    }

    /// The tags the person has finished (a separator came after them), in the order typed, no repeats.
    public var finishedTags: [String] { tokens(in: tagSplit.head) }

    /// The text after the last separator: what is still being typed. Setting it keeps the finished tags, and a
    /// separator in the new text finishes the tag (it then reads back as empty).
    public var pendingTag: String {
        get { String(tagSplit.tail) }
        set { tagsText = String(tagSplit.head) + newValue }
    }

    /// Turn the text being typed into a finished tag (return key, leaving the field, saving).
    public mutating func finishPendingTag() {
        let pending = tagSplit.tail.trimmingCharacters(in: .whitespaces)
        guard !pending.isEmpty else { return }
        tagsText = String(tagSplit.head) + pending + ", "
    }

    /// Remove one finished tag, keeping the others and the text being typed.
    public mutating func removeTag(_ tag: String) {
        let kept = finishedTags.filter { $0 != tag }
        let pending = String(tagSplit.tail)
        tagsText = kept.isEmpty ? pending : kept.joined(separator: ", ") + ", " + pending
    }

    /// Whether a tag breaks the rules, so its chip can say so.
    public static func isValidTag(_ tag: String) -> Bool {
        tag.count <= 40 && tag.unicodeScalars.allSatisfy(tagCharacters.contains)
    }
}

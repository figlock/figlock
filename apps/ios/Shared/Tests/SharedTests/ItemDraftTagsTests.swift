import Foundation
import Testing

@testable import Shared

@Test func tagsFinishAtASeparatorAndTheTextAfterItIsStillBeingTyped() {
    var draft = ItemDraft(kind: .login)
    draft.pendingTag = "Work"
    #expect(draft.finishedTags.isEmpty)
    #expect(draft.pendingTag == "Work")
    #expect(draft.tags == ["work"])  // saving never loses the tag being typed

    draft.pendingTag = "Work,"  // a comma finishes it and the field starts empty again
    #expect(draft.finishedTags == ["work"])
    #expect(draft.pendingTag.isEmpty)

    draft.pendingTag = "mail #Ops Work"  // space and # split too; the unfinished "Work" is still text
    #expect(draft.finishedTags == ["work", "mail", "ops"])
    #expect(draft.pendingTag == "Work")

    draft.finishPendingTag()  // a repeat of a finished tag shows once
    #expect(draft.pendingTag.isEmpty)
    #expect(draft.finishedTags == ["work", "mail", "ops"])
    #expect(draft.tags == ["work", "mail", "ops"])
    draft.finishPendingTag()  // nothing pending: no change
    #expect(draft.tags == ["work", "mail", "ops"])

    draft.removeTag("mail")
    #expect(draft.finishedTags == ["work", "ops"])
    draft.pendingTag = "ne"
    draft.removeTag("work")
    #expect(draft.finishedTags == ["ops"])
    #expect(draft.pendingTag == "ne")
    #expect(draft.tags == ["ops", "ne"])
}

@Test func savedTagsOpenAsFinishedChipsAndBadOnesAreFlagged() {
    let item = VaultItem(
        id: "i", kind: .login, updated: Date(timeIntervalSince1970: 0),
        fields: ["title": "T", "tags/b": "1", "tags/a": "1"], secretFields: [], isDeleted: false)
    let draft = ItemDraft(editing: item)
    #expect(draft.finishedTags == ["a", "b"])
    #expect(draft.pendingTag.isEmpty)
    #expect(draft.problem == nil)
    #expect(ItemDraft.isValidTag("ok-1_x"))
    #expect(!ItemDraft.isValidTag("bad!"))
    #expect(!ItemDraft.isValidTag(String(repeating: "a", count: 41)))
}

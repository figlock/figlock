import FigLockFFI
import Shared
import SwiftUI

// MARK: Recently deleted

/// "Recently deleted" (design reference FIG-252): a grouped card of deleted items, each with Restore, and a menu with
/// "Recover all". Deleting for good (Delete forever, Delete all) waits for a purge in the core (FIG-255).
struct DeletedItemsView: View {
    let browser: VaultBrowser
    @Environment(VaultEditor.self) private var editor: VaultEditor?
    @State private var failure: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: FLSpace.s4) {
                Text("Deleted items stay here until you restore them.")
                    .font(FLFont.ui(FLText.md)).foregroundStyle(FLColor.textSecondary)
                if let failure { Notice(text: failure, isError: true) }
                if browser.deletedItems.isEmpty {
                    emptyState
                } else {
                    VStack(spacing: 0) {
                        ForEach(browser.deletedItems) { item in
                            DeletedItemRow(item: item) { Task { await restore(item) } }
                            if item.id != browser.deletedItems.last?.id { Divider().padding(.leading, FLSpace.s4) }
                        }
                    }
                    .background(FLColor.surface, in: RoundedRectangle(cornerRadius: FLRadius.md))
                }
            }
            .padding(FLSpace.s4)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(FLColor.bg.ignoresSafeArea())
        .navigationTitle("Recently deleted")
        .navigationBarTitleDisplayMode(.large)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button {
                        Task { await recoverAll() }
                    } label: {
                        Label("Recover all", systemImage: "arrow.uturn.backward")
                    }
                } label: {
                    Image(systemName: "ellipsis.circle")
                }
                .disabled(editor == nil || browser.deletedItems.isEmpty)
                .accessibilityLabel("Recently deleted actions")
                .flMinTouchTarget()
            }
        }
    }

    private var emptyState: some View {
        VStack(spacing: FLSpace.s2) {
            Image(systemName: "trash").font(.system(size: 32)).foregroundStyle(FLColor.textTertiary)
                .accessibilityHidden(true)
            Text("Nothing deleted").font(FLFont.ui(FLText.lg, weight: FLWeight.semibold, relativeTo: .headline))
                .foregroundStyle(FLColor.text)
            Text("Deleted items appear here.")
                .font(FLFont.ui(FLText.base, relativeTo: .callout)).foregroundStyle(FLColor.textSecondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity).padding(.vertical, FLSpace.s8)
    }

    private func restore(_ item: VaultItem) async {
        failure = nil
        do { try await editor?.restore(id: item.id) } catch let error as EditFailure { failure = error.message } catch {
            failure = EditFailure.rejected.message
        }
    }

    private func recoverAll() async {
        failure = nil
        do { try await editor?.restoreAll(ids: browser.deletedItems.map(\.id)) } catch let error as EditFailure {
            failure = error.message
        } catch {
            failure = EditFailure.rejected.message
        }
    }
}

private struct DeletedItemRow: View {
    let item: VaultItem
    let restore: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: FLSpace.s2) {
            HStack(alignment: .top, spacing: FLSpace.s3) {
                Image(systemName: item.kind.symbol).foregroundStyle(FLColor.textSecondary)
                    .frame(width: 24, height: 24).accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(item.title).font(FLFont.ui(FLText.md, weight: FLWeight.semibold)).foregroundStyle(FLColor.text)
                    Text(DeletedItemAge.text(deleted: item.updated, now: Date()))
                        .font(FLFont.ui(FLText.sm, relativeTo: .footnote)).foregroundStyle(FLColor.textSecondary)
                }
                Spacer(minLength: 0)
            }
            .accessibilityElement(children: .combine)
            Button(action: restore) {
                Label("Restore", systemImage: "arrow.uturn.backward")
                    .font(FLFont.ui(FLText.base, weight: FLWeight.semibold, relativeTo: .callout))
                    .foregroundStyle(FLColor.textLink)
                    .frame(minHeight: FLSize.controlHeightLg)
            }
            .padding(.leading, 24 + FLSpace.s3)
            .accessibilityLabel("Restore \(item.title)")
        }
        .padding(FLSpace.s4)
    }
}

import FigLockFFI
import Shared
import SwiftUI

/// Settings > Password health (FIG-292): the Mac Health page on the iPhone. Weak, reused and old passwords are found
/// on this iPhone and nothing leaves it; the breach check is opt-in and sends only 5 character hash prefixes. Each
/// item that needs attention opens the item.
struct PasswordHealthView: View {
    let browser: VaultBrowser
    let copier: ClipboardCopier

    @State private var health: PasswordHealthModel

    init(browser: VaultBrowser, copier: ClipboardCopier) {
        self.browser = browser
        self.copier = copier
        #if DEBUG
            let service: any BreachRangeFetching =
                DebugSeed.breachServiceOffline ? OfflineRangeService() : PwnedRangeService()
        #else
            let service: any BreachRangeFetching = PwnedRangeService()
        #endif
        _health = State(initialValue: PasswordHealthModel(session: browser.session, service: service))
    }

    var body: some View {
        List {
            if health.breaches == .unreachable {
                Notice(
                    text:
                        "Could not reach the breach service, so breaches were not checked. The other results are below. Try again when you are online.",
                    isError: true
                )
                .listRowBackground(Color.clear).listRowInsets(EdgeInsets())
                .accessibilityIdentifier("health-unreachable")
            }

            Section {
                Button {
                    Task { await health.checkBreaches() }
                } label: {
                    Label(breachButtonTitle, systemImage: "shield.lefthalf.filled.badge.checkmark").rowLabel()
                        .frame(maxWidth: .infinity, minHeight: FLSize.controlHeightLg, alignment: .leading)
                }
                .disabled(health.breaches == .checking || !health.loaded)
                .accessibilityIdentifier("health-check-breaches")
            } header: {
                FLSectionHeader("Known breaches")
            } footer: {
                Text(
                    "FigLock sends only the first 5 characters of a scrambled (SHA-1) fingerprint of each password to Have I Been Pwned, which replies with every match for that start. The match is made on this iPhone, so neither your passwords nor their full fingerprints are sent."
                )
            }
            .flTileRows()

            if health.loaded {
                resultSection
            } else {
                Section { ProgressView().frame(maxWidth: .infinity) }.flTileRows()
            }
        }
        .flTileList()
        .navigationTitle("Password health")
        .navigationBarTitleDisplayMode(.inline)
        .task { await health.load() }
    }

    private var breachButtonTitle: String {
        switch health.breaches {
        case .checking: "Checking..."
        case .checked: "Check again"
        case .notChecked, .unreachable: "Check for breaches"
        }
    }

    @ViewBuilder
    private var resultSection: some View {
        if health.rows.isEmpty {
            Section {
                VStack(spacing: FLSpace.s2) {
                    Image(systemName: "checkmark.shield.fill").font(.system(size: 32)).foregroundStyle(FLColor.success)
                        .accessibilityHidden(true)
                    Text(health.checked == 0 ? "No passwords to check yet" : "No problems found")
                        .font(FLFont.ui(FLText.md, weight: FLWeight.semibold)).foregroundStyle(FLColor.text)
                    Text(emptyDetail)
                        .font(FLFont.ui(FLText.base, relativeTo: .callout)).foregroundStyle(FLColor.textSecondary)
                        .multilineTextAlignment(.center)
                }
                .frame(maxWidth: .infinity).padding(.vertical, FLSpace.s4)
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("health-clean")
            }
            .listRowBackground(Color.clear)
        } else {
            Section {
                ForEach(health.rows) { row in
                    if let item = browser.item(id: row.id) {
                        NavigationLink {
                            ItemDetailHost(id: item.id, browser: browser, copier: copier)
                        } label: {
                            HealthItemRow(item: item, row: row)
                        }
                        .accessibilityIdentifier("health-row-\(item.id)")
                    }
                }
            } header: {
                FLSectionHeader("Needs attention")
            } footer: {
                Text(
                    "\(count(health.rows.count, "item needs", "items need")) attention, out of \(count(health.checked, "password", "passwords"))."
                )
            }
            .flTileRows()
        }
    }

    private var emptyDetail: String {
        if health.checked == 0 { return "Logins and documents with a password show up here." }
        let checks = health.breaches == .checked ? ", and for known breaches" : ""
        return "Checked \(count(health.checked, "password", "passwords")) for weak, reused and old ones\(checks)."
    }

    private func count(_ amount: Int, _ one: String, _ many: String) -> String {
        "\(amount) \(amount == 1 ? one : many)"
    }
}

/// One item that needs attention: its title and a badge for each problem.
private struct HealthItemRow: View {
    let item: VaultItem
    let row: HealthRow

    var body: some View {
        HStack(spacing: FLSpace.s3) {
            TypeTile(kind: item.kind, size: 40)
            VStack(alignment: .leading, spacing: FLSpace.s1) {
                Text(item.title)
                    .font(FLFont.ui(FLText.md, weight: FLWeight.semibold)).foregroundStyle(FLColor.text).lineLimit(1)
                FlowBadges(badges: badges)
            }
        }
        .padding(.vertical, FLSpace.s3)
        .frame(minHeight: FLSize.controlHeightLg)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(item.title), \(badges.map(\.text).joined(separator: ", "))")
    }

    private var badges: [HealthBadge] {
        var out: [HealthBadge] = []
        if let times = row.breached {
            let text = times == 1 ? "In 1 known breach" : "In \(times) known breaches"
            out.append(HealthBadge(text: text, danger: true))
        }
        if row.weakBits != nil { out.append(HealthBadge(text: "Weak", danger: true)) }
        if row.reusedWith > 0 {
            out.append(
                HealthBadge(
                    text: "Reused with \(row.reusedWith == 1 ? "1 other item" : "\(row.reusedWith) other items")",
                    danger: false))
        }
        if let days = row.oldDays {
            let text = days >= 730 ? "Unchanged for \(days / 365) years" : "Unchanged for \(days / 30) months"
            out.append(HealthBadge(text: text, danger: false))
        }
        return out
    }
}

private struct HealthBadge: Hashable {
    let text: String
    let danger: Bool
}

/// Badges that wrap onto more lines when they do not fit, instead of squeezing.
private struct FlowBadges: View {
    let badges: [HealthBadge]

    var body: some View {
        FlowLayout(spacing: FLSpace.s1) {
            ForEach(badges, id: \.self) { badge in
                Text(badge.text)
                    .font(FLFont.ui(FLText.xs, weight: FLWeight.medium, relativeTo: .caption1))
                    .foregroundStyle(badge.danger ? FLColor.danger : FLColor.warning)
                    .padding(.horizontal, FLSpace.s2).padding(.vertical, 2)
                    .background(
                        badge.danger ? FLColor.dangerSubtle : FLColor.warningSubtle, in: Capsule())
            }
        }
    }
}

/// A left-to-right layout that starts a new line when the next view does not fit.
private struct FlowLayout: Layout {
    var spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(width: proposal.width ?? .infinity, subviews: subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let placed = arrange(width: bounds.width, subviews: subviews)
        for (subview, origin) in zip(subviews, placed.origins) {
            subview.place(at: CGPoint(x: bounds.minX + origin.x, y: bounds.minY + origin.y), proposal: .unspecified)
        }
    }

    private func arrange(width: CGFloat, subviews: Subviews) -> (size: CGSize, origins: [CGPoint]) {
        var origins: [CGPoint] = []
        var cursorX: CGFloat = 0
        var cursorY: CGFloat = 0
        var lineHeight: CGFloat = 0
        var widest: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if cursorX > 0, cursorX + size.width > width {
                cursorX = 0
                cursorY += lineHeight + spacing
                lineHeight = 0
            }
            origins.append(CGPoint(x: cursorX, y: cursorY))
            cursorX += size.width + spacing
            lineHeight = max(lineHeight, size.height)
            widest = max(widest, cursorX - spacing)
        }
        return (CGSize(width: widest, height: cursorY + lineHeight), origins)
    }
}

#if DEBUG
    /// Debug builds only: a breach service that is never reachable, for `-figlock-breach-offline`.
    private struct OfflineRangeService: BreachRangeFetching {
        func range(prefix: String) async throws -> String { throw PwnedRangeService.Unreachable() }
    }
#endif

import Foundation

/// Which saved logins belong to the site or app AutoFill is asking about. Pure and local: it compares the hosts
/// of the item's website fields with the hosts the system passes in, and never leaves the process.
public enum ServiceMatch {
    /// The lower-case host of a website field or a service identifier: a bare domain ("example.com"), a url
    /// with or without a scheme, with `www.`, a port, a path and a trailing dot removed.
    public static func host(of text: String) -> String? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        let withScheme = trimmed.contains("://") ? trimmed : "https://" + trimmed
        guard let host = URLComponents(string: withScheme)?.host?.lowercased(), !host.isEmpty else { return nil }
        var result = host
        while result.hasSuffix(".") { result.removeLast() }
        if result.hasPrefix("www.") { result.removeFirst(4) }
        return result.isEmpty ? nil : result
    }

    /// Whether a login saved for `itemHost` should be offered on `requestHost`: the same host, or one is a
    /// subdomain of the other (`accounts.example.com` and `example.com`). A single label such as `com` never
    /// makes a match, and two sibling subdomains do not match each other.
    public static func hostsMatch(_ itemHost: String, _ requestHost: String) -> Bool {
        if itemHost == requestHost { return true }
        return isSubdomain(itemHost, of: requestHost) || isSubdomain(requestHost, of: itemHost)
    }

    private static func isSubdomain(_ host: String, of parent: String) -> Bool {
        parent.contains(".") && host.hasSuffix("." + parent)
    }

    /// The logins whose websites match any of the requested identifiers, best first: an exact host match before a
    /// subdomain match, then the most recently changed.
    public static func matches(_ items: [VaultItem], identifiers: [String]) -> [VaultItem] {
        // A single label ("com", "intranet") is too broad to match on.
        let requested = identifiers.compactMap(host(of:)).filter { $0.contains(".") }
        guard !requested.isEmpty else { return [] }
        var scored: [(item: VaultItem, exact: Bool)] = []
        for item in items where item.kind == .login && !item.isDeleted {
            let hosts = item.websites.compactMap(host(of:)).filter { $0.contains(".") }
            let exact = hosts.contains { itemHost in requested.contains(itemHost) }
            if exact || hosts.contains(where: { saved in requested.contains { hostsMatch(saved, $0) } }) {
                scored.append((item, exact))
            }
        }
        return scored.sorted {
            if $0.exact != $1.exact { return $0.exact }
            return $0.item.updated > $1.item.updated
        }.map(\.item)
    }
}

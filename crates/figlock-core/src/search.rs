//! Local search (`docs/prd.md` 7.2, `docs/research.md` 5). Pure and in memory: an [`Index`] is built from
//! decrypted [`ItemView`]s after unlock and dropped on lock.
//!
//! What is searched is an allow-list of non-secret fields (title, provider, username, urls and allowed
//! hosts, tags, notes, custom field names, ...). Anything else, including every secret field and any field
//! added in future, is not indexed and so can never be found or ranked by its value.
//!
//! A query is split on whitespace and every term must match. A term matches a field by, best first: exact
//! token, token prefix, substring, one or two typos (Damerau-Levenshtein on the token or on a token
//! prefix), then a compact in-order subsequence ("cldflr" finds Cloudflare). Domain-shaped terms match
//! hosts by suffix on label boundaries. A term that starts like a well-known token prefix (`ghp_`,
//! `sk-ant-`, ...) also finds token items of that provider, without ever indexing the token itself.
//! Caller supplied aliases map a short word to another ("cf" to "cloudflare"). Recent items get a small
//! boost that decays with age; an empty query lists items by recency.

use crate::{ItemId, ItemType, ItemView, is_secret_field};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

/// Terms beyond this are ignored, and longer terms are cut, so a hostile query stays cheap.
const MAX_TERMS: usize = 8;
const MAX_TERM_CHARS: usize = 64;
/// Cut for one indexed field, so a megabyte note does not dominate memory or time.
const MAX_INDEXED_CHARS: usize = 16 * 1024;
const MAX_BOOST: u64 = 20;
const BOOST_HALF_LIFE_DAYS: u64 = 30;
const DAY_MS: u64 = 86_400_000;

/// Well-known API token prefixes and the provider words they point at. Longer prefixes first within a
/// provider is not needed: every matching row contributes.
const TOKEN_PREFIXES: &[(&str, &[&str])] = &[
    ("ghp_", &["github"]),
    ("gho_", &["github"]),
    ("ghu_", &["github"]),
    ("ghs_", &["github"]),
    ("ghr_", &["github"]),
    ("github_pat_", &["github"]),
    ("glpat-", &["gitlab"]),
    ("sk-ant-", &["anthropic", "claude"]),
    ("sk-proj-", &["openai"]),
    ("sk-", &["openai"]),
    ("xoxb-", &["slack"]),
    ("xoxp-", &["slack"]),
    ("xoxa-", &["slack"]),
    ("xapp-", &["slack"]),
    ("akia", &["aws", "amazon"]),
    ("asia", &["aws", "amazon"]),
    ("aiza", &["google"]),
    ("sk_live_", &["stripe"]),
    ("sk_test_", &["stripe"]),
    ("rk_live_", &["stripe"]),
    ("pk_live_", &["stripe"]),
    ("npm_", &["npm"]),
    ("pypi-", &["pypi"]),
    ("hf_", &["huggingface", "hugging"]),
    ("dop_v1_", &["digitalocean"]),
];

/// One result: the item and its relevance. Higher is better; ties are ordered by item id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub id: ItemId,
    pub score: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Title,
    Host,
    Provider,
    Tag,
    Username,
    Name,
    Text,
}

impl Kind {
    fn weight(self) -> u32 {
        match self {
            Kind::Title => 100,
            Kind::Host => 90,
            Kind::Provider | Kind::Tag => 80,
            Kind::Username => 50,
            Kind::Name => 30,
            Kind::Text => 20,
        }
    }
}

struct Tok {
    s: String,
    chars: usize,
    mask: u64,
}

struct Field {
    kind: Kind,
    /// Normalized text (lowercase, no diacritics). For hosts, the bare host.
    text: String,
    /// Distinct tokens of `text`.
    tokens: Vec<Tok>,
    /// Union of the token masks.
    mask: u64,
}

/// One bit per character (folded to 64 buckets). A collision can only make a character look present, so
/// "term has more than `e` characters missing from the token" is a safe reason to skip an edit distance.
fn mask_of(chars: impl Iterator<Item = char>) -> u64 {
    chars.fold(0, |m, c| m | 1 << (u32::from(c) % 64))
}

struct Entry {
    id: ItemId,
    item_type: ItemType,
    updated_ms: u64,
    fields: Vec<Field>,
}

/// An in-memory search index over non-secret fields.
#[derive(Default)]
pub struct Index {
    entries: Vec<Entry>,
    /// (normalized alias, normalized target).
    aliases: Vec<(String, String)>,
}

/// Lowercase, compatibility-decompose and drop combining marks, so "Café" and "cafe" meet.
fn norm(s: &str) -> String {
    s.chars()
        .take(MAX_INDEXED_CHARS)
        .collect::<String>()
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The bare host of a url or host pattern: no scheme, userinfo, port, path, `www.` or `*.` prefix.
fn host_of(s: &str) -> Option<String> {
    let s = s.trim().to_lowercase();
    let rest = s.split_once("://").map_or(s.as_str(), |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    };
    let host = host.trim_end_matches('.');
    let host = host.strip_prefix("*.").unwrap_or(host);
    let host = host.strip_prefix("www.").unwrap_or(host);
    (!host.is_empty()).then(|| host.to_owned())
}

/// Whether `h` looks like a dns name with a dotted, alphabetic tld (so "3.5" and "v1.2.3" are not hosts).
fn looks_like_domain(h: &str) -> bool {
    let labels: Vec<&str> = h.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_alphanumeric() || c == '-')
        })
        && labels
            .last()
            .is_some_and(|t| t.chars().count() >= 2 && t.chars().all(|c| c.is_alphabetic()))
}

/// Which allow-listed kind a field is, or `None` when it must not be indexed.
fn classify(field: &str) -> Option<Kind> {
    if is_secret_field(field) {
        return None;
    }
    Some(match field {
        "title" => Kind::Title,
        "provider" => Kind::Provider,
        "username" => Kind::Username,
        "env_var" => Kind::Name,
        "scopes" | "notes" | "body" => Kind::Text,
        _ if field.starts_with("url/") || field.starts_with("allowed_hosts/") => Kind::Host,
        _ if field.starts_with("tags/") => Kind::Tag,
        _ if field.starts_with("custom/") && field.ends_with("/name") => Kind::Name,
        _ => return None,
    })
}

fn make_field(kind: Kind, raw: &str) -> Option<Field> {
    let text = if kind == Kind::Host {
        norm(&host_of(raw)?)
    } else {
        norm(raw)
    };
    let mut tokens = tokens(&text);
    tokens.sort();
    tokens.dedup();
    let tokens: Vec<Tok> = tokens
        .into_iter()
        .map(|s| Tok {
            chars: s.chars().count(),
            mask: mask_of(s.chars()),
            s,
        })
        .collect();
    let mask = tokens.iter().fold(0, |m, t| m | t.mask);
    (!tokens.is_empty()).then_some(Field {
        kind,
        text,
        tokens,
        mask,
    })
}

/// Optimal string alignment distance (adjacent swaps count once), capped: returns `max + 1` once the
/// distance is known to exceed `max`.
fn distance(a: &[char], b: &[char], max: usize) -> usize {
    if a.len().abs_diff(b.len()) > max {
        return max + 1;
    }
    let (n, m) = (a.len(), b.len());
    let mut prev2 = vec![0usize; m + 1];
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        cur[0] = i;
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut d = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d = d.min(prev2[j - 2] + 1);
            }
            cur[j] = d;
        }
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m].min(max + 1)
}

fn max_edits(len: usize) -> usize {
    match len {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    }
}

/// `needle` appears in order in `hay`, starting at a token start and spanning at most twice its length.
fn compact_subsequence(needle: &[char], hay: &str) -> bool {
    if needle.len() < 2 {
        return false;
    }
    let hay: Vec<char> = hay.chars().collect();
    'start: for s in 0..hay.len() {
        if hay[s] != needle[0] || (s > 0 && hay[s - 1].is_alphanumeric()) {
            continue;
        }
        let mut k = 1;
        for (off, c) in hay.iter().enumerate().skip(s + 1) {
            if off - s + 1 > needle.len() * 2 {
                continue 'start;
            }
            if *c == needle[k] {
                k += 1;
                if k == needle.len() {
                    return true;
                }
            }
        }
    }
    false
}

/// A host matches a domain query when equal, when it is a subdomain of the query, or when the query is a
/// subdomain of the host. Labels never match mid-label, so "hub.com" does not find github.com.
fn host_quality(host: &str, q: &str) -> u32 {
    if host == q {
        100
    } else if host.strip_suffix(q).is_some_and(|p| p.ends_with('.')) {
        90
    } else if q.strip_suffix(host).is_some_and(|p| p.ends_with('.')) {
        80
    } else {
        0
    }
}

struct Term {
    /// Normalized, as typed.
    s: String,
    /// Letters and digits only, as chars.
    clean: Vec<char>,
    clean_str: String,
    mask: u64,
    domain: Option<String>,
}

impl Term {
    fn new(raw: &str) -> Option<Self> {
        let s = norm(&raw.chars().take(MAX_TERM_CHARS).collect::<String>());
        let clean: Vec<char> = s.chars().filter(|c| c.is_alphanumeric()).collect();
        if clean.is_empty() {
            return None;
        }
        let domain = host_of(&s).filter(|h| looks_like_domain(h));
        Some(Term {
            clean_str: clean.iter().collect(),
            mask: mask_of(clean.iter().copied()),
            s,
            clean,
            domain,
        })
    }

    /// Best match quality (0 to 100) of this term against one field.
    fn quality(&self, f: &Field) -> u32 {
        let mut best = 0;
        if let (Kind::Host, Some(d)) = (f.kind, &self.domain) {
            // A domain only ever matches a host on label boundaries, never as a loose substring.
            return host_quality(&f.text, d);
        }
        let edits = max_edits(self.clean.len());
        let len = self.clean.len();
        for tok in &f.tokens {
            if tok.s == self.clean_str {
                return 100;
            }
            if tok.s.starts_with(&self.clean_str) {
                best = best.max(85);
            } else if best < 55
                && edits > 0
                && (self.mask & !tok.mask).count_ones() as usize <= edits
            {
                let t: Vec<char> = tok.s.chars().collect();
                if tok.chars.abs_diff(len) <= edits && distance(&self.clean, &t, edits) <= edits {
                    best = best.max(55);
                } else if t.len() > len && distance(&self.clean, &t[..len], edits) <= edits {
                    best = best.max(45);
                }
            }
        }
        if best < 60 && f.text.contains(&self.s) {
            best = 60;
        }
        if best < 35 && self.mask & !f.mask == 0 && compact_subsequence(&self.clean, &f.text) {
            best = 35;
        }
        best
    }
}

/// One way a query term can match: the term itself, an alias target, or a provider implied by a token
/// prefix. `cap` limits how good an indirect match can score.
struct Variant {
    term: Term,
    cap: u32,
    /// Only provider and title fields of token items.
    token_items_only: bool,
}

fn variants(raw: &str, aliases: &[(String, String)]) -> Vec<Variant> {
    let Some(term) = Term::new(raw) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (alias, target) in aliases {
        if *alias == term.s
            && let Some(t) = Term::new(target)
        {
            out.push(Variant {
                term: t,
                cap: 95,
                token_items_only: false,
            });
        }
    }
    // A pasted token must only ever lend its well-known prefix; the rest is dropped here and the term
    // itself still goes through the ordinary path, which can only see non-secret fields.
    if term.clean.len() >= 3 {
        // The longest known prefix the term starts with wins ("sk-ant-" over "sk-"); a short term that is
        // only the start of known prefixes ("ghp") falls back to all of those.
        let longest = TOKEN_PREFIXES
            .iter()
            .filter(|(p, _)| term.s.starts_with(p))
            .max_by_key(|(p, _)| p.len());
        let matched: Vec<&(&str, &[&str])> = match longest {
            Some(row) => vec![row],
            None => TOKEN_PREFIXES
                .iter()
                .filter(|(p, _)| p.starts_with(&term.s))
                .collect(),
        };
        for (_, providers) in matched {
            out.extend(
                providers
                    .iter()
                    .filter_map(|p| Term::new(p))
                    .map(|t| Variant {
                        term: t,
                        cap: 90,
                        token_items_only: true,
                    }),
            );
        }
    }
    out.push(Variant {
        term,
        cap: 100,
        token_items_only: false,
    });
    out
}

impl Index {
    /// Index the readable, non-deleted items. Secret field values are never copied.
    pub fn build<'a>(items: impl IntoIterator<Item = &'a ItemView>) -> Self {
        let entries = items
            .into_iter()
            .filter(|v| !v.deleted)
            .map(|v| Entry {
                id: v.id,
                item_type: v.item_type,
                updated_ms: v.updated.wall,
                fields: v
                    .fields
                    .iter()
                    .filter_map(|(name, value)| {
                        let kind = classify(name)?;
                        // A tag is carried by its field name; the value is not meaningful.
                        let raw = match kind {
                            Kind::Tag => name.strip_prefix("tags/").unwrap_or(""),
                            _ => value.as_str(),
                        };
                        make_field(kind, raw)
                    })
                    .collect(),
            })
            .collect();
        Index {
            entries,
            aliases: Vec::new(),
        }
    }

    /// Set the alias table: each `(alias, target)` makes a query term equal to `alias` also match
    /// `target`, so ("cf", "cloudflare") lets "cf" find Cloudflare. Replaces any earlier table.
    pub fn set_aliases(&mut self, aliases: &[(&str, &str)]) {
        self.aliases = aliases
            .iter()
            .map(|(a, t)| (norm(a), norm(t)))
            .filter(|(a, t)| !a.is_empty() && !t.is_empty())
            .collect();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Items matching every term of `query`, best first, at most `limit`. An empty query lists items
    /// most recently updated first. `now_ms` only feeds the recency boost.
    pub fn search(&self, query: &str, now_ms: u64, limit: usize) -> Vec<Hit> {
        let groups: Vec<Vec<Variant>> = query
            .split_whitespace()
            .take(MAX_TERMS)
            .map(|t| variants(t, &self.aliases))
            .filter(|g| !g.is_empty())
            .collect();
        // Only punctuation was typed: nothing can match, which is not the same as asking for the list.
        if groups.is_empty() && query.split_whitespace().next().is_some() {
            return Vec::new();
        }
        let mut hits: Vec<Hit> = self
            .entries
            .iter()
            .filter_map(|e| {
                let mut score = 0;
                for g in &groups {
                    score += Self::group_score(e, g)?;
                }
                Some(Hit {
                    id: e.id,
                    score: score + recency_boost(e.updated_ms, now_ms),
                })
            })
            .collect();
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
        hits.truncate(limit);
        hits
    }

    /// Best weighted match of any variant of one term in any field, or `None` when nothing matches.
    fn group_score(e: &Entry, group: &[Variant]) -> Option<u32> {
        let mut best = 0;
        for v in group {
            if v.token_items_only && e.item_type != ItemType::Token {
                continue;
            }
            for f in &e.fields {
                if v.token_items_only && !matches!(f.kind, Kind::Provider | Kind::Title) {
                    continue;
                }
                let q = v.term.quality(f).min(v.cap);
                best = best.max(f.kind.weight() * q / 100);
            }
        }
        (best > 0).then_some(best)
    }
}

/// Up to `MAX_BOOST` points, halving roughly every 30 days. Integer math, so results are identical on
/// every platform.
fn recency_boost(updated_ms: u64, now_ms: u64) -> u32 {
    let age_days = now_ms.saturating_sub(updated_ms) / DAY_MS;
    let steps = age_days / BOOST_HALF_LIFE_DAYS;
    let rem = age_days % BOOST_HALF_LIFE_DAYS;
    let base = (MAX_BOOST * (2 * BOOST_HALF_LIFE_DAYS - rem)) / (2 * BOOST_HALF_LIFE_DAYS);
    u32::try_from(
        base.checked_shr(u32::try_from(steps).unwrap_or(u32::MAX))
            .unwrap_or(0),
    )
    .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn host_extraction_strips_decoration() {
        for (raw, want) in [
            ("https://www.GitHub.com/login?x=1", Some("github.com")),
            (
                "https://user:pw@api.example.org:8443/a#b",
                Some("api.example.org"),
            ),
            ("*.example.com", Some("example.com")),
            ("example.com.", Some("example.com")),
            ("http://[::1]:3000/x", Some("::1")),
            ("localhost:3000", Some("localhost")),
            ("", None),
            ("https://", None),
        ] {
            assert_eq!(host_of(raw).as_deref(), want, "{raw}");
        }
    }

    #[test]
    fn domain_shape() {
        assert!(looks_like_domain("github.com"));
        assert!(looks_like_domain("a-b.example.co"));
        for no in ["v1.2.3", "3.5", "foo", "a..b", "-a.com", "a.c"] {
            assert!(!looks_like_domain(no), "{no}");
        }
    }

    #[test]
    fn host_match_respects_label_boundaries() {
        assert_eq!(host_quality("github.com", "github.com"), 100);
        assert_eq!(host_quality("gist.github.com", "github.com"), 90);
        assert_eq!(host_quality("github.com", "api.github.com"), 80);
        assert_eq!(host_quality("github.com", "hub.com"), 0);
        assert_eq!(host_quality("notgithub.com", "github.com"), 0);
    }

    #[test]
    fn edit_distance_counts_swaps_once_and_caps() {
        assert_eq!(distance(&chars("github"), &chars("gihtub"), 2), 1);
        assert_eq!(distance(&chars("github"), &chars("gthub"), 2), 1);
        assert_eq!(distance(&chars("github"), &chars("gitlab"), 2), 2);
        assert_eq!(distance(&chars("github"), &chars("zzzzzz"), 1), 2);
        assert_eq!(distance(&chars("a"), &chars("abcdef"), 2), 3);
    }

    #[test]
    fn subsequence_is_compact_and_starts_at_a_token() {
        let n = |s: &str| chars(s);
        assert!(compact_subsequence(&n("cldflr"), "cloudflare"));
        assert!(!compact_subsequence(&n("cf"), "cloudflare"));
        assert!(!compact_subsequence(&n("loud"), "cloudflare"));
        assert!(compact_subsequence(&n("gthb"), "my github"));
        assert!(!compact_subsequence(&n("x"), "x"));
    }

    #[test]
    fn recency_decays_and_never_underflows() {
        let now = 400 * DAY_MS;
        assert_eq!(recency_boost(now, now), 20);
        assert_eq!(recency_boost(now + DAY_MS, now), 20, "future stamp");
        assert!(recency_boost(now - 15 * DAY_MS, now) < 20);
        assert_eq!(recency_boost(now - 30 * DAY_MS, now), 10);
        assert_eq!(recency_boost(now - 60 * DAY_MS, now), 5);
        assert_eq!(recency_boost(0, u64::MAX), 0);
    }

    #[test]
    fn normalization_folds_case_and_diacritics() {
        assert_eq!(norm("Café ÉCOLE"), "cafe ecole");
    }

    fn view(id: u8, t: ItemType, fields: &[(&str, &str)]) -> ItemView {
        ItemView {
            id: [id; 16],
            item_type: t,
            vault: String::new(),
            deleted: false,
            fields: fields
                .iter()
                .map(|(k, v)| ((*k).to_owned(), zeroize::Zeroizing::new((*v).to_owned())))
                .collect(),
            updated: crate::Stamp::ZERO,
        }
    }

    use proptest::prelude::*;

    proptest! {
        /// Whatever the query, results are unique, ordered best first and within the limit, and the
        /// search never panics.
        #[test]
        fn search_is_total_and_ordered(q in "\\PC{0,80}", limit in 0usize..8) {
            let items: Vec<ItemView> = ["Cloudflare", "GitHub", "Neon Café", "ssh box", ""]
                .iter()
                .enumerate()
                .map(|(i, t)| view(i as u8, ItemType::Login, &[("title", t), ("url/a", "https://x.example.com")]))
                .collect();
            let ix = Index::build(&items);
            let hits = ix.search(&q, 1_000_000, limit);
            prop_assert!(hits.len() <= limit);
            for w in hits.windows(2) {
                prop_assert!(w[0].score > w[1].score || (w[0].score == w[1].score && w[0].id < w[1].id));
            }
        }

        /// A secret value, or any long fragment of it, never finds the item that holds it.
        #[test]
        fn secret_values_never_match(secret in "[a-z0-9]{12,40}", t in 0usize..4) {
            let (ty, field) = [
                (ItemType::Login, "password"),
                (ItemType::Token, "token"),
                (ItemType::SshKey, "private_key"),
                (ItemType::Login, "totp"),
            ][t];
            let items = [view(1, ty, &[("title", "Example Service"), ("username", "someone"), (field, &secret)])];
            let ix = Index::build(&items);
            prop_assert!(ix.search(&secret, 0, 5).is_empty());
            prop_assert!(ix.search(&secret[..12], 0, 5).is_empty());
        }

        /// Any single edit of a title word of four or more letters still finds the item.
        #[test]
        fn one_typo_still_finds(word in "[a-z]{5,12}", pos in 0usize..12, kind in 0u8..3) {
            let items = [view(1, ItemType::Login, &[("title", &word)])];
            let ix = Index::build(&items);
            let mut w: Vec<char> = word.chars().collect();
            let p = pos % w.len();
            match kind {
                0 => { w.remove(p); }
                1 => { w[p] = if w[p] == 'q' { 'z' } else { 'q' }; }
                _ => { w.insert(p, 'q'); }
            }
            let typo: String = w.into_iter().collect();
            prop_assert_eq!(ix.search(&typo, 0, 5).len(), 1, "{} -> {}", word, typo);
        }
    }

    #[test]
    fn only_allow_listed_fields_are_indexed() {
        for secret in [
            "password",
            "totp",
            "token",
            "private_key",
            "passphrase",
            "env/API_KEY",
            "custom/ab12/value",
            "public_key",
            "_ik",
            "future_secret",
        ] {
            assert!(classify(secret).is_none(), "{secret}");
        }
        assert!(classify("custom/ab12/name").is_some());
        assert!(classify("tags/work").is_some());
    }
}

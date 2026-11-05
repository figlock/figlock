//! Search tests (FIG-27) through the public API: items are written into a real vault, read back decrypted and
//! indexed. Includes the negative cases from `AGENTS.md`: secret values must never be findable.

use figlock_core::{Hit, Index, ItemId, ItemType, KdfParams, Vault};

const T0: u64 = 1_700_000_000_000;
const DAY: u64 = 86_400_000;

/// A generated value, so no real or well-known secret appears in the tests.
fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).unwrap();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn vault() -> Vault {
    Vault::create(&random_hex(16), "Laptop", KdfParams::FLOOR, T0)
        .unwrap()
        .0
}

fn index(v: &mut Vault) -> Index {
    let (items, corrupt) = v.items(false);
    assert!(corrupt.is_empty());
    Index::build(&items)
}

fn login(v: &mut Vault, title: &str, url: &str, now: u64) -> ItemId {
    v.add_item(
        ItemType::Login,
        "personal",
        &[
            ("title", title),
            ("username", "someone@example.com"),
            ("url/a1", url),
            ("password", &random_hex(12)),
        ],
        now,
    )
    .unwrap()
}

fn ids(hits: &[Hit]) -> Vec<ItemId> {
    hits.iter().map(|h| h.id).collect()
}

#[test]
fn finds_by_exact_prefix_and_substring() {
    let mut v = vault();
    let cf = login(&mut v, "Cloudflare", "https://dash.cloudflare.com", T0);
    login(&mut v, "Netflix", "https://netflix.com", T0);
    let ix = index(&mut v);
    for q in ["cloudflare", "CLOUD", "flare", "cloudflare someone"] {
        assert_eq!(ids(&ix.search(q, T0, 10)), vec![cf], "{q}");
    }
}

#[test]
fn tolerates_typos_and_fuzzy_subsequences() {
    let mut v = vault();
    let cf = login(&mut v, "Cloudflare", "https://dash.cloudflare.com", T0);
    let gh = login(&mut v, "GitHub", "https://github.com", T0);
    let ix = index(&mut v);
    assert_eq!(
        ids(&ix.search("clodflare", T0, 10)),
        vec![cf],
        "dropped letter"
    );
    assert_eq!(
        ids(&ix.search("cloudflrae", T0, 10)),
        vec![cf],
        "swapped letters"
    );
    assert_eq!(
        ids(&ix.search("githbu", T0, 10)),
        vec![gh],
        "swapped letters"
    );
    assert_eq!(ids(&ix.search("cldflr", T0, 10)), vec![cf], "subsequence");
    assert!(ix.search("zebra", T0, 10).is_empty());
    // Too short to guess at: no edits are allowed under four characters.
    assert!(ix.search("gix", T0, 10).is_empty());
}

#[test]
fn every_term_must_match() {
    let mut v = vault();
    let a = login(&mut v, "GitHub Personal", "https://github.com", T0);
    login(&mut v, "GitHub Work", "https://github.com", T0);
    let ix = index(&mut v);
    assert_eq!(ids(&ix.search("github personal", T0, 10)), vec![a]);
    assert!(ix.search("github missing", T0, 10).is_empty());
}

#[test]
fn domain_queries_match_on_label_boundaries() {
    let mut v = vault();
    let gh = login(&mut v, "Code host", "https://www.GitHub.com/login", T0);
    let gist = login(&mut v, "Snippets", "https://gist.github.com", T0);
    login(&mut v, "Other", "https://notgithub.com", T0);
    let ix = index(&mut v);
    let found = |q: &str| {
        let mut f = ids(&ix.search(q, T0, 10));
        f.sort();
        f
    };
    let mut both = vec![gh, gist];
    both.sort();
    assert_eq!(found("github.com"), both);
    assert_eq!(found("https://github.com/anything?x=1"), both);
    assert_eq!(ids(&ix.search("gist.github.com", T0, 10))[0], gist);
    // A mid-label fragment of a domain does not match as a domain.
    assert!(found("hub.com").is_empty());
}

#[test]
fn allowed_hosts_of_a_token_are_searchable_by_domain() {
    let mut v = vault();
    let t = v
        .add_item(
            ItemType::Token,
            "work",
            &[
                ("title", "CI deploy"),
                ("allowed_hosts/a1", "*.example.org:8443"),
                ("token", &random_hex(20)),
            ],
            T0,
        )
        .unwrap();
    let ix = index(&mut v);
    assert_eq!(ids(&ix.search("api.example.org", T0, 10)), vec![t]);
}

#[test]
fn token_prefix_finds_the_provider_but_never_the_token() {
    let mut v = vault();
    let secret = format!("ghp_{}", random_hex(18));
    let gh = v
        .add_item(
            ItemType::Token,
            "work",
            &[
                ("title", "CI deploy key"),
                ("provider", "GitHub"),
                ("token", &secret),
            ],
            T0,
        )
        .unwrap();
    let other = v
        .add_item(
            ItemType::Token,
            "work",
            &[
                ("title", "Billing"),
                ("provider", "Stripe"),
                ("token", &format!("sk_live_{}", random_hex(10))),
            ],
            T0,
        )
        .unwrap();
    // A login at the same provider is not a token item.
    login(&mut v, "GitHub", "https://github.com", T0);
    let ix = index(&mut v);
    assert_eq!(ids(&ix.search("ghp_", T0, 10)), vec![gh]);
    assert_eq!(ids(&ix.search("sk_live_", T0, 10)), vec![other]);
    // A pasted token only contributes its well-known prefix; the rest is matched against non-secret
    // fields only.
    assert_eq!(ids(&ix.search(&secret, T0, 10)), vec![gh]);
    // The longest known prefix wins, so a pasted Anthropic-style key does not lend "sk-" to OpenAI.
    assert!(ix.search("sk-ant-xxxxxxxx", T0, 10).is_empty());
}

#[test]
fn secret_values_are_never_findable() {
    let mut v = vault();
    let pw = random_hex(12);
    let tok = random_hex(20);
    let env = random_hex(12);
    let custom = random_hex(12);
    let key = random_hex(16);
    v.add_item(
        ItemType::Login,
        "personal",
        &[
            ("title", "Example"),
            ("password", &pw),
            ("totp", &random_hex(8)),
        ],
        T0,
    )
    .unwrap();
    v.add_item(
        ItemType::Token,
        "personal",
        &[("title", "Example token"), ("token", &tok)],
        T0,
    )
    .unwrap();
    v.add_item(
        ItemType::EnvBundle,
        "personal",
        &[("title", "Example env"), ("env/API_KEY", &env)],
        T0,
    )
    .unwrap();
    v.add_item(
        ItemType::Login,
        "personal",
        &[
            ("title", "Example custom"),
            ("custom/ab12/name", "recovery"),
            ("custom/ab12/value", &custom),
        ],
        T0,
    )
    .unwrap();
    v.add_item(
        ItemType::SshKey,
        "personal",
        &[
            ("title", "Example ssh"),
            ("private_key", &key),
            ("passphrase", &random_hex(8)),
        ],
        T0,
    )
    .unwrap();
    let ix = index(&mut v);
    for secret in [&pw, &tok, &env, &custom, &key] {
        assert!(ix.search(secret, T0, 10).is_empty(), "secret found");
        // Nor a long fragment of it.
        assert!(
            ix.search(&secret[..10], T0, 10).is_empty(),
            "fragment found"
        );
    }
    // Non-secret siblings are still found, including a custom field's name.
    assert_eq!(ix.search("recovery", T0, 10).len(), 1);
    assert_eq!(ix.search("example", T0, 10).len(), 5);
    // Names of env vars are secret-adjacent but the key name sits in the field id, which is not indexed.
    assert!(ix.search("API_KEY", T0, 10).is_empty());
}

#[test]
fn tags_notes_and_usernames_are_searched() {
    let mut v = vault();
    let a = v
        .add_item(
            ItemType::Login,
            "personal",
            &[
                ("title", "Router"),
                ("tags/home-lab", "1"),
                ("notes", "behind the garage door"),
            ],
            T0,
        )
        .unwrap();
    let ix = index(&mut v);
    assert_eq!(ids(&ix.search("home-lab", T0, 10)), vec![a]);
    assert_eq!(ids(&ix.search("garage", T0, 10)), vec![a]);
}

#[test]
fn aliases_expand_a_short_word() {
    let mut v = vault();
    let cf = login(&mut v, "Cloudflare", "https://dash.cloudflare.com", T0);
    login(&mut v, "Netflix", "https://netflix.com", T0);
    let mut ix = index(&mut v);
    assert!(ix.search("cf", T0, 10).is_empty(), "no alias yet");
    ix.set_aliases(&[("CF", "cloudflare")]);
    assert_eq!(ids(&ix.search("cf", T0, 10)), vec![cf]);
    // The alias table replaces, it does not accumulate.
    ix.set_aliases(&[]);
    assert!(ix.search("cf", T0, 10).is_empty());
}

#[test]
fn recent_items_rank_first_and_empty_query_lists_by_recency() {
    let mut v = vault();
    let now = T0 + 200 * DAY;
    // Created oldest first: the vault clock never runs backwards.
    let old = login(&mut v, "Mail", "https://mail.example.com", T0);
    let mid = login(&mut v, "Chat", "https://chat.example.com", now - 40 * DAY);
    let new = login(&mut v, "Mail", "https://mail.example.org", now - DAY);
    let ix = index(&mut v);
    assert_eq!(ids(&ix.search("mail", now, 10)), vec![new, old]);
    assert_eq!(ids(&ix.search("", now, 10)), vec![new, mid, old]);
    assert_eq!(ids(&ix.search("   ", now, 2)), vec![new, mid]);
    assert!(ix.search("mail", now, 0).is_empty());
}

#[test]
fn deleted_items_are_not_indexed() {
    let mut v = vault();
    let gone = login(&mut v, "Cloudflare", "https://cloudflare.com", T0);
    let kept = login(&mut v, "Cloudflare backup", "https://cloudflare.com", T0);
    v.delete_item(&gone, T0 + 1).unwrap();
    let (items, _) = v.items(true);
    assert_eq!(items.len(), 2);
    let ix = Index::build(&items);
    assert_eq!(ix.len(), 1);
    assert_eq!(ids(&ix.search("cloudflare", T0, 10)), vec![kept]);
}

#[test]
fn hostile_queries_are_cheap_and_safe() {
    let mut v = vault();
    login(&mut v, "Cloudflare", "https://cloudflare.com", T0);
    let ix = index(&mut v);
    let long = "x".repeat(100_000);
    assert!(ix.search(&long, T0, 10).is_empty());
    let many = "cloud ".repeat(10_000);
    assert_eq!(ix.search(&many, T0, 10).len(), 1);
    for q in [
        "\0", "\u{202e}", "💥💥", "é", "..", "://", "[::1", "*.", "%",
    ] {
        let _ = ix.search(q, T0, 10);
    }
    // Typing only punctuation is a query that matches nothing, not a request for the whole list.
    assert!(ix.search("%%", T0, 10).is_empty());
    assert_eq!(ix.search("", T0, 10).len(), 1);
}

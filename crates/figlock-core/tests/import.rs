//! Import tests (FIG-30): the four CSV exports and the notes dump, with the negative cases from `AGENTS.md`
//! (malformed input, huge fields, hostile text, secrets never echoed). Token values are generated here, never
//! written out as literals.

use figlock_core::{
    Draft, ImportError, ItemType, MAX_INPUT_BYTES, MAX_RECORDS, MAX_VALUE_BYTES, SkipReason,
    Source, detect_tokens, is_secret_field, parse_import, validate_field,
};
use proptest::prelude::*;

/// `n` characters drawn from `alphabet`, varied by `seed` so two tokens differ.
fn sample(alphabet: &str, n: usize, seed: usize) -> String {
    let a: Vec<char> = alphabet.chars().collect();
    (0..n)
        .map(|i| a[(i * 7 + seed * 13 + 3) % a.len()])
        .collect()
}

const ALNUM: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
const ALNUM_DASH: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-";
const UPPER_DIGIT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn gh(seed: usize) -> String {
    format!("ghp_{}", sample(ALNUM, 36, seed))
}
fn anthropic(seed: usize) -> String {
    format!("sk-ant-{}", sample(ALNUM_DASH, 80, seed))
}
fn aws(seed: usize) -> String {
    format!("AKIA{}", sample(UPPER_DIGIT, 16, seed))
}

fn field<'a>(d: &'a Draft, name: &str) -> Option<&'a str> {
    d.fields
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

fn run(source: Source, input: &str) -> figlock_core::Report {
    parse_import(source, input.as_bytes()).expect("parses")
}

/// Every draft must be addable: valid field names for its type, values within the limit.
fn assert_addable(items: &[Draft]) {
    for d in items {
        assert!(field(d, "title").is_some(), "every item has a title");
        for (name, value) in &d.fields {
            validate_field(d.item_type, name).unwrap_or_else(|_| panic!("bad field {name}"));
            assert!(value.len() <= MAX_VALUE_BYTES);
            assert!(!value.is_empty(), "empty values are not written");
        }
    }
}

// ---- CSV exports ----

#[test]
fn chrome_export_with_quotes_commas_newlines_and_crlf() {
    let csv = "name,url,username,password,note\r\n\
               Example,https://www.example.com/login,ada,\" p,\"\"w\"\" \",\"line one\nline two\"\r\n\
               ,https://user@Mail.Example.org:8443/x?y=1,bob,pw2,\r\n";
    let r = run(Source::Chrome, csv);
    assert_eq!(r.items.len(), 2);
    assert!(r.skipped.is_empty());
    let a = &r.items[0];
    assert_eq!(a.item_type, ItemType::Login);
    assert_eq!(field(a, "title"), Some("Example"));
    assert_eq!(field(a, "username"), Some("ada"));
    // The password is kept exactly: quotes, comma and surrounding spaces.
    assert_eq!(field(a, "password"), Some(" p,\"w\" "));
    assert_eq!(field(a, "url/1"), Some("https://www.example.com/login"));
    assert_eq!(field(a, "notes"), Some("line one\nline two"));
    // No title in the export: the host stands in, without credentials, port or www.
    assert_eq!(field(&r.items[1], "title"), Some("mail.example.org"));
    assert_addable(&r.items);
}

#[test]
fn apple_export_keeps_totp_uri() {
    let uri = format!("otpauth://totp/x?secret={}", sample(UPPER_DIGIT, 16, 1));
    let csv = format!(
        "Title,URL,Username,Password,Notes,OTPAuth\nBank,https://bank.example,ada,pw,hello,{uri}\n"
    );
    let r = run(Source::Apple, &csv);
    assert_eq!(field(&r.items[0], "totp"), Some(uri.as_str()));
    assert!(is_secret_field("totp"));
    assert_addable(&r.items);
}

#[test]
fn bitwarden_export_logins_notes_and_unsupported_types() {
    let csv = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
        Work,1,login,Intranet,,\"Employee id: 42\nPIN: 9911\",0,\"https://a.example\nhttps://b.example\",ada,pw,\n\
        ,,note,Wifi,\"router admin is on the fridge\",,0,,,,\n\
        ,,card,Visa,,,0,,,,\n\
        ,,identity,Me,,,0,,,,\n";
    let r = run(Source::Bitwarden, csv);
    assert_eq!(r.items.len(), 2);
    let login = &r.items[0];
    assert_eq!(field(login, "url/1"), Some("https://a.example"));
    assert_eq!(field(login, "url/2"), Some("https://b.example"));
    assert_eq!(field(login, "tags/work"), Some("1"));
    assert_eq!(field(login, "custom/1/name"), Some("Employee id"));
    assert_eq!(field(login, "custom/2/value"), Some("9911"));
    let note = &r.items[1];
    assert_eq!(note.item_type, ItemType::Note);
    assert_eq!(field(note, "body"), Some("router admin is on the fridge"));
    assert_eq!(
        r.skipped
            .iter()
            .map(|s| (s.record, s.reason))
            .collect::<Vec<_>>(),
        vec![
            (4, SkipReason::UnsupportedType),
            (5, SkipReason::UnsupportedType)
        ]
    );
    assert_addable(&r.items);
}

#[test]
fn onepassword_export_tags_and_empty_rows() {
    let csv = "Title,Url,Username,Password,OTPAuth,Favorite,Archived,Tags,Notes\n\
        Git,https://git.example,ada,pw,,0,0,\"Dev Ops, home lab,,!!\",\n\
        ,,,,,0,0,,\n";
    let r = run(Source::OnePassword, csv);
    assert_eq!(r.items.len(), 1);
    let d = &r.items[0];
    assert_eq!(field(d, "tags/dev-ops"), Some("1"));
    assert_eq!(field(d, "tags/home-lab"), Some("1"));
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].reason, SkipReason::Empty);
    assert_addable(&r.items);
}

#[test]
fn bom_is_ignored_and_header_case_does_not_matter() {
    let csv = "\u{feff}NAME,URL,Username,PASSWORD\nx,https://x.example,u,p\n";
    assert_eq!(run(Source::Chrome, csv).items.len(), 1);
}

// ---- negative cases ----

#[test]
fn a_file_from_another_source_is_refused() {
    let chrome = "name,url,username,password\nx,y,z,w\n";
    for s in [Source::Apple, Source::Bitwarden, Source::OnePassword] {
        assert_eq!(
            parse_import(s, chrome.as_bytes()).err(),
            Some(ImportError::WrongFormat)
        );
    }
    assert_eq!(
        parse_import(Source::Chrome, b"").err(),
        Some(ImportError::WrongFormat)
    );
}

#[test]
fn malformed_and_oversized_input_is_refused() {
    let unterminated = "name,url,username,password\n\"never closed,https://x,u,p\n";
    assert_eq!(
        parse_import(Source::Chrome, unterminated.as_bytes()).err(),
        Some(ImportError::Malformed)
    );
    let wide = format!("{}\n", vec!["a"; 100].join(","));
    assert_eq!(
        parse_import(Source::Chrome, wide.as_bytes()).err(),
        Some(ImportError::Malformed)
    );
    assert_eq!(
        parse_import(Source::Chrome, &[0xff, 0xfe, b'a']).err(),
        Some(ImportError::NotUtf8)
    );
    assert_eq!(
        parse_import(Source::Notes, &[0xc3]).err(),
        Some(ImportError::NotUtf8)
    );
    let huge = vec![b'a'; MAX_INPUT_BYTES + 1];
    for s in [Source::Chrome, Source::Notes] {
        assert_eq!(parse_import(s, &huge).err(), Some(ImportError::TooLarge));
    }
    let many = format!(
        "name,url,username,password\n{}",
        "a,b,c,d\n".repeat(MAX_RECORDS + 1)
    );
    assert_eq!(
        parse_import(Source::Chrome, many.as_bytes()).err(),
        Some(ImportError::TooManyRecords)
    );
    let many_notes = "x\n---\n".repeat(MAX_RECORDS + 1);
    assert_eq!(
        parse_import(Source::Notes, many_notes.as_bytes()).err(),
        Some(ImportError::TooManyRecords)
    );
}

#[test]
fn a_huge_field_skips_that_record_only() {
    let big = "a".repeat(MAX_VALUE_BYTES + 1);
    let csv = format!(
        "name,url,username,password\nbig,https://x.example,u,{big}\nok,https://y.example,u,p\n"
    );
    let r = run(Source::Chrome, &csv);
    assert_eq!(r.items.len(), 1);
    assert_eq!(field(&r.items[0], "title"), Some("ok"));
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].reason, SkipReason::FieldTooLarge);
    assert_eq!(r.skipped[0].record, 2);

    let note = format!("{big}\n");
    let r = run(Source::Notes, &note);
    assert!(r.items.is_empty());
    assert_eq!(r.skipped[0].reason, SkipReason::FieldTooLarge);
}

#[test]
fn errors_never_contain_the_input() {
    let secret = sample(ALNUM, 24, 9);
    let inputs: Vec<Vec<u8>> = vec![
        format!("name,url,username,password\n\"{secret}\n").into_bytes(),
        format!("{secret},{secret}\n").into_bytes(),
        [secret.as_bytes(), &[0xff]].concat(),
    ];
    for input in inputs {
        for s in [
            Source::Chrome,
            Source::Apple,
            Source::Bitwarden,
            Source::OnePassword,
            Source::Notes,
        ] {
            if let Err(e) = parse_import(s, &input) {
                assert!(!e.to_string().contains(&secret));
                assert!(!format!("{e:?}").contains(&secret));
            }
        }
    }
}

#[test]
fn hostile_text_is_kept_as_data_and_stays_valid() {
    // Escape sequences, NUL, bidi marks and formula starts are stored as text; display code cleans them.
    let csv = "name,url,username,password\n\"=cmd|' /c calc'!A1\",\"\u{1b}[2J\",\"a\u{0}b\",\"\u{202e}pw\"\n";
    let r = run(Source::Chrome, csv);
    assert_eq!(r.items.len(), 1);
    assert_addable(&r.items);
}

// ---- token detection ----

#[test]
fn detects_each_provider_and_reports_exact_spans() {
    let cases: Vec<(String, &str)> = vec![
        (gh(1), "github"),
        (format!("github_pat_{}", sample(ALNUM, 82, 2)), "github"),
        (format!("glpat-{}", sample(ALNUM_DASH, 20, 3)), "gitlab"),
        (anthropic(4), "anthropic"),
        (format!("sk-proj-{}", sample(ALNUM_DASH, 60, 5)), "openai"),
        (format!("sk-{}", sample(ALNUM, 48, 6)), "openai"),
        (aws(7), "aws"),
        (
            format!("xoxb-{}", sample("0123456789-abcdef", 40, 8)),
            "slack",
        ),
        (format!("sk_live_{}", sample(ALNUM, 32, 9)), "stripe"),
        (format!("npm_{}", sample(ALNUM, 36, 10)), "npm"),
        (format!("AIza{}", sample(ALNUM_DASH, 35, 11)), "google"),
    ];
    for (token, provider) in cases {
        let text = format!("my key is {token}, keep it safe");
        let found = detect_tokens(&text);
        assert_eq!(found.len(), 1, "{provider}");
        assert_eq!(found[0].provider, provider);
        assert_eq!(&text[found[0].start..found[0].end], token);
    }
}

#[test]
fn does_not_report_lookalikes() {
    let not_tokens = [
        format!("ghp_{}", sample(ALNUM, 35, 1)), // one too short
        format!("sk-{}", sample(ALNUM, 47, 2)),  // one too short
        format!("sk-{}", sample(ALNUM, 49, 3)),  // too long for the legacy shape
        format!("AKIA{}", sample(UPPER_DIGIT, 15, 4)), // short
        format!("AKIA{}", sample(UPPER_DIGIT, 17, 5)), // long
        format!("AKIA{}", sample("abcdef", 16, 6)), // lowercase
        format!("x{}", gh(7)),                   // inside a word
        format!("my-{}", gh(8)),                 // glued to a dashed word
        "sk-ask-antelope-ask-antelope-ask-antelope-x".to_owned(),
        "the quick brown fox ghp_ sk- AKIA".to_owned(),
    ];
    for t in not_tokens {
        assert!(detect_tokens(&t).is_empty(), "{t}");
    }
}

#[test]
fn names_the_variable_a_token_is_assigned_to() {
    let t = gh(1);
    for (text, want) in [
        (format!("GITHUB_TOKEN={t}"), Some("GITHUB_TOKEN")),
        (format!("export GITHUB_TOKEN=\"{t}\""), Some("GITHUB_TOKEN")),
        (format!("MY_KEY: {t}"), Some("MY_KEY")),
        (format!("lowercase_name={t}"), None),
        (format!("just {t}"), None),
    ] {
        let found = detect_tokens(&text);
        assert_eq!(found[0].env_var.as_deref(), want, "{text}");
    }
}

// ---- notes dump ----

#[test]
fn notes_dump_files_tokens_and_removes_them_from_the_note() {
    let (g, a, k) = (gh(1), anthropic(2), aws(3));
    let dump = format!(
        "# Cloud keys\nGITHUB_TOKEN={g}\nanthropic: {a}\nthe same again {g}\n\
         ---\nShopping\nmilk\neggs\n\
         ===\n\n\
         ---\nAWS\n{k}\n"
    );
    let r = run(Source::Notes, &dump);
    let notes: Vec<_> = r
        .items
        .iter()
        .filter(|d| d.item_type == ItemType::Note)
        .collect();
    let tokens: Vec<_> = r
        .items
        .iter()
        .filter(|d| d.item_type == ItemType::Token)
        .collect();
    assert_eq!(notes.len(), 3);
    assert_eq!(tokens.len(), 3, "the repeated token is filed once");
    assert_eq!(r.tokens_detected, 3);
    assert_eq!(field(notes[0], "title"), Some("Cloud keys"));
    assert_eq!(field(notes[1], "title"), Some("Shopping"));
    assert_eq!(field(notes[1], "body"), Some("milk\neggs"));
    // No note body keeps any token value; each is replaced by a pointer.
    for n in &notes {
        let body = field(n, "body").unwrap();
        for t in [&g, &a, &k] {
            assert!(!body.contains(t.as_str()));
        }
    }
    assert!(
        field(notes[0], "body")
            .unwrap()
            .contains("[token filed as: GitHub token (Cloud keys)]")
    );
    let gh_item = tokens
        .iter()
        .find(|d| field(d, "token") == Some(g.as_str()))
        .unwrap();
    assert_eq!(field(gh_item, "provider"), Some("github"));
    assert_eq!(field(gh_item, "env_var"), Some("GITHUB_TOKEN"));
    assert_eq!(field(gh_item, "tags/imported"), Some("1"));
    assert!(is_secret_field("token"));
    assert_eq!(
        r.skipped.len(),
        1,
        "the empty section is reported, not imported"
    );
    assert_addable(&r.items);
}

#[test]
fn a_token_on_the_first_line_does_not_become_a_title() {
    let t = gh(1);
    let r = run(Source::Notes, &format!("# {t}\nsecond line\n"));
    for d in &r.items {
        for (name, value) in &d.fields {
            if name != "token" {
                assert!(!value.contains(t.as_str()), "{name} leaks the token");
            }
        }
    }
    assert_eq!(r.tokens_detected, 1);
    assert_eq!(field(&r.items[0], "title"), Some("[token]"));
}

#[test]
fn two_tokens_glued_together_are_both_filed() {
    let (a, g) = (aws(1), gh(2));
    let r = run(Source::Notes, &format!("{a}{g}\n"));
    assert_eq!(r.tokens_detected, 2);
    assert_no_token_text(&r.items);
}

/// Outside the `token` field of a token item, nothing a notes import writes may still look like a token.
fn assert_no_token_text(items: &[Draft]) {
    for d in items {
        for (name, value) in &d.fields {
            if d.item_type == ItemType::Token && name == "token" {
                continue;
            }
            assert!(detect_tokens(value).is_empty(), "{name} still has a token");
        }
    }
}

#[test]
fn a_long_title_cut_to_length_cannot_become_a_token() {
    // 40 characters after `npm_` is too long for a token, but a title cut at 200 characters can land on 36.
    let t = format!("{}npm_{}", "x ".repeat(80), sample(ALNUM, 40, 1));
    let r = run(Source::Notes, &format!("{t}\nbody\n"));
    assert_no_token_text(&r.items);
}

#[test]
fn a_notes_file_without_separators_is_one_note() {
    let r = run(Source::Notes, "only line\n");
    assert_eq!(r.items.len(), 1);
    assert_eq!(field(&r.items[0], "title"), Some("only line"));
    assert_eq!(field(&r.items[0], "body"), Some("only line"));
    assert!(run(Source::Notes, "").items.is_empty());
}

// ---- properties ----

fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("ghp_".to_owned()),
        Just("sk-ant-".to_owned()),
        Just("sk-".to_owned()),
        Just("AKIA".to_owned()),
        Just("npm_".to_owned()),
        Just("xoxb-".to_owned()),
        Just("glpat-".to_owned()),
        Just("AIza".to_owned()),
        Just("github_pat_".to_owned()),
        Just("\n".to_owned()),
        Just("\n---\n".to_owned()),
        Just(" ".to_owned()),
        Just("=".to_owned()),
        Just("-".to_owned()),
        Just("_".to_owned()),
        "[A-Za-z0-9]{1,60}",
        "[A-Z0-9]{1,30}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn nothing_that_looks_like_a_token_is_left_in_a_note(frags in proptest::collection::vec(fragment(), 0..40)) {
        let dump = frags.concat();
        let r = parse_import(Source::Notes, dump.as_bytes()).unwrap();
        assert_no_token_text(&r.items);
        assert_addable(&r.items);
    }

    #[test]
    fn arbitrary_bytes_never_panic_and_drafts_stay_addable(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        for s in [Source::Chrome, Source::Apple, Source::Bitwarden, Source::OnePassword, Source::Notes] {
            if let Ok(r) = parse_import(s, &bytes) {
                assert_addable(&r.items);
            }
        }
    }

    #[test]
    fn arbitrary_csv_shaped_text_stays_addable(rows in proptest::collection::vec(proptest::collection::vec("[ -~\n\r\"]{0,12}", 0..8), 0..12)) {
        let mut csv = String::from("name,url,username,password,note\n");
        for row in rows {
            csv.push_str(&row.iter().map(|f| format!("\"{}\"", f.replace('"', "\"\""))).collect::<Vec<_>>().join(","));
            csv.push('\n');
        }
        let r = parse_import(Source::Chrome, csv.as_bytes()).unwrap();
        assert_addable(&r.items);
    }

    #[test]
    fn no_detected_token_survives_in_a_note(
        parts in proptest::collection::vec("[a-z .:=\n]{0,20}", 1..6),
        seeds in proptest::collection::vec(0usize..50, 1..4),
    ) {
        let tokens: Vec<String> = seeds.iter().map(|s| gh(*s)).collect();
        let mut dump = String::new();
        let mut used: Vec<&String> = Vec::new();
        for (i, p) in parts.iter().enumerate() {
            dump.push_str(p);
            dump.push(' ');
            dump.push_str(&tokens[i % tokens.len()]);
            used.push(&tokens[i % tokens.len()]);
            dump.push(' ');
        }
        let r = parse_import(Source::Notes, dump.as_bytes()).unwrap();
        let filed: Vec<&str> = r.items.iter().filter(|d| d.item_type == ItemType::Token)
            .filter_map(|d| field(d, "token")).collect();
        for t in &used {
            assert!(filed.contains(&t.as_str()), "every token is filed");
        }
        for d in r.items.iter().filter(|d| d.item_type == ItemType::Note) {
            for (_, value) in &d.fields {
                for t in &used {
                    assert!(!value.contains(t.as_str()));
                }
            }
        }
        assert_eq!(r.tokens_detected, filed.len());
        assert_addable(&r.items);
    }
}

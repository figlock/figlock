//! Emergency Kit tests (FIG-31, `pairing-recovery.md` 5 and 9 item 2): content, QR round trip, the forced
//! verification step and the negative cases.
//!
//! The PDF is generated public output, so the tests parse it rather than trust the code that wrote it: the
//! text strings come out of the content stream, the QR code is rebuilt from the drawn rectangles and decoded.

use figlock_core::{
    Error, KIT_VERSION, KdfParams, KitInfo, SecretKey, Vault, render_kit, verify_kit_groups,
};

const INFO: KitInfo = KitInfo {
    vault_name: "Personal vault",
    storage: "Google Drive, sample@example.com",
    created: "2026-11-05",
};

fn key() -> SecretKey {
    SecretKey::from_bytes(*b"fig-kit-test-key")
}

fn kit_bytes(info: &KitInfo, sk: &SecretKey) -> Vec<u8> {
    render_kit(info, sk).unwrap().to_vec()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// The page content stream, cut out by its declared `/Length` the way a PDF reader does.
fn content_stream(pdf: &[u8]) -> String {
    let tag = find(pdf, b"<< /Length ").unwrap() + "<< /Length ".len();
    let len: usize = String::from_utf8_lossy(&pdf[tag..tag + 8])
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let start = find(pdf, b"stream\n").unwrap() + "stream\n".len();
    assert_eq!(
        &pdf[start + len..start + len + 10],
        b"endstream\n",
        "Length ends at endstream"
    );
    String::from_utf8(pdf[start..start + len].to_vec()).unwrap()
}

/// Every `( ... ) Tj` string of the content stream, with the PDF escapes undone.
fn texts(pdf: &[u8]) -> Vec<String> {
    let s = content_stream(pdf);
    let (mut out, mut cur, mut open, mut esc) = (Vec::new(), String::new(), false, false);
    for c in s.chars() {
        match (open, esc, c) {
            (false, _, '(') => open = true,
            (true, false, '\\') => esc = true,
            (true, false, ')') => {
                open = false;
                out.push(std::mem::take(&mut cur));
            }
            (true, _, c) => {
                cur.push(c);
                esc = false;
            }
            _ => {}
        }
    }
    out
}

/// Rebuild the QR module grid from the `re f` rectangles and decode it.
fn decode_qr(pdf: &[u8]) -> String {
    // Each rectangle is a run of one or more modules in a single row.
    let rects: Vec<[f32; 4]> = content_stream(pdf)
        .lines()
        .filter(|l| l.ends_with(" re f"))
        .map(|l| {
            let n: Vec<f32> = l
                .split_whitespace()
                .take(4)
                .map(|v| v.parse().unwrap())
                .collect();
            [n[0], n[1], n[2], n[3]]
        })
        .collect();
    let m = rects[0][3];
    assert!(rects.iter().all(|r| r[3] == m), "one row high");
    let min_x = rects.iter().map(|r| r[0]).fold(f32::MAX, f32::min);
    let max_x = rects.iter().map(|r| r[0] + r[2]).fold(f32::MIN, f32::max);
    let max_y = rects.iter().map(|r| r[1]).fold(f32::MIN, f32::max);
    let min_y = rects.iter().map(|r| r[1]).fold(f32::MAX, f32::min);
    // The finder patterns put a dark module on every edge of the symbol.
    let n = ((max_x - min_x) / m).round() as usize;
    assert_eq!(
        n,
        ((max_y - min_y) / m).round() as usize + 1,
        "square symbol"
    );
    let mut grid = vec![false; n * n];
    for [x, y, w, _] in &rects {
        let row = ((max_y - y) / m).round() as usize;
        let first = ((x - min_x) / m).round() as usize;
        for col in first..first + (w / m).round() as usize {
            grid[row * n + col] = true;
        }
    }
    // Scale up and add a quiet zone so the decoder can find the finders.
    let (scale, quiet) = (6usize, 4usize);
    let side = (n + 2 * quiet) * scale;
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(side, side, |x, y| {
        let (col, row) = (x / scale, y / scale);
        let dark = col >= quiet
            && row >= quiet
            && col < n + quiet
            && row < n + quiet
            && grid[(row - quiet) * n + (col - quiet)];
        if dark { 0 } else { 255 }
    });
    let grids = img.detect_grids();
    assert_eq!(grids.len(), 1, "exactly one QR code on the kit");
    grids[0].decode().expect("QR decodes").1
}

#[test]
fn kit_is_a_well_formed_pdf() {
    let pdf = kit_bytes(&INFO, &key());
    assert!(pdf.starts_with(b"%PDF-1.4\n"));
    assert!(pdf.ends_with(b"%%EOF\n"));

    // Every xref entry points at its object, and startxref at the table.
    let at = |needle: &[u8]| find(&pdf, needle).unwrap();
    let xref = at(b"xref\n");
    let startxref: usize = {
        let t = String::from_utf8_lossy(&pdf[at(b"startxref\n") + 10..]).to_string();
        t.lines().next().unwrap().parse().unwrap()
    };
    assert_eq!(startxref, xref);
    let table = String::from_utf8_lossy(&pdf[xref..]).to_string();
    let entries: Vec<&str> = table.lines().skip(3).take(7).collect();
    assert_eq!(entries.len(), 7);
    for (i, e) in entries.iter().enumerate() {
        let off: usize = e[..10].parse().unwrap();
        let head = format!("{} 0 obj", i + 1);
        assert_eq!(
            &pdf[off..off + head.len()],
            head.as_bytes(),
            "object {}",
            i + 1
        );
    }

    // The declared stream length is the real one (checked inside `content_stream`).
    assert!(!content_stream(&pdf).is_empty());
    assert!(table.contains("/Size 8"));
}

#[test]
fn kit_content_matches_the_spec() {
    let sk = key();
    let pdf = kit_bytes(&INFO, &sk);
    let t = texts(&pdf);
    let has = |s: &str| t.iter().any(|x| x == s);
    let has_part = |s: &str| t.iter().any(|x| x.contains(s));

    assert!(has(KIT_VERSION));
    assert_eq!(KIT_VERSION, "FL-KIT-1");
    assert!(has("FigLock Emergency Kit"));
    assert!(has(&sk.format()), "Secret Key in the display format");
    assert!(has(INFO.vault_name) && has(INFO.storage) && has(INFO.created));
    assert!(has("Master password (optional, write it elsewhere)"));
    assert!(has_part("Do not store your master password with this kit"));
    assert!(has_part("Install FigLock") && has_part("sign in") && has_part("master password"));
    assert!(has_part("Secret Key"));
    assert!(has_part(
        "cannot reset a lost master password or Secret Key"
    ));
    assert!(has_part(
        "Anyone who holds this kit AND knows your master password"
    ));
    assert!(has_part("no Recovery Key"));
}

#[test]
fn kit_holds_the_secret_key_and_no_other_secret() {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).unwrap();
    let pw: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let (_vault, created) = Vault::create(&pw, "Laptop", KdfParams::FLOOR, 0).unwrap();
    let pdf = kit_bytes(&INFO, &created.secret_key);

    assert!(
        texts(&pdf)
            .iter()
            .any(|x| *x == *created.secret_key.format())
    );
    assert!(find(&pdf, pw.as_bytes()).is_none(), "no master password");
    let raw = created.secret_key.expose();
    assert!(find(&pdf, raw).is_none(), "no raw key bytes");
    // Nothing from the vault file either: no wrapped key, no vault id.
    let vault_json = String::from_utf8(created.vault_json.clone()).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&vault_json).unwrap();
    for field in ["vault_id", "wrapped_vk", "salt"] {
        if let Some(v) = doc.get(field).and_then(|v| v.as_str()) {
            assert!(find(&pdf, v.as_bytes()).is_none(), "{field} not on the kit");
        }
    }
}

#[test]
fn qr_code_decodes_to_the_secret_key() {
    for bytes in [
        *b"fig-kit-test-key",
        [0u8; 16],
        [0xff; 16],
        *b"0123456789abcdef",
    ] {
        let sk = SecretKey::from_bytes(bytes);
        let pdf = kit_bytes(&INFO, &sk);
        let decoded = decode_qr(&pdf);
        assert_eq!(decoded, *sk.format());
        // And what the QR carries is accepted by Secret Key entry.
        assert_eq!(SecretKey::parse(&decoded).unwrap().expose(), &bytes);
    }
}

#[test]
fn rendering_is_deterministic_and_key_specific() {
    let a = kit_bytes(&INFO, &key());
    assert_eq!(a, kit_bytes(&INFO, &key()), "no clock, no randomness");

    let rotated = SecretKey::from_bytes(*b"fig-kit-rotated!");
    let b = kit_bytes(&INFO, &rotated);
    assert_ne!(a, b);
    assert!(
        find(&b, key().format().as_bytes()).is_none(),
        "a new kit carries only the new key"
    );
}

#[test]
fn verify_accepts_the_last_two_groups() {
    let sk = key();
    let f = sk.format().to_string();
    let groups: Vec<&str> = f.split('-').collect();
    assert_eq!(groups.len(), 9);
    let (g7, g8) = (groups[7], groups[8]);

    for typed in [
        format!("{g7}-{g8}"),
        format!("{g7}{g8}"),
        format!("{g7} {g8}"),
        format!("  {}-{}  ", g7.to_lowercase(), g8.to_lowercase()),
        format!("{g7}--{g8}"),
    ] {
        assert!(verify_kit_groups(&sk, &typed), "{typed:?}");
    }

    // Crockford confusables are read the way Secret Key entry reads them.
    let confused = format!("{g7}{g8}").replace('1', "l").replace('0', "o");
    assert!(verify_kit_groups(&sk, &confused));
}

#[test]
fn verify_rejects_everything_else() {
    let sk = key();
    let f = sk.format().to_string();
    let groups: Vec<&str> = f.split('-').collect();
    let (g6, g7, g8) = (groups[6], groups[7], groups[8]);

    assert!(!verify_kit_groups(&sk, ""));
    assert!(!verify_kit_groups(&sk, g8), "only one group");
    assert!(!verify_kit_groups(&sk, &format!("{g7}-{g7}")));
    assert!(
        !verify_kit_groups(&sk, &format!("{g8}-{g7}")),
        "wrong order"
    );
    assert!(
        !verify_kit_groups(&sk, &format!("{g6}-{g7}")),
        "earlier groups"
    );
    assert!(
        !verify_kit_groups(&sk, &format!("{g6}-{g7}-{g8}")),
        "too many groups"
    );
    assert!(
        !verify_kit_groups(&sk, &f),
        "the whole key is not the last two groups"
    );
    assert!(!verify_kit_groups(&sk, &format!("{g7}-{g8}X")));

    // One wrong character in each position of the eight.
    let good = format!("{g7}{g8}");
    for i in 0..good.len() {
        let mut bad: Vec<char> = good.chars().collect();
        bad[i] = if bad[i] == 'Z' { 'Y' } else { 'Z' };
        let bad: String = bad.into_iter().collect();
        assert!(!verify_kit_groups(&sk, &bad), "position {i}");
    }

    // A different key's groups do not verify.
    let other = SecretKey::from_bytes(*b"fig-kit-other-ky");
    assert!(!verify_kit_groups(&other, &format!("{g7}{g8}")));
}

#[test]
fn bad_inputs_are_refused_with_the_generic_error() {
    let sk = key();
    let long = "x".repeat(81);
    let bad: [KitInfo; 7] = [
        KitInfo {
            created: "2026-11-5",
            ..INFO
        },
        KitInfo {
            created: "2026/11/05",
            ..INFO
        },
        KitInfo {
            created: "2026-11-05\n",
            ..INFO
        },
        KitInfo {
            created: "",
            ..INFO
        },
        KitInfo {
            vault_name: "",
            ..INFO
        },
        KitInfo {
            vault_name: &long,
            ..INFO
        },
        KitInfo {
            storage: "",
            ..INFO
        },
    ];
    for info in bad {
        let Err(err) = render_kit(&info, &sk) else {
            panic!("bad input was accepted");
        };
        assert_eq!(err, Error);
        // The error says nothing about the key or the input.
        let shown = format!("{err} {err:?}");
        assert!(!shown.contains(&*sk.format()) && !shown.contains("2026"));
    }
    assert!(
        render_kit(
            &KitInfo {
                vault_name: &"x".repeat(80),
                ..INFO
            },
            &sk
        )
        .is_ok()
    );
}

#[test]
fn hostile_labels_cannot_break_the_pdf() {
    let sk = key();
    // Parentheses, a backslash, a stream terminator and non-ASCII text in the labels.
    let info = KitInfo {
        vault_name: "a) Tj ET endstream (b\\c",
        storage: "caf\u{e9} \u{1f512}\ttab\nline",
        created: "2026-11-05",
    };
    let pdf = kit_bytes(&info, &sk);
    let t = texts(&pdf);
    assert!(
        t.iter().any(|x| x == "a) Tj ET endstream (b\\c"),
        "round trips through the escapes"
    );
    assert!(
        t.iter().any(|x| x == "caf? ??tab?line"),
        "non-printable ASCII is replaced"
    );

    // The document keeps its shape: the stream ends where it says, the objects are all there, the QR decodes.
    assert!(pdf.ends_with(b"%%EOF\n"));
    assert_eq!(pdf.windows(7).filter(|w| *w == b" 0 obj\n").count(), 7);
    assert_eq!(decode_qr(&pdf), *sk.format());
}

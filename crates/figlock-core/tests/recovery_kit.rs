//! The printable Recovery Key (FIG-103): content of the code page and the cards, the QR round trip, and the
//! negative cases. The PDF is generated public output, so the tests parse it instead of trusting the writer.

use figlock_core::{
    KitInfo, RecoveryCards, RecoveryKey, render_recovery_kit, slip39::WORDS_FOR_256_BITS,
};

const INFO: KitInfo = KitInfo {
    vault_name: "Personal vault",
    storage: "Folder Documents",
    created: "2026-11-13",
};

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Every content stream, cut out by its declared `/Length` the way a PDF reader does.
fn streams(pdf: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = find(&pdf[from..], b"<< /Length ") {
        let tag = from + at + "<< /Length ".len();
        let len: usize = String::from_utf8_lossy(&pdf[tag..tag + 8])
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        let start = tag + find(&pdf[tag..], b"stream\n").unwrap() + "stream\n".len();
        assert_eq!(&pdf[start + len..start + len + 10], b"endstream\n");
        out.push(String::from_utf8(pdf[start..start + len].to_vec()).unwrap());
        from = start + len;
    }
    out
}

fn texts(stream: &str) -> Vec<String> {
    let (mut out, mut cur, mut open, mut esc) = (Vec::new(), String::new(), false, false);
    for c in stream.chars() {
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

fn decode_qr(stream: &str) -> String {
    let rects: Vec<[f32; 4]> = stream
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
    let min_x = rects.iter().map(|r| r[0]).fold(f32::MAX, f32::min);
    let max_x = rects.iter().map(|r| r[0] + r[2]).fold(f32::MIN, f32::max);
    let max_y = rects.iter().map(|r| r[1]).fold(f32::MIN, f32::max);
    let n = ((max_x - min_x) / m).round() as usize;
    let mut grid = vec![false; n * n];
    for [x, y, w, _] in &rects {
        let row = ((max_y - y) / m).round() as usize;
        let first = ((x - min_x) / m).round() as usize;
        for col in first..first + (w / m).round() as usize {
            grid[row * n + col] = true;
        }
    }
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
    assert_eq!(grids.len(), 1, "exactly one QR code");
    grids[0].decode().expect("QR decodes").1
}

fn well_formed(pdf: &[u8], pages: usize) {
    assert!(pdf.starts_with(b"%PDF-1.4\n") && pdf.ends_with(b"%%EOF\n"));
    // Every xref entry points at its object, and startxref at the table.
    let xref = find(pdf, b"xref\n").unwrap();
    let startxref = {
        let at = find(pdf, b"startxref\n").unwrap() + "startxref\n".len();
        String::from_utf8_lossy(&pdf[at..])
            .lines()
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap()
    };
    assert_eq!(startxref, xref);
    let table = String::from_utf8_lossy(&pdf[xref..]).into_owned();
    let entries: Vec<usize> = table
        .lines()
        .skip(3)
        .take_while(|l| l.ends_with(" n "))
        .map(|l| l[..10].parse().unwrap())
        .collect();
    assert_eq!(entries.len(), 5 + 2 * pages);
    for (i, off) in entries.iter().enumerate() {
        assert!(
            pdf[*off..].starts_with(format!("{} 0 obj\n", i + 1).as_bytes()),
            "object {}",
            i + 1
        );
    }
    assert!(find(pdf, format!("/Count {pages} >>").as_bytes()).is_some());
}

#[test]
fn the_code_page_holds_the_code_and_a_qr_that_decodes_to_it() {
    let key = RecoveryKey::random().unwrap();
    let code = key.format();
    let pdf = render_recovery_kit(&INFO, &RecoveryCards::Code(&code)).unwrap();
    well_formed(&pdf, 1);
    let s = streams(&pdf);
    assert_eq!(s.len(), 1);
    let t = texts(&s[0]);
    assert!(t.contains(&"FigLock Recovery Key".to_owned()));
    // The code is printed on two lines that together are the whole code.
    let groups: Vec<&String> = t
        .iter()
        .filter(|x| x.starts_with("FLR1-") || x.split('-').count() == 7)
        .collect();
    assert_eq!(groups.len(), 2, "{t:?}");
    assert_eq!(format!("{}-{}", groups[0], groups[1]), *code);
    assert_eq!(decode_qr(&s[0]), *code);
    assert_eq!(
        RecoveryKey::parse(&decode_qr(&s[0])).unwrap().public(),
        key.public()
    );
    for want in ["Personal vault", "Folder Documents", "2026-11-13"] {
        assert!(t.iter().any(|x| x == want), "{want}");
    }
    assert!(t.iter().any(|x| x.contains("forgot my password")));
    // Nothing else secret is on the page.
    assert!(!s[0].contains("FL1-"));
}

#[test]
fn each_share_gets_its_own_card_with_numbered_words() {
    let key = RecoveryKey::random().unwrap();
    let shares = key.split(2, 3).unwrap();
    let refs: Vec<&str> = shares.iter().map(|s| s.as_str()).collect();
    let pdf = render_recovery_kit(
        &INFO,
        &RecoveryCards::Shares {
            shares: &refs,
            threshold: 2,
        },
    )
    .unwrap();
    well_formed(&pdf, 3);
    let s = streams(&pdf);
    assert_eq!(s.len(), 3);
    for (i, page) in s.iter().enumerate() {
        let t = texts(page);
        assert!(t.contains(&format!("Recovery Card {} of 3", i + 1)));
        assert!(t.iter().any(|x| x.contains("Any 2 of the 3 cards")));
        // The numbered words, read back column by column, are the share.
        let mut numbered: Vec<(usize, String)> = t
            .iter()
            .filter_map(|x| {
                let (n, w) = x.trim().split_once(". ")?;
                // The numbered steps below the words have spaces in them; a word does not.
                (!w.contains(' ')).then_some(())?;
                Some((n.trim().parse().ok()?, w.to_owned()))
            })
            .collect();
        numbered.sort();
        assert_eq!(numbered.len(), WORDS_FOR_256_BITS);
        let words: Vec<String> = numbered.into_iter().map(|(_, w)| w).collect();
        assert_eq!(words.join(" "), *shares[i]);
    }
    // Two cards recover the key, as a person typing them from the pages would.
    assert_eq!(
        RecoveryKey::from_shares(&[&shares[0], &shares[2]])
            .unwrap()
            .public(),
        key.public()
    );
}

#[test]
fn bad_inputs_are_refused() {
    let key = RecoveryKey::random().unwrap();
    let shares = key.split(2, 3).unwrap();
    let refs: Vec<&str> = shares.iter().map(|s| s.as_str()).collect();
    let cards = |shares: &[&str], threshold| {
        render_recovery_kit(&INFO, &RecoveryCards::Shares { shares, threshold })
    };
    assert!(cards(&refs, 1).is_err());
    assert!(cards(&refs, 4).is_err());
    assert!(cards(&refs[..1], 2).is_err());
    assert!(cards(&["not a share!", "x y"], 2).is_err());
    let bad_date = KitInfo {
        created: "13/11/2026",
        ..INFO
    };
    assert!(render_recovery_kit(&bad_date, &RecoveryCards::Code("FLR1-AAAA")).is_err());
    let empty_name = KitInfo {
        vault_name: "",
        ..INFO
    };
    assert!(render_recovery_kit(&empty_name, &RecoveryCards::Code("FLR1-AAAA")).is_err());
}

#[test]
fn hostile_labels_cannot_break_the_pdf() {
    let key = RecoveryKey::random().unwrap();
    let info = KitInfo {
        vault_name: "a) Tj ET (b \\ c\u{202e}",
        storage: "storage",
        created: "2026-11-13",
    };
    let pdf = render_recovery_kit(&info, &RecoveryCards::Code(&key.format())).unwrap();
    well_formed(&pdf, 1);
    let t = texts(&streams(&pdf)[0]);
    assert!(t.iter().any(|x| x == "a) Tj ET (b \\ c?"));
}

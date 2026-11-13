//! SLIP-0039 against the official vectors of `trezor/python-shamir-mnemonic` (all 45, unchanged), the shares
//! of `slip39.json` made with that reference library, and round trips of the shares FigLock makes (ADR-0028).

use figlock_core::slip39::{self, MAX_SHARES};
use proptest::prelude::*;

fn vectors_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/vectors")
}

fn read(name: &str) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(vectors_dir().join(name)).unwrap()).unwrap()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Every official vector: the valid ones recover the stated master secret (all use the passphrase TREZOR), the
/// invalid ones are rejected.
#[test]
fn the_official_vectors_all_pass() {
    let all = read("slip39-official.json");
    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 45);
    let (mut valid, mut invalid) = (0, 0);
    for v in all {
        let title = v[0].as_str().unwrap();
        let mnemonics: Vec<&str> = v[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_str().unwrap())
            .collect();
        let want = v[2].as_str().unwrap();
        let got = slip39::combine(&mnemonics, b"TREZOR");
        if want.is_empty() {
            assert!(got.is_err(), "{title}: an invalid set was accepted");
            invalid += 1;
        } else {
            let got = got.unwrap_or_else(|_| panic!("{title}: a valid set was rejected"));
            assert_eq!(hex(&got), want, "{title}");
            valid += 1;
        }
    }
    assert_eq!((valid, invalid), (15, 30));
}

/// Shares made by the reference library: every combination of `M` recovers the secret, fewer do not, a
/// changed word fails the checksum.
#[test]
fn shares_from_the_reference_library_recover() {
    let doc = read("slip39.json");
    for case in doc["cases"].as_array().unwrap() {
        let m = case["inputs"]["threshold"].as_u64().unwrap() as usize;
        let n = case["inputs"]["share_count"].as_u64().unwrap() as usize;
        let want = unhex(case["expected"].as_str().unwrap());
        let shares: Vec<&str> = case["shares"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        assert_eq!(shares.len(), n);
        for mask in 0u32..(1 << n) {
            if mask.count_ones() as usize != m {
                continue;
            }
            let pick: Vec<&str> = (0..n)
                .filter(|i| mask >> i & 1 == 1)
                .map(|i| shares[i])
                .collect();
            assert_eq!(*slip39::combine(&pick, b"").unwrap(), want);
        }
        assert!(slip39::combine(&shares[..m - 1], b"").is_err());
        let mut words: Vec<&str> = shares[0].split(' ').collect();
        words[3] = if words[3] == "academic" {
            "acid"
        } else {
            "academic"
        };
        let broken = words.join(" ");
        let mut set = vec![broken.as_str()];
        set.extend(&shares[1..m]);
        assert!(slip39::combine(&set, b"").is_err());
    }
}

fn secret() -> [u8; 32] {
    let mut s = [0u8; 32];
    getrandom::fill(&mut s).unwrap();
    s
}

#[test]
fn every_threshold_of_every_count_recovers_and_fewer_do_not() {
    let s = secret();
    // The extremes and a middle: each split runs PBKDF2, which is slow in a debug build.
    for n in [2u8, 3, 5, MAX_SHARES as u8] {
        for m in [2, n.div_ceil(2) + 1, n]
            .into_iter()
            .filter(|m| (2..=n).contains(m))
        {
            let shares = slip39::split(&s, m, n).unwrap();
            assert_eq!(shares.len(), usize::from(n));
            let take = |from: usize, k: usize| -> Vec<&str> {
                (0..k)
                    .map(|i| shares[(from + i) % shares.len()].as_str())
                    .collect()
            };
            assert_eq!(*slip39::combine(&take(0, m as usize), b"").unwrap(), s);
            assert_eq!(
                *slip39::combine(&take(usize::from(n) - 1, m as usize), b"").unwrap(),
                s
            );
            assert!(slip39::combine(&take(0, m as usize - 1), b"").is_err());
        }
    }
}

#[test]
fn a_repeated_share_counts_once_and_extra_shares_are_refused() {
    let s = secret();
    let shares = slip39::split(&s, 2, 3).unwrap();
    assert!(slip39::combine(&[&shares[0], &shares[0]], b"").is_err());
    assert_eq!(
        *slip39::combine(&[&shares[0], &shares[1], &shares[1]], b"").unwrap(),
        s
    );
    // The reference library takes exactly the threshold; so does this one.
    assert!(slip39::combine(&[&shares[0], &shares[1], &shares[2]], b"").is_err());
}

#[test]
fn words_are_case_insensitive_and_whitespace_is_free() {
    let s = secret();
    let shares = slip39::split(&s, 2, 2).unwrap();
    let loud = shares[0].to_uppercase();
    let messy = format!("  {}\n", shares[1].replace(' ', "   "));
    assert_eq!(*slip39::combine(&[&loud, &messy], b"").unwrap(), s);
}

#[test]
fn a_passphrase_changes_the_secret_and_a_wrong_one_does_not_fail() {
    let s = secret();
    let groups = slip39::generate(1, &[(2, 3)], &s, b"hunter2").unwrap();
    let pick = [groups[0][0].as_str(), groups[0][2].as_str()];
    assert_eq!(*slip39::combine(&pick, b"hunter2").unwrap(), s);
    // SLIP-0039 has no way to tell a wrong passphrase: it gives another valid secret (plausible deniability).
    let other = slip39::combine(&pick, b"hunter3").unwrap();
    assert_ne!(*other, s);
    assert!(slip39::combine(&pick, "naïve".as_bytes()).is_err());
}

#[test]
fn groups_need_enough_groups_and_enough_members() {
    let s = secret();
    // Two of three groups, each group 2 of 3.
    let groups = slip39::generate(2, &[(2, 3), (2, 3), (1, 1)], &s, b"").unwrap();
    let g = |a: usize, b: usize| groups[a][b].as_str();
    assert_eq!(
        *slip39::combine(&[g(0, 0), g(0, 1), g(1, 0), g(1, 2)], b"").unwrap(),
        s
    );
    assert_eq!(
        *slip39::combine(&[g(2, 0), g(0, 1), g(0, 2)], b"").unwrap(),
        s
    );
    assert!(slip39::combine(&[g(0, 0), g(0, 1)], b"").is_err());
    assert!(slip39::combine(&[g(0, 0), g(1, 0)], b"").is_err());
}

#[test]
fn bad_parameters_are_refused() {
    let s = secret();
    assert!(slip39::split(&s, 1, 3).is_err());
    assert!(slip39::split(&s, 4, 3).is_err());
    assert!(slip39::split(&s, 2, 17).is_err());
    assert!(slip39::split(&s, 2, 1).is_err());
    assert!(slip39::split(&s[..14], 2, 3).is_err());
    assert!(slip39::split(&s[..17], 2, 3).is_err());
    assert!(slip39::combine(&[], b"").is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn any_threshold_subset_recovers(
        secret in proptest::collection::vec(any::<u8>(), 8..=16).prop_map(|v| v.repeat(2)),
        count in 2u8..=8,
        pick in any::<u64>(),
    ) {
        let threshold = 2 + (pick % u64::from(count - 1)) as u8;
        let shares = slip39::split(&secret, threshold, count).unwrap();
        let start = (pick >> 8) as usize % shares.len();
        let chosen: Vec<&str> = (0..usize::from(threshold))
            .map(|i| shares[(start + i * 3) % shares.len()].as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        // Skip draws where the stride landed twice on the same card.
        prop_assume!(chosen.len() == usize::from(threshold));
        prop_assert_eq!(&**slip39::combine(&chosen, b"").unwrap(), &secret[..]);
    }

    #[test]
    fn a_single_changed_word_never_recovers_the_secret(
        word in 0usize..33,
        replacement in 0usize..1024,
    ) {
        let s = secret();
        let shares = slip39::split(&s, 2, 3).unwrap();
        let list = include_str!("../src/slip39_words.txt").lines().collect::<Vec<_>>();
        let mut words: Vec<&str> = shares[0].split(' ').collect();
        prop_assume!(words[word] != list[replacement]);
        words[word] = list[replacement];
        let changed = words.join(" ");
        // The checksum catches any change of up to three words.
        prop_assert!(slip39::combine(&[&changed, &shares[1]], b"").is_err());
    }
}

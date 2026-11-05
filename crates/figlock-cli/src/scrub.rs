//! Output scrubbing for `figlock run`: replace the secret values handed to a program wherever they show up
//! in what the program prints. It stops accidental leaks (a debug line, an error that echoes a header); it
//! cannot stop a program that deliberately re-encodes a secret (`docs/adr/0003`).

use zeroize::Zeroizing;

/// Secrets shorter than this are not scrubbed: they would shred ordinary output and are guessable anyway.
pub const MIN_LEN: usize = 6;

struct Pattern {
    needle: Zeroizing<Vec<u8>>,
    mark: Vec<u8>,
}

/// A streaming filter. Feed it chunks in order; it holds back only the few trailing bytes that could be the
/// start of a secret, so a value split across two reads is still caught.
pub struct Scrubber {
    patterns: Vec<Pattern>,
    keep: usize,
    pending: Vec<u8>,
}

fn base64(bytes: &[u8], url_safe: bool) -> Vec<u8> {
    let table: &[u8; 64] = if url_safe {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
    } else {
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
    };
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        out.push(table[(n >> 18) as usize & 63]);
        out.push(table[(n >> 12) as usize & 63]);
        if c.len() > 1 {
            out.push(table[(n >> 6) as usize & 63]);
        }
        if c.len() > 2 {
            out.push(table[n as usize & 63]);
        }
    }
    out
}

fn percent(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &b in bytes {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b);
        } else {
            out.extend_from_slice(format!("%{b:02X}").as_bytes());
        }
    }
    out
}

impl Scrubber {
    /// `secrets` are `(variable name, value)`. Each value is matched as is, in standard and URL-safe base64
    /// (unpadded, so padding does not matter) and percent-encoded. Values under [`MIN_LEN`] are skipped.
    pub fn new(secrets: &[(String, Zeroizing<String>)]) -> Scrubber {
        let mut patterns: Vec<Pattern> = Vec::new();
        for (name, value) in secrets {
            if value.len() < MIN_LEN {
                continue;
            }
            let raw = value.as_bytes();
            let mark = format!("[REDACTED:{name}]").into_bytes();
            for form in [
                raw.to_vec(),
                base64(raw, false),
                base64(raw, true),
                percent(raw),
            ] {
                if !patterns.iter().any(|p| *p.needle == form) {
                    patterns.push(Pattern {
                        needle: Zeroizing::new(form),
                        mark: mark.clone(),
                    });
                }
            }
        }
        // Longest first, so a secret that contains another is replaced whole.
        patterns.sort_by_key(|p| std::cmp::Reverse(p.needle.len()));
        let keep = patterns.first().map_or(0, |p| p.needle.len() - 1);
        Scrubber {
            patterns,
            keep,
            pending: Vec::new(),
        }
    }

    fn matching(&self, at: &[u8]) -> Option<&Pattern> {
        self.patterns.iter().find(|p| at.starts_with(&p.needle))
    }

    /// Whether `at` (shorter than some pattern) is the beginning of a pattern, so more input could complete it.
    fn could_grow(&self, at: &[u8]) -> bool {
        self.patterns
            .iter()
            .any(|p| p.needle.len() > at.len() && p.needle.starts_with(at))
    }

    fn drain(&mut self, last: bool) -> Vec<u8> {
        let buf = std::mem::take(&mut self.pending);
        let mut out = Vec::with_capacity(buf.len());
        let mut i = 0;
        while i < buf.len() {
            let rest = &buf[i..];
            if let Some(p) = self.matching(rest) {
                out.extend_from_slice(&p.mark);
                i += p.needle.len();
            } else if !last && rest.len() <= self.keep && self.could_grow(rest) {
                break;
            } else {
                out.push(buf[i]);
                i += 1;
            }
        }
        self.pending = buf[i..].to_vec();
        out
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<u8> {
        if self.patterns.is_empty() {
            return chunk.to_vec();
        }
        self.pending.extend_from_slice(chunk);
        self.drain(false)
    }

    /// Flush what was held back. Call once, when the stream ends.
    pub fn finish(&mut self) -> Vec<u8> {
        self.drain(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret(name: &str, v: &str) -> (String, Zeroizing<String>) {
        (name.to_owned(), Zeroizing::new(v.to_owned()))
    }

    fn run(s: &mut Scrubber, chunks: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for c in chunks {
            out.extend(s.feed(c));
        }
        out.extend(s.finish());
        out
    }

    #[test]
    fn a_value_in_the_output_is_replaced_by_its_name() {
        let mut s = Scrubber::new(&[secret("API_KEY", "tok_8f3a91c2d7")]);
        let out = run(&mut s, &[b"curl -H 'Authorization: tok_8f3a91c2d7' done\n"]);
        assert_eq!(out, b"curl -H 'Authorization: [REDACTED:API_KEY]' done\n");
    }

    #[test]
    fn a_value_split_at_every_possible_point_is_still_caught() {
        let line = b"before tok_8f3a91c2d7 after tok_8f3a91c2d7";
        for cut in 0..line.len() {
            let mut s = Scrubber::new(&[secret("K", "tok_8f3a91c2d7")]);
            let out = run(&mut s, &[&line[..cut], &line[cut..]]);
            assert_eq!(out, b"before [REDACTED:K] after [REDACTED:K]", "cut {cut}");
        }
        // One byte at a time, the worst case.
        let mut s = Scrubber::new(&[secret("K", "tok_8f3a91c2d7")]);
        let chunks: Vec<&[u8]> = line.chunks(1).collect();
        assert_eq!(
            run(&mut s, &chunks),
            b"before [REDACTED:K] after [REDACTED:K]"
        );
    }

    #[test]
    fn base64_and_percent_encoded_forms_are_caught() {
        let value = "p@ss/w0rd+Z?9";
        let mut s = Scrubber::new(&[secret("PW", value)]);
        let std = String::from_utf8(base64(value.as_bytes(), false)).unwrap();
        let url = String::from_utf8(base64(value.as_bytes(), true)).unwrap();
        let pct = "p%40ss%2Fw0rd%2BZ%3F9";
        let text = format!("a={std} b={url} c={pct} d={value}");
        let out = String::from_utf8(run(&mut s, &[text.as_bytes()])).unwrap();
        assert_eq!(
            out,
            "a=[REDACTED:PW] b=[REDACTED:PW] c=[REDACTED:PW] d=[REDACTED:PW]"
        );
        assert!(!out.contains("w0rd"));
    }

    #[test]
    fn known_base64_vector() {
        assert_eq!(base64(b"Man", false), b"TWFu");
        assert_eq!(base64(b"Ma", false), b"TWE");
        assert_eq!(base64(b"M", false), b"TQ");
        assert_eq!(base64(&[0xfb, 0xff], false), b"+/8");
        assert_eq!(base64(&[0xfb, 0xff], true), b"-_8");
    }

    #[test]
    fn the_longer_secret_wins_when_one_contains_another() {
        let mut s = Scrubber::new(&[secret("SHORT", "abcdef"), secret("LONG", "abcdefghij")]);
        assert_eq!(run(&mut s, &[b"x abcdefghij y"]), b"x [REDACTED:LONG] y");
    }

    #[test]
    fn short_values_are_left_alone_and_other_output_is_untouched() {
        let mut s = Scrubber::new(&[secret("PIN", "1234"), secret("K", "tok_8f3a91c2d7")]);
        let out = run(&mut s, &[b"pin 1234 and tok_8f3a91c2 (cut short)\n"]);
        assert_eq!(out, b"pin 1234 and tok_8f3a91c2 (cut short)\n");
    }

    #[test]
    fn held_back_bytes_are_flushed_at_the_end_and_binary_output_passes() {
        let mut s = Scrubber::new(&[secret("K", "tok_8f3a91c2d7")]);
        assert_eq!(run(&mut s, &[b"ends with tok_8f3a"]), b"ends with tok_8f3a");
        let bin = [0u8, 255, 1, 2, 128];
        let mut s = Scrubber::new(&[secret("K", "tok_8f3a91c2d7")]);
        assert_eq!(run(&mut s, &[&bin]), bin);
        // No secrets: passes straight through.
        let mut s = Scrubber::new(&[]);
        assert_eq!(s.feed(b"abc"), b"abc");
    }
}

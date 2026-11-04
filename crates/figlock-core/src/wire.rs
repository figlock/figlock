//! Byte framing from `crypto.md` 3: big-endian integers, length prefixes and the AAD builder.

pub fn u16be(n: u16) -> [u8; 2] {
    n.to_be_bytes()
}

pub fn u32be(n: u32) -> [u8; 4] {
    n.to_be_bytes()
}

pub fn u64be(n: u64) -> [u8; 8] {
    n.to_be_bytes()
}

/// `aad(ctx, parts...) = u16(len(ctx)) || ctx || lp(part1) || lp(part2) ...` with `lp(x) = u32(len(x)) || x`.
/// Panics only if `ctx` or a part exceeds the framing limits, which is a programming error.
pub fn aad(ctx: &str, parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&u16be(u16::try_from(ctx.len()).expect("ctx fits u16")));
    out.extend_from_slice(ctx.as_bytes());
    for p in parts {
        out.extend_from_slice(&u32be(u32::try_from(p.len()).expect("part fits u32")));
        out.extend_from_slice(p);
    }
    out
}

/// The 8-byte hybrid logical clock stamp: `u48(wall_ms) || u16(counter)` (`sync.md` 5).
pub fn hlc_bytes(wall_ms: u64, counter: u16) -> [u8; 8] {
    let w = wall_ms.to_be_bytes();
    let c = counter.to_be_bytes();
    [w[2], w[3], w[4], w[5], w[6], w[7], c[0], c[1]]
}

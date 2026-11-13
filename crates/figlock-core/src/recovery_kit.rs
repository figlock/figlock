//! The printable Recovery Key: one page with the single `FLR1-` code and its QR, or one card per SLIP-0039 share
//! (`pairing-recovery.md` 7.2, FIG-103). Pure like the Emergency Kit: it returns bytes and the caller decides
//! where they go; nothing here writes a file, reads the clock or logs. A page holds the key (or one share) and
//! nothing else secret: no master password, no Secret Key.

use qrcode::{Color, EcLevel, QrCode};
use zeroize::Zeroizing;

use crate::kit::{
    KitInfo, MARGIN, PAGE_H, PAGE_W, Page, QR_MODULE, assemble_pages, label, valid_date,
};
use crate::{Error, Result};

/// Version marker printed on the page.
pub const RECOVERY_KIT_VERSION: &str = "FL-RK-1";

/// What to print.
#[derive(Debug, Clone, Copy)]
pub enum RecoveryCards<'a> {
    /// The single `FLR1-` code.
    Code(&'a str),
    /// SLIP-0039 shares, one card each, of which `threshold` open the vault.
    Shares {
        shares: &'a [&'a str],
        threshold: u8,
    },
}

const CODE_STEPS: [&str; 4] = [
    "1. Install FigLock on any device and choose \"I forgot my password\".",
    "2. Choose the same storage as before and sign in to it.",
    "3. Type this key, or scan the QR code.",
    "4. Choose a new master password. FigLock makes a new Secret Key and a new Emergency Kit.",
];

const CODE_NOTICES: [&str; 4] = [
    "Anyone who holds this key can open your vault without your master password.",
    "Keep it apart from your Emergency Kit and from your master password.",
    "FigLock cannot recover this key. If you make a new one, this one stops working.",
    "Using it is a good moment to make a new Recovery Key and revoke devices you do not trust.",
];

fn card_steps(threshold: u8) -> [String; 4] {
    [
        "1. Install FigLock on any device and choose \"I forgot my password\".".to_owned(),
        "2. Choose the same storage as before and sign in to it.".to_owned(),
        format!("3. Type the words of {threshold} different cards, in any order."),
        "4. Choose a new master password. FigLock makes a new Secret Key and a new Emergency Kit."
            .to_owned(),
    ]
}

fn card_notices(threshold: u8, count: usize) -> [String; 4] {
    [
        format!(
            "Anyone who gathers {threshold} of the {count} cards can open your vault without your master password."
        ),
        "Keep each card in a different place, or with a different person you trust.".to_owned(),
        format!(
            "Fewer than {threshold} cards reveal nothing. Losing more than {} cards loses this recovery, not the vault.",
            count - usize::from(threshold) + 1
        ),
        "If you make a new Recovery Key, every card of this set stops working.".to_owned(),
    ]
}

fn belongs(p: &mut Page, y: &mut f32, name: &str, storage: &str, created: &str) {
    p.text("F2", 12.0, MARGIN, *y, "This belongs to");
    *y -= 18.0;
    for (k, v) in [("Vault", name), ("Storage", storage), ("Created", created)] {
        p.text("F2", 10.0, MARGIN, *y, k);
        p.text("F1", 10.0, MARGIN + 60.0, *y, v);
        *y -= 15.0;
    }
    *y -= 12.0;
}

fn lines(p: &mut Page, y: &mut f32, heading: &str, items: &[&str]) {
    p.text("F2", 12.0, MARGIN, *y, heading);
    *y -= 18.0;
    for s in items {
        p.text("F1", 10.0, MARGIN, *y, s);
        *y -= 15.0;
    }
    *y -= 14.0;
}

fn code_page(info: &KitInfo, name: &str, storage: &str, code: &str) -> Result<Zeroizing<String>> {
    let qr = QrCode::with_error_correction_level(code.as_bytes(), EcLevel::M).map_err(|_| Error)?;
    let mut p = Page {
        ops: Zeroizing::new(String::with_capacity(8 * 1024)),
    };
    let mut y = PAGE_H - MARGIN - 18.0;
    p.text("F2", 24.0, MARGIN, y, "FigLock Recovery Key");
    p.text(
        "F1",
        10.0,
        PAGE_W - MARGIN - 50.0,
        y + 2.0,
        RECOVERY_KIT_VERSION,
    );
    y -= 38.0;
    p.text("F2", 12.0, MARGIN, y, "Your Recovery Key");
    y -= 26.0;
    // Two lines of seven groups each; the first starts with the prefix.
    let groups: Vec<&str> = code.split('-').collect();
    let (head, tail) = groups.split_at(groups.len().div_ceil(2));
    p.text("F3", 14.0, MARGIN, y, &head.join("-"));
    y -= 20.0;
    p.text("F3", 14.0, MARGIN, y, &tail.join("-"));
    y -= 18.0;

    let width = qr.width();
    let side = width as f32 * QR_MODULE;
    let top = y - 4.0 * QR_MODULE;
    let mut colors = qr.to_colors();
    for (row, modules) in colors.chunks(width).enumerate() {
        let mut col = 0;
        while col < width {
            if modules[col] != Color::Dark {
                col += 1;
                continue;
            }
            let start = col;
            while col < width && modules[col] == Color::Dark {
                col += 1;
            }
            p.rect(
                MARGIN + 4.0 * QR_MODULE + start as f32 * QR_MODULE,
                top - (row + 1) as f32 * QR_MODULE,
                (col - start) as f32 * QR_MODULE,
                QR_MODULE,
            );
        }
    }
    colors.fill(Color::Light);
    y = top - side - 4.0 * QR_MODULE - 18.0;

    belongs(&mut p, &mut y, name, storage, info.created);
    lines(
        &mut p,
        &mut y,
        "If you forget your master password",
        &CODE_STEPS,
    );
    lines(&mut p, &mut y, "Read this", &CODE_NOTICES);
    if y < MARGIN - 14.0 {
        return Err(Error);
    }
    Ok(Zeroizing::new(std::mem::take(&mut *p.ops)))
}

fn card_page(
    info: &KitInfo,
    (name, storage): (&str, &str),
    (number, count, threshold): (usize, usize, u8),
    share: &str,
) -> Result<Zeroizing<String>> {
    let words: Vec<&str> = share.split(' ').collect();
    if words.is_empty()
        || words
            .iter()
            .any(|w| w.is_empty() || !w.bytes().all(|b| b.is_ascii_lowercase()))
    {
        return Err(Error);
    }
    let mut p = Page {
        ops: Zeroizing::new(String::with_capacity(8 * 1024)),
    };
    let mut y = PAGE_H - MARGIN - 18.0;
    p.text(
        "F2",
        24.0,
        MARGIN,
        y,
        &format!("Recovery Card {number} of {count}"),
    );
    p.text(
        "F1",
        10.0,
        PAGE_W - MARGIN - 50.0,
        y + 2.0,
        RECOVERY_KIT_VERSION,
    );
    y -= 26.0;
    p.text(
        "F1",
        11.0,
        MARGIN,
        y,
        &format!(
            "Any {threshold} of the {count} cards open the vault without the master password."
        ),
    );
    y -= 30.0;

    // The words in three columns, numbered down each column.
    let rows = words.len().div_ceil(3);
    for (i, w) in words.iter().enumerate() {
        let (col, row) = (i / rows, i % rows);
        p.text(
            "F3",
            12.0,
            MARGIN + col as f32 * 165.0,
            y - row as f32 * 20.0,
            &format!("{:>2}. {w}", i + 1),
        );
    }
    y -= rows as f32 * 20.0 + 18.0;

    belongs(&mut p, &mut y, name, storage, info.created);
    lines(
        &mut p,
        &mut y,
        "If you forget your master password",
        &card_steps(threshold)
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    lines(
        &mut p,
        &mut y,
        "Read this",
        &card_notices(threshold, count)
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    if y < MARGIN - 14.0 {
        return Err(Error);
    }
    Ok(Zeroizing::new(std::mem::take(&mut *p.ops)))
}

/// Render the Recovery Key as a PDF: one page for a code, one page per card for shares.
pub fn render_recovery_kit(info: &KitInfo, cards: &RecoveryCards) -> Result<Zeroizing<Vec<u8>>> {
    let name = label(info.vault_name)?;
    let storage = label(info.storage)?;
    if !valid_date(info.created) {
        return Err(Error);
    }
    let pages: Vec<Zeroizing<String>> = match cards {
        RecoveryCards::Code(code) => vec![code_page(info, &name, &storage, code)?],
        RecoveryCards::Shares { shares, threshold } => {
            if shares.len() < 2
                || shares.len() > crate::slip39::MAX_SHARES
                || *threshold < 2
                || usize::from(*threshold) > shares.len()
            {
                return Err(Error);
            }
            shares
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    card_page(
                        info,
                        (&name, &storage),
                        (i + 1, shares.len(), *threshold),
                        s,
                    )
                })
                .collect::<Result<_>>()?
        }
    };
    let refs: Vec<&str> = pages.iter().map(|p| p.as_str()).collect();
    Ok(assemble_pages(&refs))
}

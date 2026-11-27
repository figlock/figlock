//! The printable Recovery Key: one page with the single `FLR1-` code and its QR (`pairing-recovery.md` 7, FIG-103).
//! Pure like the Emergency Kit: it returns bytes and the caller decides where they go; nothing here writes a file,
//! reads the clock or logs. The page holds the key and nothing else secret: no password, no Secret Key.

use qrcode::{Color, EcLevel, QrCode};
use zeroize::Zeroizing;

use crate::kit::{
    KitInfo, MARGIN, PAGE_H, PAGE_W, Page, QR_MODULE, assemble_pages, label, valid_date,
};
use crate::{Error, Result};

/// Version marker printed on the page.
pub const RECOVERY_KIT_VERSION: &str = "FL-RK-1";

const CODE_STEPS: [&str; 4] = [
    "1. Install FigLock on any device and choose \"I forgot my password\".",
    "2. Choose the same storage as before and sign in to it.",
    "3. Type this key, or scan the QR code.",
    "4. Choose a new password. FigLock makes a new Secret Key and a new Emergency Kit.",
];

const CODE_NOTICES: [&str; 4] = [
    "Anyone who holds this key can open your vault without your password.",
    "Keep it apart from your Emergency Kit and from your password.",
    "FigLock cannot recover this key. If you make a new one, this one stops working.",
    "Using it is a good moment to make a new Recovery Key and revoke devices you do not trust.",
];

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
    lines(&mut p, &mut y, "If you forget your password", &CODE_STEPS);
    lines(&mut p, &mut y, "Read this", &CODE_NOTICES);
    if y < MARGIN - 14.0 {
        return Err(Error);
    }
    Ok(Zeroizing::new(std::mem::take(&mut *p.ops)))
}

/// Render the Recovery Key as a one page PDF holding the code and its QR.
pub fn render_recovery_kit(info: &KitInfo, code: &str) -> Result<Zeroizing<Vec<u8>>> {
    let name = label(info.vault_name)?;
    let storage = label(info.storage)?;
    if !valid_date(info.created) {
        return Err(Error);
    }
    let page = code_page(info, &name, &storage, code)?;
    Ok(assemble_pages(&[page.as_str()]))
}

//! The Emergency Kit PDF and its forced verification step (`pairing-recovery.md` 5).
//!
//! Pure: [`render_kit`] returns the bytes and the caller decides where they go. Nothing here writes a file,
//! reads the clock or logs. The kit holds the Secret Key and nothing else secret: no master password, no
//! `VK`, no other key.
//!
//! The PDF is written by hand (one page, the built-in Helvetica and Courier fonts, no compression) so the
//! content stays inspectable and the only added dependency is the QR encoder. The built-in fonts cover
//! printable ASCII; any other character in the vault name or storage label is printed as `?`.

use qrcode::{Color, EcLevel, QrCode};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::secret_key::normalise;
use crate::{Error, Result, SecretKey};

/// Version marker printed on the kit.
pub const KIT_VERSION: &str = "FL-KIT-1";

const MAX_LABEL_CHARS: usize = 80;

const PAGE_W: f32 = 595.0;
const PAGE_H: f32 = 842.0;
const MARGIN: f32 = 56.0;
const QR_MODULE: f32 = 4.0;

/// What the kit says about the vault it belongs to, so the user knows what it opens.
#[derive(Debug, Clone, Copy)]
pub struct KitInfo<'a> {
    /// The vault name shown to the user.
    pub vault_name: &'a str,
    /// The storage in use, for example "Google Drive, name@example.com".
    pub storage: &'a str,
    /// Creation date as `YYYY-MM-DD`. The caller supplies it; this crate never reads the clock.
    pub created: &'a str,
}

fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

/// Printable ASCII only, with the PDF string delimiters escaped.
fn pdf_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

fn label(s: &str) -> Result<String> {
    if s.is_empty() || s.chars().count() > MAX_LABEL_CHARS {
        return Err(Error);
    }
    Ok(s.to_owned())
}

struct Page {
    ops: String,
}

impl Page {
    fn text(&mut self, font: &str, size: f32, x: f32, y: f32, text: &str) {
        self.ops.push_str(&format!(
            "BT /{font} {size:.1} Tf {x:.2} {y:.2} Td ({}) Tj ET\n",
            pdf_text(text)
        ));
    }

    fn rule(&mut self, x1: f32, x2: f32, y: f32) {
        self.ops
            .push_str(&format!("0.5 w {x1:.2} {y:.2} m {x2:.2} {y:.2} l S\n"));
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.ops
            .push_str(&format!("{x:.2} {y:.2} {w:.2} {h:.2} re f\n"));
    }
}

const STEPS: [&str; 5] = [
    "1. Install FigLock on the new device.",
    "2. Choose the same storage as before and sign in to it.",
    "3. Type your master password.",
    "4. Type the Secret Key above, or scan the QR code.",
    "5. Optionally revoke the lost device from the device list.",
];

const NOTICES: [&str; 5] = [
    "Anyone who holds this kit AND knows your master password can open your vault.",
    "Keep the kit somewhere safe and apart from your master password.",
    "FigLock has no server and cannot reset a lost master password or Secret Key.",
    "If you lose the password and have no Recovery Key set up, your data cannot be recovered.",
    "If this kit is lost or photographed, rotate your Secret Key and print a new kit.",
];

/// Render the Emergency Kit as a one-page PDF.
///
/// The bytes contain the Secret Key, so they are returned in a zeroizing buffer. The caller writes them only
/// to the path the user chose and never uploads, logs or copies them on its own (`pairing-recovery.md` 5).
pub fn render_kit(info: &KitInfo, secret_key: &SecretKey) -> Result<Zeroizing<Vec<u8>>> {
    let name = label(info.vault_name)?;
    let storage = label(info.storage)?;
    if !valid_date(info.created) {
        return Err(Error);
    }

    let formatted = secret_key.format();
    let qr =
        QrCode::with_error_correction_level(formatted.as_bytes(), EcLevel::M).map_err(|_| Error)?;

    let mut p = Page { ops: String::new() };
    let mut y = PAGE_H - MARGIN - 18.0;

    p.text("F2", 24.0, MARGIN, y, "FigLock Emergency Kit");
    p.text("F1", 10.0, PAGE_W - MARGIN - 50.0, y + 2.0, KIT_VERSION);
    y -= 38.0;

    p.text("F2", 12.0, MARGIN, y, "Your Secret Key");
    y -= 26.0;
    p.text("F3", 15.0, MARGIN, y, &formatted);
    y -= 18.0;

    // QR code of the same string, with a four-module quiet zone.
    let width = qr.width();
    let side = width as f32 * QR_MODULE;
    let top = y - 4.0 * QR_MODULE;
    // One rectangle per horizontal run of dark modules, so no hairline seams show between neighbours.
    let colors = qr.to_colors();
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
    y = top - side - 4.0 * QR_MODULE - 18.0;

    p.text("F2", 12.0, MARGIN, y, "This kit belongs to");
    y -= 18.0;
    for (k, v) in [
        ("Vault", name.as_str()),
        ("Storage", storage.as_str()),
        ("Created", info.created),
    ] {
        p.text("F2", 10.0, MARGIN, y, k);
        p.text("F1", 10.0, MARGIN + 60.0, y, v);
        y -= 15.0;
    }
    y -= 12.0;

    p.text("F2", 12.0, MARGIN, y, "If you lose all your devices");
    y -= 18.0;
    for s in STEPS {
        p.text("F1", 10.0, MARGIN, y, s);
        y -= 15.0;
    }
    y -= 14.0;

    p.text(
        "F2",
        10.0,
        MARGIN,
        y,
        "Master password (optional, write it elsewhere)",
    );
    y -= 24.0;
    p.rule(MARGIN, PAGE_W - MARGIN, y);
    y -= 14.0;
    p.text(
        "F1",
        9.0,
        MARGIN,
        y,
        "Do not store your master password with this kit.",
    );
    y -= 30.0;

    p.text("F2", 12.0, MARGIN, y, "Read this");
    y -= 18.0;
    for s in NOTICES {
        p.text("F1", 10.0, MARGIN, y, s);
        y -= 15.0;
    }
    if y < MARGIN {
        return Err(Error);
    }

    Ok(Zeroizing::new(assemble(&p.ops)))
}

/// Wrap a content stream in a minimal PDF 1.4 document.
fn assemble(content: &str) -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W:.0} {PAGE_H:.0}] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R /F2 6 0 R /F3 7 0 R >> >> >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier-Bold /Encoding /WinAnsiEncoding >>"
            .to_string(),
    ];

    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// The forced verification step: does `typed` match the last two groups of the Secret Key?
///
/// Spaces and dashes are ignored, case does not matter and the Crockford confusables are accepted, like
/// the rest of Secret Key entry. Pasting is allowed (`pairing-recovery.md` 5).
pub fn verify_kit_groups(secret_key: &SecretKey, typed: &str) -> bool {
    let formatted = secret_key.format();
    let body: Zeroizing<String> =
        Zeroizing::new(formatted.chars().filter(|c| *c != '-').skip(3).collect());
    let expected = &body.as_bytes()[24..];
    let typed = Zeroizing::new(normalise(typed));
    bool::from(typed.as_bytes().ct_eq(expected))
}

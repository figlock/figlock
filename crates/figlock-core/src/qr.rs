//! QR codes for text the window shows, as rows of modules. The window draws them itself, so no markup comes
//! from here.

use qrcode::{Color, EcLevel, QrCode};

use crate::{Error, Result};

/// The modules of a QR code for `text`, one string per row of `'1'` (dark) and `'0'` (light), without the
/// quiet zone. Error correction level M, like the Emergency Kit.
pub fn qr_rows(text: &str) -> Result<Vec<String>> {
    let qr = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).map_err(|_| Error)?;
    let width = qr.width();
    Ok(qr
        .to_colors()
        .chunks(width)
        .map(|row| {
            row.iter()
                .map(|c| if *c == Color::Dark { '1' } else { '0' })
                .collect()
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_square_and_start_with_a_finder_pattern() {
        let rows = qr_rows("ABCD-EFGH-JKMN-PQRS-TVWX").unwrap();
        assert!(rows.len() >= 21);
        assert!(rows.iter().all(|r| r.len() == rows.len()));
        assert!(rows[0].starts_with("1111111"));
        assert!(
            rows.iter()
                .all(|r| r.bytes().all(|b| b == b'0' || b == b'1'))
        );
    }

    #[test]
    fn a_code_decodes_back_to_its_text() {
        let text = "ABCD-EFGH-JKMN-PQRS-TVWX";
        let rows = qr_rows(text).unwrap();
        // Draw with a quiet zone, as the window does, and read it back.
        let n = rows.len() + 8;
        let img = rqrr::PreparedImage::prepare_from_greyscale(n * 4, n * 4, |x, y| {
            let (cx, cy) = (x / 4, y / 4);
            let dark = cx >= 4
                && cy >= 4
                && rows
                    .get(cy - 4)
                    .and_then(|r| r.as_bytes().get(cx - 4))
                    .is_some_and(|b| *b == b'1');
            if dark { 0 } else { 255 }
        });
        let mut img = img;
        let grids = img.detect_grids();
        assert_eq!(grids.len(), 1);
        let (_, content) = grids[0].decode().unwrap();
        assert_eq!(content, text);
    }
}

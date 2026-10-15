//! What a file kept with an entry may be, and how it is handed back.
//!
//! A file is taken for what its bytes are, not for what the uploader calls it:
//! a page served from this origin with the type a client claimed is a page
//! that can run a script with the person's session. So the type is read from
//! the first bytes, only the kinds a receipt or a statement comes as are
//! taken, and each goes back out the way that kind is safe to open.

/// The largest file kept: a phone's photo of a receipt is two to six
/// megabytes, a bank statement's PDF less.
pub const MAX_BYTES: usize = 10 * 1024 * 1024;

/// The largest request a file may arrive in: somewhat more than the file, so
/// one just over the limit is answered with what the limit is rather than cut
/// off unread. The gateway carries requests this large and no larger.
pub const BODY_LIMIT: usize = MAX_BYTES + 2 * 1024 * 1024;

/// What a file is, read from its bytes; `None` for anything else.
#[must_use]
pub fn media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    // What an iPhone's camera writes: an ISO media file whose brand says HEIF.
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return match &bytes[8..12] {
            b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" => Some("image/heic"),
            b"mif1" | b"msf1" => Some("image/heif"),
            _ => None,
        };
    }
    if bytes.starts_with(b"%PDF-") {
        return Some("application/pdf");
    }
    // A receipt as the tax authority issues it (SIFEN in Paraguay). Only a
    // document that says it is XML: a bare `<` could as well be HTML.
    let text = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let start = text.iter().position(|byte| !byte.is_ascii_whitespace()).unwrap_or(text.len());
    if text[start..].starts_with(b"<?xml") {
        return Some("application/xml");
    }
    None
}

/// Whether a browser may show a file of this type in place.
///
/// Pictures and PDFs cannot run anything in the page that opens them. An XML
/// document can - one in the XHTML namespace is a page - so it is only ever
/// saved, never opened.
#[must_use]
pub fn opens_in_place(media_type: &str) -> bool {
    media_type.starts_with("image/") || media_type == "application/pdf"
}

/// The digits a percent-encoded byte is written in.
const HEX: &[u8; 16] = b"0123456789ABCDEF";

/// A file's name as it is kept: the last part of whatever path a browser
/// sent, without control characters, and not empty.
#[must_use]
pub fn file_name(raw: &str) -> Option<String> {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
    let cleaned: String = last.chars().filter(|c| !c.is_control()).collect();
    let trimmed = cleaned.trim();
    // Long enough for any name a phone gives a photo, short enough that a
    // header carrying it stays a header.
    let kept: String = trimmed.chars().take(200).collect();
    (!kept.is_empty()).then_some(kept)
}

/// The `Content-Disposition` a file goes out with: shown in place or saved,
/// under its own name.
///
/// The name twice, as RFC 6266 has it: an ASCII fallback for any reader, and
/// the name itself percent-encoded as UTF-8 for those that read `filename*` -
/// every browser - so `чек.jpg` is saved as `чек.jpg`.
#[must_use]
pub fn disposition(media_type: &str, name: &str) -> String {
    let how = if opens_in_place(media_type) { "inline" } else { "attachment" };
    let fallback: String = name
        .chars()
        .map(|c| {
            if (c.is_ascii_graphic() && c != '"' && c != '\\') || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut encoded = String::new();
    for byte in name.bytes() {
        if byte.is_ascii_alphanumeric() || b"!#$&+-.^_`|~".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0F)]));
        }
    }
    format!("{how}; filename=\"{fallback}\"; filename*=UTF-8''{encoded}")
}

#[cfg(test)]
mod tests {
    use super::{disposition, file_name, media_type, opens_in_place};

    #[test]
    fn a_file_is_what_its_bytes_say_whatever_it_is_called() {
        assert_eq!(media_type(b"\xFF\xD8\xFF\xE0\x00\x10JFIF"), Some("image/jpeg"));
        assert_eq!(media_type(b"\x89PNG\r\n\x1a\n\x00\x00"), Some("image/png"));
        assert_eq!(media_type(b"RIFF\x24\x00\x00\x00WEBPVP8 "), Some("image/webp"));
        assert_eq!(media_type(b"\x00\x00\x00\x18ftypheic\x00\x00"), Some("image/heic"));
        assert_eq!(media_type(b"\x00\x00\x00\x18ftypmif1\x00\x00"), Some("image/heif"));
        assert_eq!(media_type(b"%PDF-1.7\n"), Some("application/pdf"));
        assert_eq!(media_type(b"\xEF\xBB\xBF  <?xml version=\"1.0\"?><rDE/>"), Some("application/xml"));
    }

    #[test]
    fn a_page_is_not_taken_whatever_it_is_called() {
        // The case the sniffing is for: a page uploaded as `receipt.jpg` and
        // served from this origin would run with the person's session.
        assert_eq!(media_type(b"<html><script>alert(1)</script>"), None);
        assert_eq!(media_type(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"), None);
        assert_eq!(media_type(b"\x00\x00\x00\x18ftypisom"), None, "a video is not a receipt");
        assert_eq!(media_type(b""), None);
    }

    #[test]
    fn only_what_cannot_run_a_script_opens_in_place() {
        assert!(opens_in_place("image/jpeg"));
        assert!(opens_in_place("application/pdf"));
        assert!(!opens_in_place("application/xml"));
    }

    #[test]
    fn a_name_is_its_last_part_without_what_a_header_cannot_carry() {
        assert_eq!(file_name("C:\\Users\\me\\Pictures\\receipt.jpg").as_deref(), Some("receipt.jpg"));
        assert_eq!(file_name("/tmp/scan.pdf").as_deref(), Some("scan.pdf"));
        assert_eq!(file_name("чек\r\n.jpg").as_deref(), Some("чек.jpg"));
        assert_eq!(file_name("   "), None);
        assert_eq!(file_name("dir/"), None);
    }

    #[test]
    fn a_name_goes_out_twice_so_every_reader_can_save_it() {
        assert_eq!(
            disposition("image/jpeg", "чек 1.jpg"),
            "inline; filename=\"___ 1.jpg\"; filename*=UTF-8''%D1%87%D0%B5%D0%BA%201.jpg"
        );
        assert!(disposition("application/xml", "factura.xml").starts_with("attachment; "));
        // A quote in a name cannot end the header's own quoting early.
        assert!(disposition("application/pdf", "a\"b.pdf").contains("filename=\"a_b.pdf\""));
    }
}

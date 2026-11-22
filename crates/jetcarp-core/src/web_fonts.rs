//! Atkinson Hyperlegible Next and Mono as WOFF2 files, for the HTML pages JETCARP writes (the
//! clip-rating page, the website export's page): fonts whose letters are easier to tell apart,
//! designed by the Braille Institute for low-vision readers, as the app and the book are set in.
//! Each page carries them in its style sheet as `data:` addresses ([`font_faces`]), so it works as
//! one file, offline, and fetches nothing from another site.
//!
//! The files are the copyright holders' own, whole and unmodified, under the SIL Open Font
//! License 1.1, the licences beside them (`fonts/OFL-*.txt`); `fonts/README.md` gives where each
//! came from and its SHA-256.

/// Atkinson Hyperlegible Next Regular.
pub const NEXT_REGULAR: &[u8] = include_bytes!("../fonts/AtkinsonHyperlegibleNext-Regular.woff2");
/// Atkinson Hyperlegible Next Bold.
pub const NEXT_BOLD: &[u8] = include_bytes!("../fonts/AtkinsonHyperlegibleNext-Bold.woff2");
/// Atkinson Hyperlegible Mono Regular.
pub const MONO_REGULAR: &[u8] = include_bytes!("../fonts/AtkinsonHyperlegibleMono-Regular.woff2");

/// The families, as a page's `font-family` names them.
pub const NEXT: &str = "Atkinson Hyperlegible Next";
pub const MONO: &str = "Atkinson Hyperlegible Mono";

/// The fonts a page carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Faces {
    /// Next, regular and bold.
    Text,
    /// Next, regular and bold, and Mono.
    TextAndMono,
}

/// CSS `@font-face` rules for the fonts, each file inside as a `data:` address. Next's regular
/// face covers weights to 549 and its bold the rest; Mono's one face covers every weight, so no
/// browser makes up a bold of its own.
pub fn font_faces(faces: Faces) -> String {
    let face = |family: &str, weights: &str, bytes: &[u8]| {
        format!(
            "@font-face {{ font-family: \"{family}\"; font-style: normal; font-weight: {weights}; \
             font-display: swap; src: url(\"data:font/woff2;base64,{}\") format(\"woff2\"); }}\n",
            base64(bytes)
        )
    };
    let mut css = String::from(
        "/* Atkinson Hyperlegible Next and Mono (SIL Open Font License 1.1), the Braille \
         Institute's, as JETCARP ships them. */\n",
    );
    css += &face(NEXT, "100 549", NEXT_REGULAR);
    css += &face(NEXT, "550 900", NEXT_BOLD);
    if faces == Faces::TextAndMono {
        css += &face(MONO, "100 900", MONO_REGULAR);
    }
    css
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `data` in base64 (RFC 4648, with padding).
pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The files are the ones `fonts/README.md` names, byte for byte: the book's.
    #[test]
    fn the_page_fonts_are_the_ones_the_readme_names() {
        let readme = include_str!("../fonts/README.md");
        for (file, bytes) in [
            ("AtkinsonHyperlegibleNext-Regular.woff2", NEXT_REGULAR),
            ("AtkinsonHyperlegibleNext-Bold.woff2", NEXT_BOLD),
            ("AtkinsonHyperlegibleMono-Regular.woff2", MONO_REGULAR),
            (
                "OFL-AtkinsonHyperlegibleNext.txt",
                include_bytes!("../fonts/OFL-AtkinsonHyperlegibleNext.txt"),
            ),
            (
                "OFL-AtkinsonHyperlegibleMono.txt",
                include_bytes!("../fonts/OFL-AtkinsonHyperlegibleMono.txt"),
            ),
        ] {
            let line = readme
                .lines()
                .find(|l| l.starts_with(&format!("| `{file}`")))
                .unwrap_or_else(|| panic!("{file} is not in fonts/README.md"));
            assert!(
                line.contains(&crate::sha256::sha256_hex(bytes)),
                "{file}'s SHA-256 is not the README's: {line}"
            );
            if file.ends_with(".woff2") {
                assert_eq!(&bytes[..4], b"wOF2", "{file} is a WOFF2 file");
            }
        }
    }

    #[test]
    fn base64_matches_rfc4648() {
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), coded);
        }
    }

    #[test]
    fn a_page_carries_the_fonts_it_names() {
        let text = font_faces(Faces::Text);
        assert_eq!(text.matches("@font-face").count(), 2);
        assert!(text.contains(&base64(NEXT_REGULAR)));
        assert!(text.contains(&base64(NEXT_BOLD)));
        assert!(!text.contains(MONO));
        let both = font_faces(Faces::TextAndMono);
        assert_eq!(both.matches("@font-face").count(), 3);
        assert!(both.contains(&base64(MONO_REGULAR)));
        // Nothing fetched from elsewhere.
        assert!(!both.contains("http"));
    }
}

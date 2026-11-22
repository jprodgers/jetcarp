//! The fonts JETCARP ships: Atkinson Hyperlegible Next and its monospaced sibling, Atkinson
//! Hyperlegible Mono, fonts whose letters are easier to tell apart (designed by the Braille
//! Institute for low-vision readers). The app draws its words in them by default, and so does the
//! caption on surfaces with no egui ([`crate::caption::CaptionPainter`]).
//!
//! The files are the copyright holders' own, whole and unmodified, under the SIL Open Font
//! License 1.1, each family's licence beside it (`fonts/*/OFL.txt`); `fonts/README.md` gives
//! where each came from and its SHA-256.

/// Atkinson Hyperlegible Next Regular.
pub const NEXT_REGULAR: &[u8] =
    include_bytes!("../fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Regular.ttf");

/// Atkinson Hyperlegible Next Bold (the app's headings).
pub const NEXT_BOLD: &[u8] =
    include_bytes!("../fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Bold.ttf");

/// Atkinson Hyperlegible Mono Regular.
pub const MONO_REGULAR: &[u8] =
    include_bytes!("../fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-Regular.ttf");

/// The family names, as the fonts give them.
pub const NEXT_NAME: &str = "Atkinson Hyperlegible Next";
pub const MONO_NAME: &str = "Atkinson Hyperlegible Mono";

#[cfg(test)]
mod tests {
    use super::*;
    use ab_glyph::{Font, FontRef};

    /// The files are the ones `fonts/README.md` names, byte for byte.
    #[test]
    fn the_shipped_fonts_are_the_ones_the_readme_names() {
        let readme = include_str!("../fonts/README.md");
        for (file, bytes) in [
            ("AtkinsonHyperlegibleNext-Regular.ttf", NEXT_REGULAR),
            ("AtkinsonHyperlegibleNext-Bold.ttf", NEXT_BOLD),
            ("AtkinsonHyperlegibleMono-Regular.ttf", MONO_REGULAR),
        ] {
            let line = readme
                .lines()
                .find(|l| l.contains(file))
                .unwrap_or_else(|| panic!("{file} is not in fonts/README.md"));
            assert!(
                line.contains(&jetcarp_core::sha256::sha256_hex(bytes)),
                "{file}'s SHA-256 is not the README's: {line}"
            );
            assert!(FontRef::try_from_slice(bytes).is_ok(), "{file} parses");
        }
    }

    #[test]
    fn the_fonts_draw_basic_latin() {
        for bytes in [NEXT_REGULAR, NEXT_BOLD, MONO_REGULAR] {
            let f = FontRef::try_from_slice(bytes).unwrap();
            for c in (0x21u8..0x7f).map(char::from) {
                assert_ne!(f.glyph_id(c).0, 0, "{c:?}");
            }
        }
    }
}

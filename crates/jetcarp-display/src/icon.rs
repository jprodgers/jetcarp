//! JETCARP's app icon (#105), as the program embeds it: the files in this crate's `icons/`, which
//! `packaging/gen-icons.py` copies from `packaging/icons/`, where every raster is made from the
//! master, `jetcarp.svg`. These are constants, so a program carries only those it uses: the app
//! the 256 px picture for its windows, the launcher ([`crate::launcher`]) the Linux menu's sizes or
//! the macOS bundle's `.icns`.

/// The icon at 256 px: the app's windows' icon.
pub const PNG_256: &[u8] = include_bytes!("../icons/jetcarp-256.png");

/// The sizes the Linux menu's icon theme (`hicolor`) gets the icon at, in pixels.
pub const HICOLOR_SIZES: [u32; 8] = [16, 24, 32, 48, 64, 128, 256, 512];

/// The icon at each of [`HICOLOR_SIZES`].
pub const HICOLOR: [(u32, &[u8]); 8] = [
    (16, include_bytes!("../icons/jetcarp-16.png")),
    (24, include_bytes!("../icons/jetcarp-24.png")),
    (32, include_bytes!("../icons/jetcarp-32.png")),
    (48, include_bytes!("../icons/jetcarp-48.png")),
    (64, include_bytes!("../icons/jetcarp-64.png")),
    (128, include_bytes!("../icons/jetcarp-128.png")),
    (256, PNG_256),
    (512, include_bytes!("../icons/jetcarp-512.png")),
];

/// The macOS icon file, `jetcarp.icns`: PNGs from 16 to 1024 px.
pub const ICNS: &[u8] = include_bytes!("../icons/jetcarp.icns");

#[cfg(test)]
mod tests {
    use super::*;

    /// Each PNG is the size its name says (the width and height in its header).
    #[test]
    fn each_png_is_its_size() {
        assert_eq!(HICOLOR.map(|(n, _)| n), HICOLOR_SIZES);
        for (n, png) in HICOLOR {
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "{n}");
            let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
            let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
            assert_eq!((w, h), (n, n));
        }
        assert_eq!(&ICNS[..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(ICNS[4..8].try_into().unwrap()) as usize,
            ICNS.len()
        );
    }
}

//! The screen a script fills, and sizes and ranges written as text.

use jetcarp_core::SizeSpec;
use std::fmt::Display;
use std::str::FromStr;

/// The screen a script fills: `size: "screen"` worlds and default image sizes.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
    /// Tile size in pixels (square root of the mean tile area), as the app's *Tile size*.
    pub cell_px: f64,
}

impl Default for Screen {
    fn default() -> Self {
        Screen {
            width: 1920,
            height: 1080,
            cell_px: 8.0,
        }
    }
}

impl Screen {
    /// A screen size: `"1920x1080"` (also `X`, `*`, or `,` between), each side 16 to 65536.
    pub fn parse_size(s: &str) -> Result<(u32, u32), String> {
        let (w, h) = s
            .trim()
            .split_once(['x', 'X', '*', ','])
            .ok_or_else(|| format!("screen size '{s}' should look like 1920x1080"))?;
        let w: u32 = w
            .trim()
            .parse()
            .map_err(|_| format!("bad screen width '{w}'"))?;
        let h: u32 = h
            .trim()
            .parse()
            .map_err(|_| format!("bad screen height '{h}'"))?;
        if !(16..=65536).contains(&w) || !(16..=65536).contains(&h) {
            return Err(format!("screen size {w}x{h} out of range (16 to 65536)"));
        }
        Ok((w, h))
    }
}

/// `what: why` when `s` (trimmed) is not a number of type `T`.
fn number<T: FromStr>(s: &str, what: &str) -> Result<T, String>
where
    T::Err: Display,
{
    s.trim().parse().map_err(|e| format!("{what}: {e}"))
}

/// A world size as the command line takes it: `"64x48"` (unit cells across and down), a level
/// for aperiodic patches (`"L5"` or `"level5"`), or about this many cells (`"4096"`). Any case.
pub fn size_spec(s: &str) -> Result<SizeSpec, String> {
    let l = s.to_ascii_lowercase();
    if let Some((w, h)) = l.split_once('x') {
        return Ok(SizeSpec::Rect {
            w: number(w, "width")?,
            h: number(h, "height")?,
        });
    }
    if let Some(n) = l.strip_prefix("level").or_else(|| l.strip_prefix('l')) {
        return Ok(SizeSpec::Level(number(n, "level")?));
    }
    Ok(SizeSpec::Cells(number(
        &l,
        "size is WxH, a cell count, or L<level>",
    )?))
}

/// Width and height as `"16x12"` (or `X`), or one number for a square (`"16"`): soup and grid
/// sizes for searches.
pub fn size_pair(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once(['x', 'X'])
        .map(|(a, b)| (a.trim(), b.trim()))
        .unwrap_or((s.trim(), s.trim()));
    Ok((number(w, "width")?, number(h, "height")?))
}

/// A range of whole numbers, possibly negative: `"-3..3"`, `"0..=4"`, or one number (`"2"`, a
/// range of one). Returned low to high whichever way round it was written.
pub fn offset_range(s: &str) -> Result<[i32; 2], String> {
    let (a, b) = s
        .split_once("..")
        .map(|(a, b)| (a.trim(), b.trim().trim_start_matches('=')))
        .unwrap_or((s.trim(), s.trim()));
    let what = format!("range '{s}'");
    let a: i32 = number(a, &what)?;
    let b: i32 = number(b, &what)?;
    Ok([a.min(b), a.max(b)])
}

/// A range of counts: `"2-5"`, or one count (`"3"`, both ends). Kept in the order written.
pub fn count_range(s: &str) -> Result<(usize, usize), String> {
    let what = format!("range '{s}'");
    match s.split_once('-') {
        Some((a, b)) => Ok((number(a, &what)?, number(b, &what)?)),
        None => {
            let n = number(s, &what)?;
            Ok((n, n))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_sizes() {
        assert_eq!(Screen::parse_size("1920x1080").unwrap(), (1920, 1080));
        assert_eq!(Screen::parse_size(" 800 * 600 ").unwrap(), (800, 600));
        assert_eq!(Screen::parse_size("640,480").unwrap(), (640, 480));
        assert!(Screen::parse_size("1920").is_err());
        assert!(Screen::parse_size("8x8").is_err());
        assert!(Screen::parse_size("axb").is_err());
    }

    #[test]
    fn world_sizes_as_text() {
        assert_eq!(size_spec("64x48").unwrap(), SizeSpec::Rect { w: 64, h: 48 });
        assert_eq!(size_spec("64X48").unwrap(), SizeSpec::Rect { w: 64, h: 48 });
        assert_eq!(size_spec("L5").unwrap(), SizeSpec::Level(5));
        assert_eq!(size_spec("level 4").unwrap(), SizeSpec::Level(4));
        assert_eq!(size_spec("4096").unwrap(), SizeSpec::Cells(4096));
        assert!(size_spec("64xh").unwrap_err().starts_with("height: "));
        assert!(size_spec("big").unwrap_err().starts_with("size is WxH"));
    }

    #[test]
    fn size_pairs() {
        assert_eq!(size_pair("16x12").unwrap(), (16, 12));
        assert_eq!(size_pair(" 16 X 12 ").unwrap(), (16, 12));
        assert_eq!(size_pair("16").unwrap(), (16, 16));
        assert!(size_pair("16x").unwrap_err().starts_with("height: "));
        assert!(size_pair("ax2").unwrap_err().starts_with("width: "));
    }

    #[test]
    fn ranges() {
        assert_eq!(offset_range("-3..3").unwrap(), [-3, 3]);
        assert_eq!(offset_range("4..=0").unwrap(), [0, 4]);
        assert_eq!(offset_range("2").unwrap(), [2, 2]);
        assert!(
            offset_range("1..x")
                .unwrap_err()
                .starts_with("range '1..x': ")
        );
        assert_eq!(count_range("2-5").unwrap(), (2, 5));
        assert_eq!(count_range("5-2").unwrap(), (5, 2));
        assert_eq!(count_range(" 3 ").unwrap(), (3, 3));
        assert!(count_range("2-").is_err());
    }
}

//! The screen a script fills.

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
}

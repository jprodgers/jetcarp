//! The app's windows' icon (#105): JETCARP's icon at 256 px ([`jetcarp_display::icon::PNG_256`]),
//! decoded once. Windows and X11 show it in the title bar and the taskbar, macOS in the Dock;
//! Wayland shows the icon of the menu entry named after the windows' app id, `jetcarp`
//! (`jetcarp launcher install` adds it).

use eframe::egui::IconData;
use std::sync::{Arc, OnceLock};

/// The icon every window of the app is given (an empty one, which shows none, if the picture
/// could not be read).
pub fn window_icon() -> Arc<IconData> {
    static ICON: OnceLock<Arc<IconData>> = OnceLock::new();
    ICON.get_or_init(|| Arc::new(decode(jetcarp_display::icon::PNG_256).unwrap_or_default()))
        .clone()
}

/// A PNG as 8-bit RGBA, whatever its colour type and depth.
fn decode(png_bytes: &[u8]) -> Result<IconData, String> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(png_bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let channels = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("an indexed PNG was not expanded".into()),
    };
    let mut rgba = Vec::with_capacity(w * h * 4);
    for px in buf[..w * h * channels].chunks_exact(channels) {
        rgba.extend_from_slice(&match channels {
            1 => [px[0], px[0], px[0], 255],
            2 => [px[0], px[0], px[0], px[1]],
            3 => [px[0], px[1], px[2], 255],
            _ => [px[0], px[1], px[2], px[3]],
        });
    }
    Ok(IconData {
        rgba,
        width: info.width,
        height: info.height,
    })
}

#[cfg(test)]
mod tests {
    use super::window_icon;

    #[test]
    fn the_window_icon_is_jetcarps_at_256_px() {
        let icon = window_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        let px = |x: usize, y: usize| &icon.rgba[(y * 256 + x) * 4..][..4];
        // Whatever the drawing: a rounded tile, so its corners are transparent and its middle
        // opaque, with more than one colour on it (the drawing is read, not a blank square).
        for (x, y) in [(0, 0), (255, 0), (0, 255), (255, 255)] {
            assert_eq!(px(x, y)[3], 0, "corner ({x}, {y})");
        }
        assert_eq!(px(128, 128)[3], 255);
        let colours: std::collections::HashSet<&[u8]> =
            icon.rgba.chunks(4).filter(|p| p[3] == 255).collect();
        assert!(colours.len() > 1, "one colour only");
        // The same picture each time, decoded once.
        assert!(Arc::ptr_eq(&icon, &window_icon()));
    }

    use std::sync::Arc;
}

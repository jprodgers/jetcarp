//! The strobe guard (#97): measuring the flashes, red flashes and strong stripes in what JETCARP
//! draws, and acting on them before they reach a rate that can trigger a seizure.
//!
//! **What is measured is what reaches the screen**, never what a rule "should" do: a period-2
//! rule run at twice the frame rate shows the same phase in every frame and does not flash, and a
//! slow rule drawn with a bright palette can. Every drawn frame is reduced to a coarse map of
//! *tiles* of [`TILE`] by [`TILE`] pixels ([`frame::TileMap`]): each tile's mean relative
//! luminance, and its mean red measure. Where frames are drawn on the GPU the reduction runs there
//! (`jetcarp_display::strobe_gpu`); everywhere else ([`frame::reduce`]) on the processor. Both
//! give the same map, and everything after the map is this module's, so one implementation
//! decides for every output.
//!
//! **The definitions** are WCAG 2.3.1's, as the Trace Center's PEAT and EA's IRIS apply them:
//!
//! - *Relative luminance* of an sRGB colour: each channel decoded to linear light, then
//!   `0.2126 R + 0.7152 G + 0.0722 B` ([`luminance`]).
//! - A *transition* is a change of relative luminance of at least [`FLASH_DELTA`] (10% of the
//!   maximum) in one direction, counted once per run, where the darker of its two states is below
//!   [`DARK_LIMIT`]. A *flash* is a pair of opposing transitions.
//! - A *red transition* is the same for the red measure ([`red_measure`]): for a saturated red
//!   (red at least [`RED_SATURATION`] of `R + G + B`), `(R - G - B) * 320`, changing by at least
//!   [`RED_DELTA`].
//! - More than three flashes in any one second fail: more than [`MAX_TRANSITIONS`] transitions
//!   in a sliding one-second window. [`WARN_TRANSITIONS`] (two flashes a second) is a warning, and
//!   a warning that lasts four seconds out of five is the *extended* failure IRIS reports
//!   (Harding's guidance on sustained flashing).
//! - **The area rule:** flashes are harmful only where the flashing areas together cover more than
//!   [`AREA_SHARE`] of any 10° field of vision. The size of that field in pixels depends on the
//!   screen and on how far away it is watched, which JETCARP cannot know, so it assumes a
//!   conservative case ([`Viewing`]): a screen of [`REFERENCE_DPI`] pixels an inch for each step of
//!   its scale factor, watched from [`VIEWING_DISTANCE_MM`]. A closer view or larger pixels
//!   would make the field fewer pixels across; few people watch from closer.
//!
//! A tile counts its own transitions; the area rule then asks how much of the worst-placed 10°
//! field is covered by tiles that made at least `k` transitions in the last second. Alongside, the
//! mean of the whole screen is tracked the way IRIS tracks a recording's frame, so anything IRIS
//! would count on a recording of the screen is counted here too ([`flash::FlashMeter`]).
//!
//! **Stripes** ([`pattern`]): regular high-contrast stripes can trigger seizures as flashes can
//! (ITU-R BT.1702, the Harding guidance). They are measured on a half-resolution luminance image
//! of the frame ([`frame::LumImage`]).
//!
//! **The guard** ([`guard`]) acts on the readings, in order: it *softens* (each tile's brightness
//! may change only so fast, so a flip becomes a fade, and strong stripes lose their contrast),
//! then *caps the pace*, then *pauses* with a note saying why. It never changes what a world
//! computes, only what is drawn and how fast generations are shown.
//!
//! The guard reduces the risk; it does not make JETCARP safe for someone with photosensitive
//! epilepsy, and the photosensitive seizure warning ([`crate::photosensitivity`]) still stands.

pub mod flash;
pub mod frame;
pub mod soften;

use std::sync::OnceLock;

/// Tiles are this many pixels on a side. A quarter of the conservative 10° field ([`Viewing`])
/// is about 54 tiles at a scale factor of 1, so the area rule is measured in small steps.
pub const TILE: u32 = 16;

/// A transition: a change of relative luminance of at least this much (WCAG 2.3.1: 10% of the
/// maximum relative luminance).
pub const FLASH_DELTA: f32 = 0.1;

/// A transition counts only when the darker of its two states is below this relative luminance.
pub const DARK_LIMIT: f32 = 0.8;

/// A red transition: a change of the red measure ([`red_measure`]) of at least this much.
pub const RED_DELTA: f32 = 20.0;

/// A colour is a saturated red when red makes up at least this share of `R + G + B` (linear).
pub const RED_SATURATION: f32 = 0.8;

/// The red measure's scale: `(R - G - B) * RED_SCALE`.
pub const RED_SCALE: f32 = 320.0;

/// More transitions than this in one second fail: more than three flashes a second.
pub const MAX_TRANSITIONS: u32 = 6;

/// This many transitions in one second (two flashes) is a warning, and a warning that lasts
/// [`EXTENDED_SECONDS`] of [`EXTENDED_WINDOW`] seconds is the extended failure.
pub const WARN_TRANSITIONS: u32 = 4;

/// Seconds of warnings, within [`EXTENDED_WINDOW`], that make the extended failure.
pub const EXTENDED_SECONDS: f64 = 4.0;

/// The window of the extended failure, in seconds.
pub const EXTENDED_WINDOW: f64 = 5.0;

/// Flashes count when the flashing areas cover more than this share of a 10° field of vision.
pub const AREA_SHARE: f64 = 0.25;

/// The field of vision the area rule is a share of, in degrees across.
pub const FIELD_DEGREES: f64 = 10.0;

/// The viewing distance JETCARP assumes, in millimetres: closer than most people sit from a
/// desktop monitor (50 to 70 cm) or a laptop (40 to 50 cm), so the field it assumes is no larger
/// than the real one.
pub const VIEWING_DISTANCE_MM: f64 = 400.0;

/// The pixel density JETCARP assumes for each step of a screen's scale factor, in pixels an inch:
/// a scale factor of 1 is taken as a 96 dpi screen, the largest pixels in common use, so a
/// 10° field is the fewest pixels it can be.
pub const REFERENCE_DPI: f64 = 96.0;

/// sRGB bytes decoded to linear light, as the sRGB standard defines it.
fn linear_table() -> &'static [f32; 256] {
    static T: OnceLock<[f32; 256]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0f32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let c = i as f64 / 255.0;
            *v = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            } as f32;
        }
        t
    })
}

/// An sRGB byte in linear light (0 to 1).
pub fn linear(byte: u8) -> f32 {
    linear_table()[byte as usize]
}

/// A linear-light value (0 to 1) as an sRGB value (0 to 1, not rounded).
pub fn encode(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Relative luminance of linear-light red, green and blue (each 0 to 1).
pub fn luminance_linear(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Relative luminance of an sRGB colour.
pub fn luminance(rgb: [u8; 3]) -> f32 {
    luminance_linear(linear(rgb[0]), linear(rgb[1]), linear(rgb[2]))
}

/// The red measure of linear-light red, green and blue: `(R - G - B) * 320` when red makes up at
/// least [`RED_SATURATION`] of `R + G + B`, never below 0; otherwise 0. It runs from 0 to 320.
pub fn red_measure_linear(r: f32, g: f32, b: f32) -> f32 {
    let s = r + g + b;
    if s > 0.0 && r >= RED_SATURATION * s {
        ((r - g - b) * RED_SCALE).max(0.0)
    } else {
        0.0
    }
}

/// The red measure of an sRGB colour.
pub fn red_measure(rgb: [u8; 3]) -> f32 {
    red_measure_linear(linear(rgb[0]), linear(rgb[1]), linear(rgb[2]))
}

/// How a screen is watched, as far as the area rule needs it: how many pixels a 10° field of
/// vision covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewing {
    /// Pixels across the 10° field (its diameter).
    pub field_px: f64,
}

impl Viewing {
    /// A screen with this scale factor (physical pixels per logical pixel), watched as JETCARP
    /// assumes ([`REFERENCE_DPI`], [`VIEWING_DISTANCE_MM`]): 265 pixels across at a scale of 1.
    pub fn screen(scale: f64) -> Viewing {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale.max(1.0)
        } else {
            1.0
        };
        let px_per_mm = REFERENCE_DPI * scale / 25.4;
        let half = (FIELD_DEGREES / 2.0).to_radians().tan();
        Viewing {
            field_px: 2.0 * VIEWING_DISTANCE_MM * half * px_per_mm,
        }
    }

    /// An animation of `width` by `height` pixels, which may be shown at any size: the smaller of
    /// a screen at a scale of 1 and WCAG's own estimate for content of unknown size, a field a
    /// third of its width by a third of its height (341 by 256 at 1024 by 768).
    pub fn animation(width: u32, height: u32) -> Viewing {
        let screen = Viewing::screen(1.0);
        let third = (width as f64 / 3.0) * (height as f64 / 3.0);
        // A circle of the same area as that rectangle.
        let d = (third * 4.0 / std::f64::consts::PI).sqrt();
        Viewing {
            field_px: screen.field_px.min(d).max(TILE as f64),
        }
    }

    /// The field's area in pixels (a circle `field_px` across).
    pub fn field_area(&self) -> f64 {
        std::f64::consts::PI / 4.0 * self.field_px * self.field_px
    }

    /// The largest flashing area allowed in one field, in pixels: [`AREA_SHARE`] of it.
    pub fn flash_area(&self) -> f64 {
        self.field_area() * AREA_SHARE
    }

    /// The side, in tiles, of the square of tiles the area rule slides over the screen: the
    /// square with the field's area, at least one tile.
    pub fn field_tiles(&self) -> u32 {
        ((self.field_area().sqrt() / TILE as f64).round() as u32).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luminance_follows_the_srgb_definition() {
        assert_eq!(luminance([0, 0, 0]), 0.0);
        assert!((luminance([255, 255, 255]) - 1.0).abs() < 1e-6);
        // Mid grey is about a fifth of white's luminance, not half.
        assert!((luminance([128, 128, 128]) - 0.2158605).abs() < 1e-5);
        assert!((luminance([255, 0, 0]) - 0.2126).abs() < 1e-6);
        assert!((luminance([0, 255, 0]) - 0.7152).abs() < 1e-6);
        assert!((luminance([0, 0, 255]) - 0.0722).abs() < 1e-6);
        // The linear segment at the bottom of the curve.
        assert!((linear(10) - 10.0 / 255.0 / 12.92).abs() < 1e-7);
        // Encoding undoes decoding.
        for b in [0u8, 1, 10, 11, 64, 128, 200, 255] {
            assert!((encode(linear(b)) * 255.0 - b as f32).abs() < 1e-3, "{b}");
        }
    }

    #[test]
    fn the_red_measure_counts_only_saturated_red() {
        assert!((red_measure([255, 0, 0]) - 320.0).abs() < 1e-3);
        assert_eq!(red_measure([0, 0, 0]), 0.0);
        assert_eq!(red_measure([255, 255, 255]), 0.0);
        // Dark red is still saturated.
        assert!((red_measure([128, 0, 0]) - 0.2158605 * 320.0).abs() < 1e-2);
        // Orange: red is 82% of the sum, so it counts, less the green.
        let o = red_measure([255, 128, 0]);
        assert!((o - (1.0 - 0.2158605) * 320.0).abs() < 1e-2, "{o}");
        // Pink: red is less than 80% of the sum.
        assert_eq!(red_measure([255, 128, 128]), 0.0);
        // Magenta-ish: blue takes it below the share.
        assert_eq!(red_measure([255, 0, 200]), 0.0);
    }

    #[test]
    fn the_field_is_conservative_and_scales_with_the_screen() {
        let v = Viewing::screen(1.0);
        assert!((v.field_px - 264.53).abs() < 0.01, "{}", v.field_px);
        // A quarter of the field, about 13,700 pixels: under 0.7% of a 1080p screen.
        assert!(
            (v.flash_area() - 13_738.0).abs() < 5.0,
            "{}",
            v.flash_area()
        );
        assert_eq!(v.field_tiles(), 15);
        // A 2x screen has twice the pixels across the same field.
        let v2 = Viewing::screen(2.0);
        assert!((v2.field_px / v.field_px - 2.0).abs() < 1e-9);
        // Scale factors below 1, and nonsense, count as 1.
        assert_eq!(Viewing::screen(0.5), v);
        assert_eq!(Viewing::screen(f64::NAN), v);
        // WCAG's estimate at 1024 x 768 is a 341 x 256 field: the screen's is smaller.
        assert_eq!(Viewing::animation(1024, 768), v);
        // A small animation might be shown large: a third of it each way.
        let a = Viewing::animation(300, 300);
        assert!(
            (a.field_area() - 100.0 * 100.0).abs() < 1.0,
            "{}",
            a.field_area()
        );
        assert!(Viewing::animation(1, 1).field_px >= TILE as f64);
    }
}

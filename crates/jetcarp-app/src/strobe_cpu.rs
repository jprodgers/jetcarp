//! The strobe guard for what the app animates on the processor (#149): the Quick Start's live
//! cards, and the stored or rendered animations the hunts' dashboard and the ants window play.
//!
//! - **Played animations** ([`guard_frames`]) are checked as an export is
//!   (`jetcarp_core::strobe::export`): one that flashes or shows strong stripes plays slower, or
//!   softened, as an export would be saved.
//! - **Live cards** ([`LiveGuard`]) pass each new picture through the guard's softening, as a GPU
//!   view does, on the processor. A card is one among many on the screen, so its field is the card
//!   itself: a quarter of a card flashing counts, since a screen of such cards may flash together.
//!
//! The sandpile preview changes at most four times a second and the HashLife view only when a jump
//! is asked for, so neither can flash more than twice a second on its own.

use jetcarp_core::photosensitivity::{GuardPlace, guard_on};
use jetcarp_core::render::Image;
use jetcarp_core::strobe::Viewing;
use jetcarp_core::strobe::export::{check, slowed_delay, soften};
use jetcarp_core::strobe::frame::reduce_both;
use jetcarp_core::strobe::guard::{Frame, Guard, Pace};
use jetcarp_core::strobe::soften::Softener;

/// Frames to play, each `delay_ms`: as they are, or softened (and slowed if need be) when they
/// flash or show strong stripes and the app's guard is on.
pub fn guard_frames(frames: Vec<Image>, delay_ms: u32) -> (Vec<Image>, u32) {
    if frames.is_empty() || !guard_on(GuardPlace::App) {
        return (frames, delay_ms);
    }
    let refs: Vec<&Image> = frames.iter().collect();
    let before = check(&refs, delay_ms);
    if !before.harmful {
        return (frames, delay_ms);
    }
    // Slower, as an export would be, when a little slower is enough and there are no stripes.
    if before.stripes.level != jetcarp_core::strobe::pattern::PatternLevel::Fail {
        let d = slowed_delay(&before, delay_ms);
        if d <= delay_ms.saturating_mul(jetcarp_core::strobe::export::MAX_SLOWER)
            && !check(&refs, d).harmful
        {
            return (frames, d);
        }
    }
    let soft = soften(&frames, delay_ms);
    let after = check(&soft.iter().collect::<Vec<_>>(), delay_ms);
    if after.harmful {
        let d = slowed_delay(&after, delay_ms);
        (soft, d)
    } else {
        (soft, delay_ms)
    }
}

/// A live picture's strobe guard (a Quick Start card).
pub struct LiveGuard {
    guard: Guard,
    soft: Softener,
    last: Option<f64>,
}

impl Default for LiveGuard {
    fn default() -> Self {
        LiveGuard::new()
    }
}

impl LiveGuard {
    pub fn new() -> LiveGuard {
        LiveGuard {
            guard: Guard::new(Viewing::screen(1.0), None, true),
            soft: Softener::new(),
            last: None,
        }
    }

    /// The picture to show for `rgba` (`w` by `h`, sRGB), new at `now` (seconds): None to show it
    /// as it is, else the softened picture. `on` is the app's setting.
    pub fn show(&mut self, rgba: &[u8], w: u32, h: u32, now: f64, on: bool) -> Option<Vec<u8>> {
        // The card is its own field.
        let field = Viewing {
            field_px: ((w as f64 * h as f64) * 4.0 / std::f64::consts::PI).sqrt(),
        };
        self.guard.set_viewing(field, None);
        self.guard.set_on(on);
        let dt = self.last.map_or(0.1, |l| (now - l).max(0.0)) as f32;
        self.last = Some(now);
        let plan = if on {
            self.guard.plan().clone()
        } else {
            Default::default()
        };
        let s = self.soft.apply(rgba, w, h, &plan, dt);
        let half = self
            .guard
            .wants_half(now)
            .then(|| reduce_both(rgba, w, h).1);
        self.guard.observe(
            Frame {
                time: now,
                input: &s.input,
                output: &s.output,
                half: half.as_ref(),
            },
            Pace {
                speed: 10.0,
                reached: None,
                frame_rate: 10.0,
            },
        );
        s.shown
    }

    /// Whether the guard is acting on this picture.
    pub fn acting(&self) -> bool {
        self.guard.plan().active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jetcarp_core::strobe::flash::FlashMeter;
    use jetcarp_core::strobe::frame::reduce;

    fn solid(w: u32, h: u32, c: u8) -> Vec<u8> {
        let mut v = vec![c; (w * h * 4) as usize];
        v.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
        v
    }

    /// A card flipping black and white at its ten pictures a second: what it shows stays under the
    /// limit, measured with the card as its own field.
    #[test]
    fn a_flipping_card_is_softened() {
        let (w, h) = (160, 120);
        let (b, wh) = (solid(w, h, 0), solid(w, h, 255));
        let mut g = LiveGuard::new();
        let field = Viewing {
            field_px: ((w * h) as f64 * 4.0 / std::f64::consts::PI).sqrt(),
        };
        let mut meter = FlashMeter::new(field, None);
        let mut most = 0;
        for k in 0..100 {
            let t = k as f64 / 10.0;
            let src = if k % 2 == 0 { &b } else { &wh };
            let shown = g.show(src, w, h, t, true);
            let px = shown.as_deref().unwrap_or(src);
            most = most.max(meter.update(&reduce(px, w, h), t).transitions);
        }
        assert!(most <= 6, "{most}");
        assert!(g.acting());
        // Switched off, the card shows what it draws.
        assert!(g.show(&b, w, h, 20.0, false).is_none());
    }

    #[test]
    fn played_frames_that_flash_are_softened() {
        let img = |c: u8| Image {
            width: 64,
            height: 64,
            rgba: solid(64, 64, c),
        };
        let frames: Vec<Image> = (0..20)
            .map(|k| img(if k % 2 == 0 { 0 } else { 255 }))
            .collect();
        let (out, d) = guard_frames(frames.clone(), 50);
        if guard_on(GuardPlace::App) {
            assert!(!check(&out.iter().collect::<Vec<_>>(), d).harmful);
            // A flip every frame needs far more than four times as long: softened.
            assert_ne!(out[1].rgba, frames[1].rgba);
        }
        // Harmless frames are left alone.
        let calm: Vec<Image> = (0..20).map(|k| img(k as u8)).collect();
        let (out, d) = guard_frames(calm.clone(), 50);
        assert_eq!((out.len(), d), (20, 50));
        assert_eq!(out[5].rgba, calm[5].rgba);
    }
}

//! Exported animations through the strobe guard (#149). An animated GIF or WebP posted online
//! flashes for whoever opens it, so an export measures its own frames before it is saved.
//!
//! For someone who answered the seizure warning *at risk*, the check and the softening use the
//! stricter stripe rule, similar parts regular or not ([`super::similar`]); `at_risk` says so.
//!
//! **The check** ([`check`]) plays the animation's frames at their own rate, looping as a player
//! would (at least twice, and for at least six seconds, so the loop's seam and the extended failure
//! are seen), through the flash meter and the stripe check, as shown at its own size on the screen
//! JETCARP assumes ([`Viewing::animation`]); the frame is the screen whose mean is tracked, as IRIS
//! tracks a recording, so a small animation that flashes all over is caught too. An animation fails, and is *harmful*, when
//! it flashes more than three times a second (or two to three for four seconds out of five), or
//! shows strong stripes for half a second.
//!
//! **The softened version** ([`soften`]) is the guard's softening carried out on the frames: every
//! tile that flashes anywhere in the loop is softened throughout, and stripes are veiled where they
//! were found. The softener runs over the loop twice and keeps the second pass, so the end flows
//! into the start.
//!
//! **The guard's choice** ([`guard`]): when showing each frame up to [`MAX_SLOWER`] times longer
//! brings it to one flash a second (and it has no strong stripes), the animation is slowed: every
//! frame stays as drawn, and an oscillator's few phases do not blur into one. Otherwise it is
//! softened. **What is saved is what is checked**: a GIF has at most 256 colours, and squeezing a
//! fade into them can turn it back into a flip, so the encoded file is decoded and checked again,
//! and if the softened file still fails it is slowed as well, until it passes.

use super::Viewing;
use super::flash::{FlashMeter, FlashReading, Level};
use super::frame::reduce_both;
use super::guard::{PATTERN_EVERY, veil_for};
use super::pattern::{PatternLevel, PatternMeter, PatternReading};
use super::soften::{Plan, Softener};
use crate::render::Image;
use serde::Serialize;

/// Seconds of looping a check covers at least.
const CHECK_SECONDS: f64 = 6.0;
/// Frames a check measures at most (a long animation is measured once through, and again for the
/// seam's second).
const MAX_CHECKED: usize = 20_000;
/// The longest a slowed frame is shown, in milliseconds.
const SLOWEST_MS: u32 = 2_000;
/// An animation is slowed rather than softened when this many times longer a frame is enough.
pub const MAX_SLOWER: u32 = 4;

/// What the check found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Check {
    /// The worst flash reading in the loop.
    pub flash: FlashReading,
    /// The worst stripe reading.
    pub stripes: PatternReading,
    /// It fails: flashing past the limits, or strong stripes for half a second.
    pub harmful: bool,
}

impl Check {
    /// What is wrong, in words ("flashes 4 or more times a second", "shows strong stripes over 60%
    /// of the frame"), or None.
    pub fn words(&self) -> Option<String> {
        let mut parts = vec![];
        if self.flash.worst().fails() {
            let t = self.flash.most();
            parts.push(if t >= super::flash::COUNT_CAP {
                format!("flashes {} or more times a second", t / 2)
            } else if self.flash.worst() == Level::Extended {
                format!(
                    "flashes {} times a second for seconds on end",
                    t as f32 / 2.0
                )
            } else {
                format!("flashes {} times a second", t as f32 / 2.0)
            });
        }
        if self.stripes.level == PatternLevel::Fail {
            parts.push(if self.stripes.similar {
                format!(
                    "shows a pattern of similar parts over {:.0}% of the frame",
                    self.stripes.share * 100.0
                )
            } else {
                format!(
                    "shows strong stripes over {:.0}% of the frame",
                    self.stripes.share * 100.0
                )
            });
        }
        (!parts.is_empty()).then(|| parts.join(" and "))
    }
}

/// How an export was saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Treatment {
    /// As drawn: it passed, or the guard was off.
    AsIs,
    /// The guard's softening.
    Softened,
    /// Each frame shown longer.
    Slowed,
}

impl Treatment {
    pub fn name(self) -> &'static str {
        match self {
            Treatment::AsIs => "as_is",
            Treatment::Softened => "softened",
            Treatment::Slowed => "slowed",
        }
    }
}

/// An export as saved.
#[derive(Clone, Debug)]
pub struct Guarded {
    pub bytes: Vec<u8>,
    /// Each frame's time, in milliseconds (longer when slowed).
    pub delay_ms: u32,
    pub treatment: Treatment,
    /// The check of the frames as drawn.
    pub before: Check,
    /// The check of the file as saved.
    pub after: Check,
}

impl Guarded {
    /// A sentence for a report, when the guard did something or found something.
    pub fn note(&self) -> Option<String> {
        let found = self.before.words()?;
        Some(match self.treatment {
            Treatment::AsIs => {
                format!("the animation {found} (saved as it is, with the strobe guard off)")
            }
            Treatment::Softened => {
                format!("the animation {found}: the strobe guard saved a softened version")
            }
            Treatment::Slowed => format!(
                "the animation {found}: the strobe guard saved it slowed, {} ms a frame",
                self.delay_ms
            ),
        })
    }

    /// The record kept with the media's metadata.
    pub fn record(&self) -> serde_json::Value {
        serde_json::json!({
            "treatment": self.treatment.name(),
            "found": self.before.words(),
            "delay_ms": self.delay_ms,
            "after": {
                "transitions": self.after.flash.most(),
                "stripes": self.after.stripes.share,
                "harmful": self.after.harmful,
            },
        })
    }
}

/// The frames looped as a player loops them: their indices, at least twice through and for at
/// least [`CHECK_SECONDS`].
fn looped(n: usize, delay_ms: u32) -> impl Iterator<Item = usize> {
    let per = (delay_ms.max(1) as f64 / 1000.0) * n as f64;
    let loops = ((CHECK_SECONDS / per.max(1e-9)).ceil() as usize).max(2);
    let total = (n * loops).min(MAX_CHECKED.max(n + 1));
    (0..total).map(move |k| k % n.max(1))
}

/// The meters for frames of this size, with the at-risk stripe rule or not.
fn meters(w: u32, h: u32, at_risk: bool) -> (FlashMeter, PatternMeter) {
    let v = Viewing::animation(w, h);
    let mut p = PatternMeter::for_viewing(v, None);
    p.set_at_risk(at_risk);
    (FlashMeter::new(v, None), p)
}

fn worse(a: &FlashReading, b: &FlashReading) -> bool {
    (b.worst(), b.most()) > (a.worst(), a.most())
}

/// Checks `frames`, each shown `delay_ms`, looping; with the at-risk stripe rule if `at_risk`.
pub fn check(frames: &[&Image], delay_ms: u32, at_risk: bool) -> Check {
    let Some(first) = frames.first() else {
        return Check::default();
    };
    let (w, h) = (first.width, first.height);
    let (mut flash, mut stripes) = meters(w, h, at_risk);
    let mut out = Check::default();
    let dt = delay_ms.max(1) as f64 / 1000.0;
    let mut last_pattern = f64::NEG_INFINITY;
    for (k, i) in looped(frames.len(), delay_ms).enumerate() {
        let t = k as f64 * dt;
        let (map, half) = reduce_both(&frames[i].rgba, w, h);
        let r = flash.update(&map, t);
        if worse(&out.flash, &r) {
            out.flash = r;
        }
        if t - last_pattern >= PATTERN_EVERY - 1e-9 {
            last_pattern = t;
            let p = stripes.update(&half, t);
            if p.level > out.stripes.level
                || (p.level == out.stripes.level && p.share > out.stripes.share)
            {
                out.stripes = p;
            }
        }
    }
    out.harmful = out.flash.worst().fails() || out.stripes.level == PatternLevel::Fail;
    out
}

/// The guard's softening carried out on `frames` (see the module's notes); stripes veiled by the
/// at-risk rule as well if `at_risk`.
pub fn soften(frames: &[Image], delay_ms: u32, at_risk: bool) -> Vec<Image> {
    let Some(first) = frames.first() else {
        return vec![];
    };
    let (w, h) = (first.width, first.height);
    // Where it flashes and where its stripes are, over the whole loop.
    let (mut flash, mut stripes) = meters(w, h, at_risk);
    let mut plan = Plan::none_for(w, h);
    let dt = delay_ms.max(1) as f64 / 1000.0;
    let mut last_pattern = f64::NEG_INFINITY;
    for (k, i) in looped(frames.len(), delay_ms).enumerate() {
        let t = k as f64 * dt;
        let (map, half) = reduce_both(&frames[i].rgba, w, h);
        flash.update(&map, t);
        let (cols, rows) = flash.tiles();
        for (j, (&c, &r)) in flash
            .tile_counts()
            .iter()
            .zip(flash.tile_red_counts())
            .enumerate()
        {
            if c >= 2 || r >= 2 {
                let (x, y) = ((j as u32 % cols) as i64, (j as u32 / cols) as i64);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (xx, yy) = (x + dx, y + dy);
                        if xx >= 0 && yy >= 0 && (xx as u32) < cols && (yy as u32) < rows {
                            plan.soften[(yy as u32 * cols + xx as u32) as usize] = true;
                        }
                    }
                }
            }
        }
        if t - last_pattern >= PATTERN_EVERY - 1e-9 {
            last_pattern = t;
            if stripes.update(&half, t).level >= PatternLevel::Harmful {
                for (j, f) in stripes.found().tiles.iter().enumerate() {
                    if let Some((contrast, mean)) = f {
                        let v = veil_for(*contrast, *mean);
                        if v[0] > plan.veil[j][0] {
                            plan.veil[j] = v;
                        }
                    }
                }
            }
        }
    }
    if !plan.active() {
        return frames.to_vec();
    }
    // Twice round the loop; the second pass is kept, so the end flows into the start.
    let mut soft = Softener::new();
    let n = frames.len();
    let mut out = Vec::with_capacity(n);
    for pass in 0..2 {
        for f in frames {
            let s = soft.apply(&f.rgba, w, h, &plan, delay_ms as f32 / 1000.0);
            if pass == 1 {
                out.push(Image {
                    width: w,
                    height: h,
                    rgba: s.shown.unwrap_or_else(|| f.rgba.clone()),
                });
            }
        }
    }
    out
}

/// The delay that slows `frames` to at most one flash a second, as `check` found them.
pub fn slowed_delay(c: &Check, delay_ms: u32) -> u32 {
    let t = c.flash.most().max(2) as f64;
    // Transitions a second now, if every one shows; aim for two (one flash).
    let factor = if c.flash.most() >= super::flash::COUNT_CAP {
        // The count stops at its cap: assume a transition every frame.
        (1000.0 / delay_ms.max(1) as f64) / 2.0
    } else {
        t / 2.0
    };
    ((delay_ms as f64 * factor.max(1.0)).ceil() as u32).min(SLOWEST_MS)
}

/// Saves `frames` (each shown `delay_ms`) through the strobe guard: checked, and if harmful and
/// `on`, softened; encoded with `encode`, decoded with `decode` and checked again; slowed if the
/// softened file still fails. With `on` false a harmful animation is saved as it is, and the check
/// says so. `at_risk`: by the stricter stripe rule, for someone who answered the warning at risk.
pub fn guard(
    frames: &[Image],
    delay_ms: u32,
    on: bool,
    at_risk: bool,
    encode: &dyn Fn(&[&Image], u32) -> Result<Vec<u8>, String>,
    decode: &dyn Fn(&[u8]) -> Result<Vec<Image>, String>,
) -> Result<Guarded, String> {
    let refs: Vec<&Image> = frames.iter().collect();
    let before = check(&refs, delay_ms, at_risk);
    if !before.harmful || !on {
        return Ok(Guarded {
            bytes: encode(&refs, delay_ms)?,
            delay_ms,
            treatment: Treatment::AsIs,
            before,
            after: before,
        });
    }
    // Slower, when a little slower is enough and there are no strong stripes: every frame stays
    // as drawn (an oscillator's few phases would blur into one if softened).
    if before.stripes.level != PatternLevel::Fail {
        let d = slowed_delay(&before, delay_ms);
        if d <= delay_ms.saturating_mul(MAX_SLOWER) {
            let bytes = encode(&refs, d)?;
            let saved = decode(&bytes)?;
            let after = check(&saved.iter().collect::<Vec<_>>(), d, at_risk);
            if !after.harmful {
                return Ok(Guarded {
                    bytes,
                    delay_ms: d,
                    treatment: Treatment::Slowed,
                    before,
                    after,
                });
            }
        }
    }
    let soft = soften(frames, delay_ms, at_risk);
    let sref: Vec<&Image> = soft.iter().collect();
    let bytes = encode(&sref, delay_ms)?;
    let saved = decode(&bytes)?;
    let after = check(&saved.iter().collect::<Vec<_>>(), delay_ms, at_risk);
    if !after.harmful {
        return Ok(Guarded {
            bytes,
            delay_ms,
            treatment: Treatment::Softened,
            before,
            after,
        });
    }
    // Slowed: the softened frames shown longer (stripes stay veiled), until they pass.
    let mut d = slowed_delay(&after, delay_ms).max(delay_ms * 2);
    loop {
        let bytes = encode(&sref, d)?;
        let saved = decode(&bytes)?;
        let after = check(&saved.iter().collect::<Vec<_>>(), d, at_risk);
        if !after.harmful || d >= SLOWEST_MS {
            return Ok(Guarded {
                bytes,
                delay_ms: d,
                treatment: Treatment::Slowed,
                before,
                after,
            });
        }
        d = (d * 2).min(SLOWEST_MS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::encode;

    fn solid(w: u32, h: u32, c: [u8; 3]) -> Image {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
        Image {
            width: w,
            height: h,
            rgba,
        }
    }

    fn stripes(w: u32, h: u32, shift: u32) -> Image {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..h {
            for x in 0..w {
                let c = if ((x + shift) / 6) % 2 == 0 { 0 } else { 255 };
                rgba.extend_from_slice(&[c, c, c, 255]);
            }
        }
        Image {
            width: w,
            height: h,
            rgba,
        }
    }

    fn gif(f: &[&Image], d: u32) -> Result<Vec<u8>, String> {
        encode::gif(f, d)
    }

    fn webp(f: &[&Image], d: u32) -> Result<Vec<u8>, String> {
        encode::webp(f, d, [0, 0, 0, 255])
    }

    /// A black-white flip at 20 frames a second fails; at 2 a second it passes.
    #[test]
    fn an_animation_is_checked_at_its_own_rate_looping() {
        let (b, w) = (solid(200, 150, [0; 3]), solid(200, 150, [255; 3]));
        let c = check(&[&b, &w], 50, false);
        assert!(c.harmful, "{c:?}");
        assert_eq!(c.words().unwrap(), "flashes 4 or more times a second");
        let c = check(&[&b, &w], 500, false);
        assert!(!c.harmful, "{c:?}");
        // A loop of black, then many frames of white: one flash a loop, at the seam.
        let mut frames = vec![&b];
        frames.extend(std::iter::repeat_n(&w, 9));
        let c = check(&frames, 20, false);
        assert!(c.harmful, "five loops a second flash at the seam: {c:?}");
        assert!(!check(&frames, 100, false).harmful, "one loop a second");
        // A small animation flashing all over: its frame's mean flashes.
        let (sb, sw) = (solid(48, 48, [0; 3]), solid(48, 48, [255; 3]));
        let c = check(&[&sb, &sw], 50, false);
        assert!(c.harmful && c.flash.screen_transitions > 6, "{c:?}");
    }

    #[test]
    fn softened_and_saved_the_flip_passes_in_both_formats() {
        let (b, w) = (solid(160, 120, [0; 3]), solid(160, 120, [255; 3]));
        let frames: Vec<Image> = (0..40)
            .map(|k| if k % 2 == 0 { b.clone() } else { w.clone() })
            .collect();
        for (name, enc, dec) in [
            (
                "gif",
                gif as fn(&[&Image], u32) -> Result<Vec<u8>, String>,
                encode::decode_gif as fn(&[u8]) -> Result<Vec<Image>, String>,
            ),
            ("webp", webp, encode::decode_webp),
        ] {
            let g = guard(&frames, 50, true, false, &enc, &dec).unwrap();
            assert!(g.before.harmful);
            assert!(!g.after.harmful, "{name}: {:?}", g.after);
            assert_ne!(g.treatment, Treatment::AsIs, "{name}");
            assert!(g.note().unwrap().contains("strobe guard saved"), "{name}");
            let rec = g.record();
            assert_eq!(rec["found"], "flashes 4 or more times a second");
            // With the guard off: as it is, and said so.
            let g = guard(&frames, 50, false, false, &enc, &dec).unwrap();
            assert_eq!(g.treatment, Treatment::AsIs);
            assert!(g.after.harmful);
            assert!(g.note().unwrap().contains("strobe guard off"));
        }
    }

    /// A still soup of square cells: no stripes, so saved as it is for anyone; for someone at
    /// risk a pattern of similar parts, saved softened, and the file passes.
    #[test]
    fn an_export_for_someone_at_risk_is_softened_for_similar_parts() {
        let mut rng = crate::rng::Rng::new(11);
        let cells: Vec<bool> = (0..21 * 16).map(|_| rng.next_f64() < 0.3).collect();
        let mut img = solid(160, 120, [20; 3]);
        for y in 0..120u32 {
            for x in 0..160u32 {
                if cells[(y / 8 * 21 + x / 8) as usize] {
                    let i = ((y * 160 + x) * 4) as usize;
                    img.rgba[i..i + 3].copy_from_slice(&[240; 3]);
                }
            }
        }
        let frames = vec![img; 10];
        let g = guard(&frames, 100, true, false, &webp, &encode::decode_webp).unwrap();
        assert_eq!(g.treatment, Treatment::AsIs);
        let g = guard(&frames, 100, true, true, &webp, &encode::decode_webp).unwrap();
        assert!(
            g.before.harmful && g.before.stripes.similar,
            "{:?}",
            g.before
        );
        assert_eq!(g.treatment, Treatment::Softened);
        assert!(!g.after.harmful, "{:?}", g.after);
    }

    /// Three phases at 10 frames a second, each changing much of the frame: shown three or four
    /// times longer, they pass, so they are slowed, every frame as drawn.
    #[test]
    fn a_few_phases_are_slowed_not_blurred() {
        let phase = |k: u32| {
            let mut img = solid(160, 120, [0; 3]);
            for y in 0..120u32 {
                for x in 0..160u32 {
                    if (x / 40 + k) % 3 == 0 {
                        let i = ((y * 160 + x) * 4) as usize;
                        img.rgba[i..i + 3].copy_from_slice(&[255; 3]);
                    }
                }
            }
            img
        };
        let frames: Vec<Image> = (0..3).map(phase).collect();
        let g = guard(&frames, 100, true, false, &gif, &encode::decode_gif).unwrap();
        assert!(g.before.harmful, "{:?}", g.before);
        assert_eq!(g.treatment, Treatment::Slowed, "{:?}", g.before);
        assert!(
            g.delay_ms > 100 && g.delay_ms <= 100 * MAX_SLOWER,
            "{}",
            g.delay_ms
        );
        let back = encode::decode_gif(&g.bytes).unwrap();
        assert_eq!(back[1].rgba, frames[1].rgba, "frames as drawn");
        assert!(g.note().unwrap().contains("slowed"));
    }

    #[test]
    fn harmless_animations_are_saved_untouched() {
        let frames: Vec<Image> = (0..20u8).map(|k| solid(64, 64, [k * 10, 40, 80])).collect();
        let g = guard(&frames, 50, true, false, &gif, &encode::decode_gif).unwrap();
        assert_eq!(g.treatment, Treatment::AsIs);
        assert_eq!(g.note(), None);
        let refs: Vec<&Image> = frames.iter().collect();
        assert_eq!(g.bytes, encode::gif(&refs, 50).unwrap());
    }

    #[test]
    fn moving_stripes_are_veiled_in_the_softened_version() {
        let frames: Vec<Image> = (0..24).map(|k| stripes(320, 240, k)).collect();
        let refs: Vec<&Image> = frames.iter().collect();
        let c = check(&refs, 50, false);
        assert!(
            c.harmful && c.words().unwrap().contains("strong stripes"),
            "{c:?}"
        );
        let g = guard(&frames, 50, true, false, &webp, &encode::decode_webp).unwrap();
        assert!(!g.after.harmful, "{:?}", g.after);
        assert_eq!(g.treatment, Treatment::Softened);
    }

    #[test]
    fn the_slowed_delay_brings_a_flip_to_one_flash_a_second() {
        let (b, w) = (solid(100, 100, [0; 3]), solid(100, 100, [255; 3]));
        let c = check(&[&b, &w], 50, false);
        let d = slowed_delay(&c, 50);
        assert!(d >= 500 && d <= SLOWEST_MS, "{d}");
        assert!(!check(&[&b, &w], d, false).harmful);
    }
}

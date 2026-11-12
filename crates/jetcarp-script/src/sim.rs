//! A world as scripts see it, running until a trigger, and screen-sized images.

use jetcarp_api::args::{self, Screen, Until, get, get_bool, get_f64, get_u64};
use jetcarp_api::out;
use jetcarp_core::render::{self, Image, RenderOptions};
use jetcarp_core::rules::Rule;
use jetcarp_core::script::{self, Metrics, ScriptState, Trigger};
use jetcarp_core::world::CaChange;
use jetcarp_core::*;
use serde_json::Value;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

/// A world plus the screen it was made for (default image size).
pub struct Sim {
    pub world: World,
    pub screen: Screen,
}

/// Shared handle: script variables holding the same world see the same state.
#[derive(Clone)]
pub struct Shared(pub Arc<Mutex<Sim>>);

impl Shared {
    pub fn new(world: World, screen: Screen) -> Shared {
        Shared(Arc::new(Mutex::new(Sim { world, screen })))
    }
    pub fn lock(&self) -> MutexGuard<'_, Sim> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// How a run ended.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct RunOutcome {
    /// Generations run by this call.
    pub generations: u64,
    /// `until` (the trigger fired), `max_gens` (the limit was reached first, or no `until` was
    /// given), or `cancelled`.
    pub stopped: &'static str,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done_reason: Option<String>,
}

/// Run until the trigger fires or `max_gens` generations have run. `stop` is polled every few
/// generations (cancellation and time limits).
pub fn run_until(w: &mut World, u: &Until, stop: &dyn Fn() -> bool) -> RunOutcome {
    let start = w.generation;
    let was_tracking = w.track_changes();
    // Rows rules know when they are done without counting changes.
    let rows_only = w.cas.iter().filter(|c| c.enabled).all(|c| {
        matches!(
            c.rule,
            Rule::Wolfram(_) | Rule::HexWolfram { .. } | Rule::HexTri { .. }
        )
    });
    let needs_changes = match &u.trigger {
        Trigger::Done => !rows_only,
        t => t.needs_activity(),
    };
    if needs_changes {
        w.set_track_changes(true);
    }
    let mut st = ScriptState {
        started: start,
        ..Default::default()
    };
    let check = |w: &World, st: &mut ScriptState| -> bool {
        let mut fields: Vec<usize> = w
            .cas
            .iter()
            .filter(|c| c.enabled)
            .map(|c| c.field)
            .collect();
        fields.sort_unstable();
        fields.dedup();
        let population = fields.iter().map(|&f| w.fields[f].population()).sum();
        let activity = w
            .last_changed()
            .map(|c| c as f64 / (w.topo.len() * fields.len()).max(1) as f64);
        let period = match w.status() {
            Status::Repeats { period, .. } => Some(period),
            _ => None,
        };
        let done = matches!(u.trigger, Trigger::Done) && w.done();
        script::fires(
            &u.trigger,
            st,
            &Metrics {
                generation: w.generation,
                population,
                activity,
                period,
                done,
            },
        )
    };
    let mut stopped = "max_gens";
    let pure_count = matches!(u.trigger, Trigger::Generations { .. } | Trigger::Never);
    if !u.trigger.needs_activity() && !pure_count && check(w, &mut st) {
        stopped = "until";
    } else if pure_count {
        let n = match u.trigger {
            Trigger::Generations { n } => n.min(u.max_gens),
            _ => u.max_gens,
        };
        let mut left = n;
        while left > 0 {
            if stop() {
                stopped = "cancelled";
                break;
            }
            let k = left.min(256);
            let _ = w.apply(Command::Step { gens: k });
            left -= k;
        }
        if stopped != "cancelled"
            && u.given
            && matches!(u.trigger, Trigger::Generations { n } if n <= u.max_gens)
        {
            stopped = "until";
        }
    } else {
        while w.generation - start < u.max_gens {
            if (w.generation - start).is_multiple_of(64) && stop() {
                stopped = "cancelled";
                break;
            }
            let _ = w.apply(Command::Step { gens: 1 });
            if check(w, &mut st) {
                stopped = "until";
                break;
            }
        }
    }
    let reason = w.done_reason();
    let out = RunOutcome {
        generations: w.generation - start,
        stopped,
        done: reason.is_some(),
        done_reason: reason,
    };
    w.set_track_changes(was_tracking);
    out
}

/// How to draw an image.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ImageSpec {
    /// Exact pixel size: the world is fitted inside and centred (default: the screen).
    pub canvas: Option<(u32, u32)>,
    /// Pixels per world unit (instead of a canvas; image size follows the tiling).
    pub scale: Option<f64>,
    /// Longest side when neither canvas nor scale is given.
    pub max_side: u32,
    /// Draw tile outlines when tiles are at least this many pixels across (0 = never).
    pub outline_min_px: f64,
    /// Set every CA's palette before drawing.
    pub palette: Option<String>,
    /// PNG compression: `fast` (about 2x larger files, 8x faster to encode), `balanced`
    /// (default), or `high`.
    pub compression: String,
}

pub const IMAGE_KEYS: &[&str] = &[
    "size",
    "scale",
    "max_side",
    "outlines",
    "palette",
    "compression",
];

/// `size` (`"screen"` default, `[w, h]`, `"WxH"`, `"fit"`), `scale`, `max_side`, `outlines`
/// (true, false, or a minimum tile size in pixels), `palette`.
pub fn image_spec(v: &Value, screen: &Screen) -> Result<ImageSpec, String> {
    args::check_keys(v, IMAGE_KEYS, "image")?;
    let scale = get_f64(v, "scale")?;
    let max_side = get_u64(v, "max_side")?;
    let canvas = match get(v, "size") {
        None if scale.is_some() || max_side.is_some() => None,
        None => Some((screen.width, screen.height)),
        Some(Value::String(s)) if s.eq_ignore_ascii_case("screen") => {
            Some((screen.width, screen.height))
        }
        Some(Value::String(s)) if s.eq_ignore_ascii_case("fit") => None,
        Some(Value::String(s)) => Some(Screen::parse_size(s)?),
        Some(Value::Array(a)) if a.len() == 2 => Some((
            a[0].as_u64()
                .ok_or("image size [w, h] needs whole numbers")? as u32,
            a[1].as_u64()
                .ok_or("image size [w, h] needs whole numbers")? as u32,
        )),
        Some(x) => {
            return Err(format!(
                "image size should be \"screen\", [w, h], or \"WxH\"; got {x}"
            ));
        }
    };
    if let Some((w, h)) = canvas
        && (w == 0 || h == 0 || w as u64 * h as u64 > 400_000_000)
    {
        return Err(format!("image size {w}x{h} out of range"));
    }
    let outline_min_px = match get(v, "outlines") {
        None => 16.0,
        Some(Value::Bool(_)) => {
            if get_bool(v, "outlines")?.unwrap() {
                3.0
            } else {
                0.0
            }
        }
        Some(_) => get_f64(v, "outlines")?.unwrap_or(16.0),
    };
    Ok(ImageSpec {
        canvas,
        scale,
        max_side: max_side.unwrap_or(1600).clamp(16, 65536) as u32,
        outline_min_px,
        palette: args::get_str(v, "palette")?.map(String::from),
        compression: match args::get_str(v, "compression")?.unwrap_or("balanced") {
            c @ ("fast" | "balanced" | "high") => c.to_string(),
            c => {
                return Err(format!(
                    "compression must be fast, balanced, or high, got '{c}'"
                ));
            }
        },
    })
}

/// Apply an image spec's palette to every CA (a logged command; palettes do not affect hashes).
pub fn apply_palette(w: &mut World, spec: &ImageSpec) -> Result<(), String> {
    if let Some(p) = &spec.palette {
        let ids: Vec<u32> = w
            .cas
            .iter()
            .filter(|c| &c.palette != p)
            .map(|c| c.id)
            .collect();
        for id in ids {
            w.apply(Command::UpdateCa {
                id,
                change: CaChange {
                    palette: Some(p.clone()),
                    ..Default::default()
                },
            })?;
        }
    }
    Ok(())
}

/// Draw the world.
pub fn render_image(w: &World, spec: &ImageSpec) -> Image {
    let Some((cw, ch)) = spec.canvas else {
        return render::render(
            w,
            &RenderOptions {
                scale: spec.scale,
                max_side: spec.max_side,
                outline_min_px: spec.outline_min_px,
            },
        );
    };
    render::render_fitted(
        w,
        cw,
        ch,
        &RenderOptions {
            scale: None,
            max_side: cw.max(ch),
            outline_min_px: spec.outline_min_px,
        },
    )
}

/// Render and write a PNG with JETCARP's metadata chunks; returns (width, height, bytes).
pub fn save_png(w: &World, spec: &ImageSpec, path: &Path) -> Result<(u32, u32, usize), String> {
    let img = render_image(w, spec);
    let level = match spec.compression.as_str() {
        "fast" => png::Compression::Fast,
        "high" => png::Compression::High,
        _ => png::Compression::Balanced,
    };
    let bytes = out::encode_png(&img, &render::png_metadata(w), level)?;
    out::write_bytes(path, &bytes)?;
    Ok((img.width, img.height, bytes.len()))
}

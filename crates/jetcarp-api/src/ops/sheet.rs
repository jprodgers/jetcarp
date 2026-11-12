//! `sheet.*`: contact sheets as PNGs: one thumbnail per elementary rule (`sheet.rules`), chosen
//! runs of a sweep folder (`sheet.sweep`), and Kaplan's 81 isohedral types (`sheet.isohedral`).
//! The Quick Start's own sheet is part of `quickstart.check`.

use crate::ctx::Ctx;
use crate::error::{ApiError, ErrorKind, ResultExt};
use crate::op::{Op, Progress, Report};
use crate::sweeps::{self, Manifest};
use jetcarp_core::VERSION;
use jetcarp_core::render::{Image, write_png};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A sheet written.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Sheet {
    /// The PNG, as given.
    pub png: String,
    pub width: u32,
    pub height: u32,
}

impl Report for Sheet {
    fn text(&self) -> String {
        format!("sheet: {} ({}x{})\n", self.png, self.width, self.height)
    }
}

fn write(cx: &Ctx, png: String, img: &Image, meta: &[(&str, String)]) -> Result<Sheet, ApiError> {
    let file = cx.fs.writable(&png)?;
    write_png(&file, img, meta).map_err(ApiError::io)?;
    Ok(Sheet {
        png,
        width: img.width,
        height: img.height,
    })
}

// ---------------------------------------------------------------------------------------------
// sheet.rules

fn kind_default() -> String {
    "w".into()
}
fn count_default() -> u32 {
    64
}
fn cols_default() -> u32 {
    8
}
fn width_default() -> u32 {
    81
}
fn thumb_default() -> u32 {
    180
}
fn palette_default() -> String {
    "Ember".into()
}

/// Which elementary rules, and how to draw them.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RulesIn {
    /// w (squares, 0-255, the default), hexw (binary hexagon, 0-15), hexw3 (ternary hexagon,
    /// 0-19682), hext (flat-top hexagons, three parents, 0-255), hext3 (0 to 3^27 - 1), or
    /// hext3sum (totalistic ternary three-parent rules, 0-2186).
    #[serde(default = "kind_default")]
    pub kind: String,
    /// The first rule number (default 0).
    #[serde(default)]
    pub from: u64,
    /// Rules shown (default 64).
    #[serde(default = "count_default")]
    pub count: u32,
    /// Thumbnails per row (default 8).
    #[serde(default = "cols_default")]
    pub cols: u32,
    /// Cells across each thumbnail (default 81).
    #[serde(default = "width_default")]
    pub width: u32,
    /// Thumbnail size in pixels (default 180).
    #[serde(default = "thumb_default")]
    pub thumb: u32,
    /// Start from a random top row with this density instead of one cell.
    #[serde(default)]
    pub random_row: Option<f64>,
    /// A built-in palette or hex colours like `#103050-#4fc3ff` (default Ember).
    #[serde(default = "palette_default")]
    pub palette: String,
    /// The PNG to write.
    pub png: String,
}

/// `sheet.rules`.
pub struct SheetRules;
impl Op for SheetRules {
    const NAME: &'static str = "sheet.rules";
    const SUMMARY: &'static str = "Contact sheet of elementary rules (W, HexW, HexW3, HexT, ...): one labelled thumbnail per rule.";
    const READ_ONLY: bool = false;
    type In = RulesIn;
    type Out = Sheet;
    fn run(cx: &Ctx, a: RulesIn, _: &dyn Progress) -> Result<Sheet, ApiError> {
        let kind = jetcarp_core::sheet::SheetKind::parse(&a.kind)
            .ok_or_else(|| ApiError::invalid("kind is w, hexw, hexw3, hext, hext3, or hext3sum"))?;
        let o = jetcarp_core::sheet::SheetOptions {
            kind,
            first: a.from,
            count: a.count,
            cols: a.cols,
            width: a.width,
            thumb_px: a.thumb,
            random_row: a.random_row,
            rng_state: 1,
            palette: a.palette,
        };
        let img = jetcarp_core::sheet::contact_sheet(&o).kind(ErrorKind::Invalid)?;
        let meta = vec![
            ("jetcarp:version", VERSION.to_string()),
            (
                "jetcarp:sheet",
                format!(
                    "{kind:?} {}+{} width {} random_row {:?}",
                    a.from, a.count, a.width, a.random_row
                ),
            ),
        ];
        write(cx, a.png, &img, &meta)
    }
}

// ---------------------------------------------------------------------------------------------
// sheet.sweep

fn sweep_thumb_default() -> u32 {
    180
}
fn scale_default() -> u32 {
    2
}

/// Which runs of a sweep folder, and how to lay them out.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SweepIn {
    /// The sweep folder (with manifest.json and run.json).
    pub folder: String,
    /// The runs: rule text (`HexT3 sum 588`), manifest index (`#12` or `12`), or series number
    /// (`n=588`).
    pub runs: Vec<String>,
    /// A label per run (default `#index`); the rule is printed under it.
    #[serde(default)]
    pub labels: Vec<String>,
    /// Thumbnail size on the long side (default 180).
    #[serde(default = "sweep_thumb_default")]
    pub thumb: u32,
    /// Thumbnails per row (default 8).
    #[serde(default = "cols_default")]
    pub cols: u32,
    /// Largest label scale: the 3x5 font at this many pixels per dot (default 2).
    #[serde(default = "scale_default")]
    pub label_scale: u32,
    /// The PNG to write.
    pub png: String,
}

/// `sheet.sweep`.
pub struct SheetSweep;
impl Op for SheetSweep {
    const NAME: &'static str = "sheet.sweep";
    const SUMMARY: &'static str =
        "Contact sheet of chosen runs of a sweep folder, each under its label and rule.";
    const READ_ONLY: bool = false;
    type In = SweepIn;
    type Out = Sheet;
    fn run(cx: &Ctx, a: SweepIn, _: &dyn Progress) -> Result<Sheet, ApiError> {
        let dir = cx.fs.readable(&a.folder)?;
        let m = Manifest::load(&dir).kind(ErrorKind::NotFound)?;
        if a.runs.is_empty() {
            return Err(ApiError::invalid(
                "give the runs to show (rule, #index, or n=number)",
            ));
        }
        let mut items = vec![];
        for (k, key) in a.runs.iter().enumerate() {
            let row = m
                .find(key)
                .ok_or_else(|| ApiError::not_found(format!("no run '{key}' in {}", a.folder)))?;
            let r = &m.rows[row];
            let label = a
                .labels
                .get(k)
                .cloned()
                .unwrap_or_else(|| format!("#{}", r["index"]));
            items.push((row, vec![label, sweeps::text(r, "rule").into()]));
        }
        let img = sweeps::contact_sheet(&m, &items, a.thumb, a.cols, a.label_scale)
            .kind(ErrorKind::Io)?;
        let meta = vec![
            ("jetcarp:version", VERSION.to_string()),
            (
                "jetcarp:sheet",
                format!("{} runs {}", m.name(), a.runs.join(",")),
            ),
        ];
        write(cx, a.png, &img, &meta)
    }
}

// ---------------------------------------------------------------------------------------------
// sheet.isohedral

fn iso_cols_default() -> u32 {
    9
}
fn iso_thumb_default() -> u32 {
    200
}
fn seed_default() -> u64 {
    7
}

/// How to draw the isohedral types.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IsohedralIn {
    /// Keep edges straight (default: random edge shapes within each type's constraints).
    #[serde(default)]
    pub straight: bool,
    /// Thumbnails per row (default 9).
    #[serde(default = "iso_cols_default")]
    pub cols: u32,
    /// Thumbnail size in pixels (default 200).
    #[serde(default = "iso_thumb_default")]
    pub thumb: u32,
    /// RNG state for the edge shapes (default 7).
    #[serde(
        default = "seed_default",
        serialize_with = "jetcarp_core::json_int::uint::serialize",
        deserialize_with = "jetcarp_core::json_int::uint::deserialize"
    )]
    pub seed: u64,
    /// The PNG to write.
    pub png: String,
}

/// `sheet.isohedral`.
pub struct SheetIsohedral;
impl Op for SheetIsohedral {
    const NAME: &'static str = "sheet.isohedral";
    const SUMMARY: &'static str = "Contact sheet of all 81 of Kaplan's isohedral tiling types.";
    const READ_ONLY: bool = false;
    type In = IsohedralIn;
    type Out = Sheet;
    fn run(cx: &Ctx, a: IsohedralIn, _: &dyn Progress) -> Result<Sheet, ApiError> {
        let o = jetcarp_core::isohedral::sheet::IhSheetOptions {
            cols: a.cols,
            thumb_px: a.thumb,
            curved: !a.straight,
            rng_state: a.seed,
            ..Default::default()
        };
        let img = jetcarp_core::isohedral::sheet::contact_sheet(&o).kind(ErrorKind::Engine)?;
        let meta = vec![
            ("jetcarp:version", VERSION.to_string()),
            (
                "jetcarp:sheet",
                format!("isohedral curved {} seed {}", !a.straight, a.seed),
            ),
        ];
        write(cx, a.png, &img, &meta)
    }
}

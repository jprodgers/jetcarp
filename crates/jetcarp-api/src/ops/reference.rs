//! `reference.*`: what exists to choose from: named rules, tilings, Kaplan's isohedral types,
//! palettes, and seed forms.

use crate::args::rule::squash;
use crate::ctx::Ctx;
use crate::error::ApiError;
use crate::op::{NoInput, Op, Progress, Report};
use jetcarp_core::{TilingSpec, Topology, catalog, families, rules};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

// ---------------------------------------------------------------------------------------------
// Rules

/// Which rules.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RulesIn {
    /// Only rules of this family (part of its name, any case, ignoring spaces and punctuation),
    /// e.g. "Life-like", "Generations", "Elementary", "Agents". Default: every rule.
    #[serde(default)]
    pub family: Option<String>,
}

/// A named rule.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RuleInfo {
    pub name: String,
    /// The rule in JETCARP's notation ("B3/S23"); the name works as a rule too.
    pub notation: String,
    pub family: String,
    /// One line about it.
    pub blurb: String,
}

/// Named rules.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RulesOut {
    pub rules: Vec<RuleInfo>,
    /// Every family, in catalog order.
    pub families: Vec<String>,
}

/// What the rule list leaves out, as `jetcarp rules` ends.
pub const NOTATION_SUMMARY: &str = "Any B/S rule (B3/S23, add V for edge neighbours), Generations B2/S/C3, W0..W255, HexW 0..15, HexW3 0..19682,\nHexT 0..255 / HexT3 n / HexT3 sum 0..2186 (flat-top hexagons), Margolus <16 block outputs> or Margolus BBM,\nLenia R=13,T=10,b=1,m=0.15,s=0.015, Ant RL, Turmite LLRR. See the README for every family.";

impl Report for RulesOut {
    fn text(&self) -> String {
        let mut s = String::new();
        for e in &self.rules {
            let _ = writeln!(s, "{:30} {:15} {}", e.name, e.notation, e.blurb);
        }
        let _ = writeln!(s, "\n{NOTATION_SUMMARY}");
        s
    }
}

/// `reference.rules`.
pub struct ReferenceRules;
impl Op for ReferenceRules {
    const NAME: &'static str = "reference.rules";
    const SUMMARY: &'static str =
        "Named rules (name, notation, family, one line each), optionally of one family.";
    const READ_ONLY: bool = true;
    type In = RulesIn;
    type Out = RulesOut;
    fn run(_: &Ctx, a: RulesIn, _: &dyn Progress) -> Result<RulesOut, ApiError> {
        let all = rules::catalog();
        let mut families: Vec<String> = Vec::new();
        for e in all {
            if !families.iter().any(|f| f == e.family) {
                families.push(e.family.to_string());
            }
        }
        let fam = a.family.as_deref().map(squash);
        let picked: Vec<RuleInfo> = all
            .iter()
            .filter(|e| {
                fam.as_ref()
                    .is_none_or(|f| squash(e.family).contains(f.as_str()))
            })
            .map(|e| RuleInfo {
                name: e.name.to_string(),
                notation: e.notation.to_string(),
                family: e.family.to_string(),
                blurb: e.blurb.to_string(),
            })
            .collect();
        if picked.is_empty() {
            return Err(ApiError::not_found(format!(
                "no rules in that family. Families: {}",
                families.join(", ")
            )));
        }
        Ok(RulesOut {
            rules: picked,
            families,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Tilings

/// Which tilings.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TilingsIn {
    /// List the tilings (families, catalog tilings, isohedral types) whose id or name contains
    /// this text, any case, e.g. "k2-", "3.4.6.4", "ih4". Default: the tiling families.
    #[serde(default)]
    pub search: Option<String>,
    /// With `search`: most matches listed (default 60).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// A tiling family, with its neighbour counts at the default size.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct FamilyInfo {
    /// The id a world's `tiling` takes ("square", "hex", "spectre").
    pub id: String,
    pub name: String,
    pub group: String,
    /// Symmetry group.
    pub sym: String,
    pub note: String,
    /// How the engine stores its cells.
    pub storage: Option<String>,
    /// Neighbours per cell sharing an edge (the distinct counts).
    pub edge: Vec<usize>,
    /// Neighbours per cell sharing an edge or a corner.
    pub touch: Vec<usize>,
    /// Why the family did not build, if it did not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The tiling catalog beyond the families.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct CatalogSummary {
    /// k-uniform tilings k1-001 .. k6-673.
    pub k_uniform: usize,
    /// Other catalog tilings.
    pub other: usize,
    /// Kaplan's isohedral tile types ih01 .. ih93.
    pub isohedral: usize,
}

/// A tiling that matched a search.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TilingMatch {
    pub id: String,
    pub name: String,
    pub group: String,
}

/// Tilings.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct TilingsOut {
    /// The families (without `search`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub families: Vec<FamilyInfo>,
    pub catalog: CatalogSummary,
    /// The search text (lowercased).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    /// The first matches (with `search`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matches: Option<Vec<TilingMatch>>,
    /// How many tilings matched in all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_matches: Option<usize>,
}

impl TilingsOut {
    /// Search results as a list, as the MCP `list_tilings` tool shows them.
    pub fn matches_text(&self) -> String {
        let (Some(q), Some(m), Some(n)) = (&self.search, &self.matches, self.total_matches) else {
            return String::new();
        };
        let mut s = format!("{n} tilings match \"{q}\"");
        if n > m.len() {
            let _ = write!(
                s,
                " (first {} shown; search for something narrower)",
                m.len()
            );
        }
        s.push_str(":\n");
        for t in m {
            let _ = writeln!(s, "- {} | {} | {}", t.id, t.name, t.group);
        }
        s
    }
}

impl Report for TilingsOut {
    fn text(&self) -> String {
        if self.search.is_some() {
            return self.matches_text();
        }
        let mut s = String::new();
        for f in &self.families {
            match &f.error {
                None => {
                    let _ = writeln!(
                        s,
                        "{:9} {:28} {:5} storage {:9} edge {:?} touch {:?}",
                        f.id,
                        f.name,
                        f.sym,
                        f.storage.as_deref().unwrap_or(""),
                        f.edge,
                        f.touch
                    );
                }
                Some(e) => {
                    let _ = writeln!(s, "{:9} error: {e}", f.id);
                }
            }
        }
        s
    }
}

fn catalog_summary() -> CatalogSummary {
    let extra: Vec<_> = catalog::all()
        .iter()
        .filter(|t| families::family(&t.id).is_none())
        .collect();
    let k_uniform = extra
        .iter()
        .filter(|t| t.k > 0 && t.id.starts_with('k'))
        .count();
    CatalogSummary {
        k_uniform,
        other: extra.len() - k_uniform,
        isohedral: jetcarp_core::isohedral::IH_FAMILIES.len(),
    }
}

/// `reference.tilings`.
pub struct ReferenceTilings;
impl Op for ReferenceTilings {
    const NAME: &'static str = "reference.tilings";
    const SUMMARY: &'static str = "Tiling families with their neighbour counts, or the tilings (catalog and isohedral too) whose id or name contains `search`.";
    const READ_ONLY: bool = true;
    type In = TilingsIn;
    type Out = TilingsOut;
    fn run(_: &Ctx, a: TilingsIn, _: &dyn Progress) -> Result<TilingsOut, ApiError> {
        let catalog = catalog_summary();
        let Some(q) = a.search.map(|q| q.trim().to_ascii_lowercase()) else {
            let families = families::FAMILIES
                .iter()
                .map(|f| {
                    let mut info = FamilyInfo {
                        id: f.id.to_string(),
                        name: f.name.to_string(),
                        group: f.group.to_string(),
                        sym: f.sym.to_string(),
                        note: f.note.to_string(),
                        storage: None,
                        edge: vec![],
                        touch: vec![],
                        error: None,
                    };
                    match Topology::build(&TilingSpec::new(f.id)) {
                        Ok(t) => {
                            let (e, to) = t.degree_sets();
                            info.storage = Some(t.storage_name().to_string());
                            info.edge = e;
                            info.touch = to;
                        }
                        Err(e) => info.error = Some(e),
                    }
                    info
                })
                .collect();
            return Ok(TilingsOut {
                families,
                catalog,
                search: None,
                matches: None,
                total_matches: None,
            });
        };
        let hit = |id: &str, name: &str| {
            id.to_ascii_lowercase().contains(&q) || name.to_ascii_lowercase().contains(&q)
        };
        let mut hits: Vec<TilingMatch> = families::FAMILIES
            .iter()
            .filter(|f| f.id.contains(&q) || f.name.to_ascii_lowercase().contains(&q))
            .map(|f| TilingMatch {
                id: f.id.into(),
                name: f.name.into(),
                group: f.group.into(),
            })
            .collect();
        hits.extend(
            catalog::all()
                .iter()
                .filter(|t| families::family(&t.id).is_none() && hit(&t.id, &t.name))
                .map(|t| TilingMatch {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    group: t.group.clone(),
                }),
        );
        hits.extend(
            jetcarp_core::isohedral::IH_FAMILIES
                .iter()
                .filter(|f| f.id.contains(&q) || f.name.to_ascii_lowercase().contains(&q))
                .map(|f| TilingMatch {
                    id: f.id.into(),
                    name: f.name.into(),
                    group: "isohedral".into(),
                }),
        );
        let total = hits.len();
        hits.truncate(a.limit.unwrap_or(60));
        Ok(TilingsOut {
            families: vec![],
            catalog,
            search: Some(q),
            matches: Some(hits),
            total_matches: Some(total),
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Isohedral types

/// One of Kaplan's isohedral tile types.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct IsohedralType {
    /// The tiling id ("ih01").
    pub id: String,
    /// Symmetry group.
    pub sym: String,
    /// Shape parameters.
    pub params: usize,
    /// Tiles per translation unit.
    pub aspects: usize,
    pub edge: Vec<usize>,
    pub touch: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Kaplan's isohedral types.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct IsohedralOut {
    pub types: Vec<IsohedralType>,
}

impl Report for IsohedralOut {
    fn text(&self) -> String {
        let mut s = String::new();
        for t in &self.types {
            match &t.error {
                None => {
                    let _ = writeln!(
                        s,
                        "{} {:5} params {} aspects {:2} edge {:?} touch {:?}",
                        t.id, t.sym, t.params, t.aspects, t.edge, t.touch
                    );
                }
                Some(e) => {
                    let _ = writeln!(s, "{} error: {e}", t.id);
                }
            }
        }
        s
    }
}

/// `reference.isohedral`.
pub struct ReferenceIsohedral;
impl Op for ReferenceIsohedral {
    const NAME: &'static str = "reference.isohedral";
    const SUMMARY: &'static str =
        "Kaplan's isohedral tile types IH01-IH93 (81 types): symmetry, parameters, neighbours.";
    const READ_ONLY: bool = true;
    type In = NoInput;
    type Out = IsohedralOut;
    fn run(_: &Ctx, _: NoInput, _: &dyn Progress) -> Result<IsohedralOut, ApiError> {
        use jetcarp_core::isohedral::{IH_FAMILIES, IH_TYPES};
        let types = IH_FAMILIES
            .iter()
            .zip(&IH_TYPES)
            .map(|(f, t)| {
                let mut out = IsohedralType {
                    id: f.id.to_string(),
                    sym: f.sym.to_string(),
                    params: t.np,
                    aspects: t.na,
                    edge: vec![],
                    touch: vec![],
                    error: None,
                };
                match Topology::build(&TilingSpec::new(f.id)) {
                    Ok(topo) => {
                        let (e, to) = topo.degree_sets();
                        out.edge = e;
                        out.touch = to;
                    }
                    Err(e) => out.error = Some(e),
                }
                out
            })
            .collect();
        Ok(IsohedralOut { types })
    }
}

// ---------------------------------------------------------------------------------------------
// Palettes

/// A built-in palette.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct PaletteInfo {
    pub name: String,
    /// Colour of empty cells, `#rrggbb`.
    pub background: String,
    /// Colour around the grid.
    pub stage: String,
    /// The colour ramp for live cells, youngest first.
    pub stops: Vec<String>,
}

/// The built-in palettes.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct PalettesOut {
    pub palettes: Vec<PaletteInfo>,
}

impl Report for PalettesOut {
    fn text(&self) -> String {
        let mut s = String::new();
        for p in &self.palettes {
            let _ = writeln!(
                s,
                "{:10} background {} stage {} stops {}",
                p.name,
                p.background,
                p.stage,
                p.stops.join(" ")
            );
        }
        s
    }
}

/// `reference.palettes`.
pub struct ReferencePalettes;
impl Op for ReferencePalettes {
    const NAME: &'static str = "reference.palettes";
    const SUMMARY: &'static str = "The built-in palettes with their background, stage, and ramp colours (Candy, Glacier, Desert, and Paper are light).";
    const READ_ONLY: bool = true;
    type In = NoInput;
    type Out = PalettesOut;
    fn run(_: &Ctx, _: NoInput, _: &dyn Progress) -> Result<PalettesOut, ApiError> {
        use jetcarp_core::render::{Palette, palette_names};
        let hex = |c: [u8; 4]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
        let palettes = palette_names()
            .filter_map(|name| {
                let p = Palette::builtin(name)?;
                Some(PaletteInfo {
                    name: name.to_string(),
                    background: hex(p.background_rgba()),
                    stage: hex(p.stage_rgba()),
                    stops: p
                        .stops
                        .iter()
                        .map(|c| hex([c[0], c[1], c[2], 255]))
                        .collect(),
                })
            })
            .collect();
        Ok(PalettesOut { palettes })
    }
}

// ---------------------------------------------------------------------------------------------
// Seeds

/// One way to write a seed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SeedForm {
    pub form: String,
    pub meaning: String,
}

/// How a CA's cells can start.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SeedsOut {
    pub forms: Vec<SeedForm>,
    /// Shapes for `pattern:` seeds.
    pub shapes: Vec<String>,
    /// Fills for `pattern:` seeds (`pattern:SHAPE:DENSITY:STATES`).
    pub states: Vec<String>,
}

const SEED_FORMS: &[(&str, &str)] = &[
    ("empty", "no live cells"),
    (
        "random, random:0.2, random 20%, soup:0.2",
        "each cell live with this probability (default 0.3)",
    ),
    (
        "{\"random\": 0.2}, {\"density\": 0.2}",
        "the same, as an object",
    ),
    (
        "center, top_center",
        "one live cell (top_center: elementary rules)",
    ),
    ("top_random:0.5", "a random top row (elementary rules)"),
    (
        "random_states",
        "a random state in every cell (multi-state rules)",
    ),
    (
        "blob:0.3:0.5",
        "random cells in a central disc: radius (fraction of the half-width), density",
    ),
    (
        "ants:3",
        "ants and turmites: one at the centre, the rest at random",
    ),
    ("pile:10000", "sandpile grains on the centre cell"),
    (
        "gauss:3:8:1",
        "continuous rules: Gaussian bumps (count, radius in cells, peak)",
    ),
    (
        "lenia, lenia:orbium, lenia:orbium:2",
        "a Lenia creature at the centre (scale)",
    ),
    (
        "signed:0.3:0.5",
        "JACA: live cells at a density, this share of them -1",
    ),
    (
        "halves:0.3",
        "JACA: +1 cells on the left half, -1 on the right",
    ),
    (
        "glider_pair",
        "a state-1 cell beside a state-3 cell (the Spectre glider)",
    ),
    (
        "wire_demo",
        "clock loops feeding wires (WireWorld on squares)",
    ),
    (
        "pattern:blobs, pattern:voronoi:0.8:by_region",
        "a shape and a fill: shape, density, states",
    ),
    (
        "{\"shape\": \"blobs\", \"count\": 3, \"density\": 0.4, \"states\": \"by_region\"}",
        "a pattern seed with the shape's parameters",
    ),
];

impl Report for SeedsOut {
    fn text(&self) -> String {
        let mut s = String::new();
        for f in &self.forms {
            let _ = writeln!(s, "{}\n    {}", f.form, f.meaning);
        }
        let _ = writeln!(s, "\nPattern shapes: {}", self.shapes.join(", "));
        let _ = writeln!(s, "Pattern states: {}", self.states.join(", "));
        s
    }
}

/// `reference.seeds`.
pub struct ReferenceSeeds;
impl Op for ReferenceSeeds {
    const NAME: &'static str = "reference.seeds";
    const SUMMARY: &'static str =
        "The seed forms a CA's `seed` takes (random, ants, piles, Lenia, JACA, pattern seeds).";
    const READ_ONLY: bool = true;
    type In = NoInput;
    type Out = SeedsOut;
    fn run(_: &Ctx, _: NoInput, _: &dyn Progress) -> Result<SeedsOut, ApiError> {
        let words = |w: &[&str]| w.iter().map(|s| s.to_string()).collect();
        Ok(SeedsOut {
            forms: SEED_FORMS
                .iter()
                .map(|(f, m)| SeedForm {
                    form: f.to_string(),
                    meaning: m.to_string(),
                })
                .collect(),
            shapes: words(&[
                "all",
                "disc",
                "blobs",
                "clusters",
                "dots",
                "ring",
                "rings",
                "stripes",
                "checker",
                "gradient",
                "radial_gradient",
                "noise",
                "voronoi",
                "symmetric",
            ]),
            states: words(&["one", "uniform", "range:lo:hi", "signed:share", "by_region"]),
        })
    }
}

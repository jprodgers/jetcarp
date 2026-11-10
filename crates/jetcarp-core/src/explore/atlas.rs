//! The atlas: the published result of a search, the file Quick Start browses. Entries are
//! selected from a search database, given a category and palette, and stored with a mature
//! state so a preview can start where the action is.

use super::cand::Cand;
use super::features::Features;
use super::job::feat_from_blob;
use super::sheet::{RunRow, query_runs};
use super::space::{ALL_FAMS, Fam, sized_spec};
use crate::world::{Scene, SeedSpec, World};
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use rayon::prelude::*;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// How the entries are picked.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AtlasOpts {
    /// Most entries per category, per family, per tiling group.
    pub per_niche: usize,
    /// A run needs at least this confirmed (worst seed) score. Contrast picks are exempt.
    pub min_score: f64,
    /// Entries at most.
    pub max_entries: usize,
}

impl Default for AtlasOpts {
    fn default() -> Self {
        AtlasOpts {
            per_niche: 12,
            min_score: 1.25,
            max_entries: 12_000,
        }
    }
}

/// A category name for a run. The list is deliberately small and will be revised after looking
/// at what the search found.
pub fn categorize(fate: &str, f: &Features, fam: Fam, gens: u32) -> &'static str {
    if fam.is_rows() {
        return if f.bits > 0.3 { "Fractals & rows" } else { QUIET_ROWS };
    }
    if fam == Fam::Sandpile {
        return "Sandpiles";
    }
    if fam == Fam::Turmite {
        return "Ants & turmites";
    }
    let settled_early = f.settle as f32 / (gens.max(1) as f32) < 0.15;
    match fate {
        "died" => {
            return if settled_early {
                "Quick deaths"
            } else {
                "Long fades"
            };
        }
        "froze" if settled_early => return "Frozen at once",
        "froze" => {}
        _ => {}
    }
    // Stochastic: always running, and its sparks read as movers.
    if fam == Fam::ForestFire {
        return "Wildfire";
    }
    // Ships need a quiet background; in dense or busy fields small blobs drift by chance.
    let field_rule = matches!(
        fam,
        Fam::Cyclic | Fam::Rps | Fam::GreenbergHastings | Fam::Hodgepodge
    );
    if f.movers >= 3 && f.density_late < 0.12 && f.activity_late < 0.04 && !field_rule {
        return "Gliders & ships";
    }
    if fam == Fam::Jaca {
        return "Battles";
    }
    // Ahead of oscillators: a settled spiral is a long cycle.
    if (field_rule && f.smooth > 0.35 && f.entropy > 0.4) || (f.travel > 0.45 && f.smooth > 0.35) {
        return "Waves & spirals";
    }
    if fam == Fam::Lenia && f.smooth > 0.3 && f.activity_late < 0.05 {
        return "Spots & stripes";
    }
    if f.smooth > 0.55 && f.largest > 0.5 && f.activity_late < 0.02 {
        return "Labyrinths";
    }
    if fate == "cycle" && f.period > 1 {
        return "Oscillators";
    }
    if f.activity_late < 0.01 && f.comps_per_live > 0.04 {
        return "Ash & debris";
    }
    if f.growth > 1.0 {
        return "Growth";
    }
    if f.smooth < 0.08 {
        return "Static";
    }
    if f.activity_late > 0.2 {
        return "Turbulence";
    }
    "Living textures"
}

/// A palette for an entry.
pub fn pick_palette(cat: &str, fam: Fam, states: usize, id: i64) -> (&'static str, Option<&'static str>) {
    // Age-coloured rules show settled cells in a palette's last stop, so it has to stay bright or
    // a pattern that has found order fades into the background.
    let settled = [
        "Verdigris", "Marigold", "Coral", "Orchid", "Citrus", "Riso", "Bone", "Ultramarine",
        "Stained Glass", "Opal", "Synthwave", "Blueprint",
    ];
    let movers = ["Prism", "Synthwave", "Neon", "Koi", "Candy", "Riso", "Coral", "Citrus"];
    // Whole-grid fields: strong, distinct stops. Palettes that open on white wash these out.
    let fields = [
        "Stained Glass", "Riso", "Ember", "Tide", "Synthwave", "Aurora", "Verdigris", "Thermal",
        "Bone", "Deep Sea",
    ];
    let wheels = ["Stained Glass", "Opal"];
    let jaca_pos = ["Aurora", "Ultramarine", "Verdigris", "Orchid"];
    let jaca_neg = ["Marigold", "Coral", "Citrus", "Bone"];
    // Run ids can share a factor with a list's length; mix them first.
    let mixed = ((id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) as usize;
    let pick = |set: &[&'static str]| set[mixed % set.len()];
    if fam == Fam::Jaca {
        return (pick(&jaca_pos), Some(jaca_neg[(mixed >> 8) % jaca_neg.len()]));
    }
    let field_rule = matches!(
        fam,
        Fam::Cyclic | Fam::Rps | Fam::GreenbergHastings | Fam::Hodgepodge | Fam::Priority
    );
    let v = if fam == Fam::Cyclic && states > 5 && (mixed >> 8) % 2 == 0 {
        pick(&wheels)
    } else if field_rule {
        pick(&fields)
    } else if cat == "Gliders & ships" {
        pick(&movers)
    } else {
        pick(&settled)
    };
    (v, None)
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = DeflateEncoder::new(Vec::with_capacity(data.len() / 4), Compression::best());
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut d = DeflateDecoder::new(data);
    let mut out = Vec::new();
    d.read_to_end(&mut out).ok()?;
    Some(out)
}

/// One published entry.
#[derive(Clone, Debug)]
pub struct AtlasEntry {
    pub id: i64,
    pub title: String,
    pub category: String,
    pub family: String,
    pub tiling: String,
    pub tiling_group: String,
    pub rule: String,
    pub seed: String,
    pub score: f64,
    /// A scene that runs on the small grid the entry was found on (for previews).
    pub preview: Scene,
    /// The same rule and start on the larger grid it was confirmed on (what "open" loads).
    pub full: Scene,
    /// Generation of the mature state.
    pub mature_gen: u32,
    /// The mature states, deflated, or `None` if the preview starts from the seed.
    pub mature: Option<Vec<u8>>,
}

/// What the app needs to list an entry: everything but its scenes and mature state, so a
/// thousands-long menu holds no scene and no state in memory. [`load_card`] and [`load_full`]
/// read those by id when a card is visible or an entry is opened.
#[derive(Clone, Debug, PartialEq)]
pub struct AtlasMeta {
    pub id: i64,
    pub title: String,
    pub category: String,
    pub family: String,
    pub tiling: String,
    pub tiling_group: String,
    pub rule: String,
    pub seed: String,
    pub score: f64,
    pub mature_gen: u32,
    pub has_mature: bool,
}

/// The file in this order: `$JETCARP_ATLAS` (when set to an existing file),
/// `<Documents>/jetcarp-out/explore/atlas.sqlite`, then `scenes/atlas.sqlite` found walking up from
/// the executable (the repo's copy).
pub fn locate() -> Option<PathBuf> {
    let env = std::env::var_os("JETCARP_ATLAS");
    let docs = dirs::document_dir().or_else(|| dirs::home_dir().map(|h| h.join("Documents")));
    let exe = std::env::current_exe().ok();
    locate_from(env.as_deref(), docs.as_deref(), exe.as_deref())
}

/// [`locate`] with the three places given (for tests).
pub fn locate_from(env: Option<&OsStr>, docs: Option<&Path>, exe: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = env.filter(|p| !p.is_empty()).map(PathBuf::from)
        && p.is_file()
    {
        return Some(p);
    }
    if let Some(d) = docs {
        let p = d.join("jetcarp-out").join("explore").join("atlas.sqlite");
        if p.is_file() {
            return Some(p);
        }
    }
    let mut dir = exe?.parent();
    while let Some(d) = dir {
        let p = d.join("scenes").join("atlas.sqlite");
        if p.is_file() {
            return Some(p);
        }
        dir = d.parent();
    }
    None
}

/// The file's schema.
pub fn create(path: &Path) -> Result<Connection, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let _ = std::fs::remove_file(path);
    let c = Connection::open(path).map_err(e)?;
    c.execute_batch(
        "PRAGMA journal_mode=DELETE;
         CREATE TABLE entries(
           id INTEGER PRIMARY KEY, title TEXT, category TEXT, family TEXT, tiling TEXT,
           tiling_group TEXT, rule TEXT, seed TEXT, score REAL, preview TEXT, full TEXT,
           mature_gen INT, mature BLOB);
         CREATE INDEX entries_cat ON entries(category);",
    )
    .map_err(e)?;
    Ok(c)
}

/// An open atlas file (a read-only SQLite connection).
pub type AtlasConn = Connection;

/// Open an atlas file for reading.
pub fn open(path: &Path) -> Result<AtlasConn, String> {
    Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|x| x.to_string())
}

/// Everything but the scenes and mature state of every entry, best score first.
pub fn load_meta(path: &Path) -> Result<Vec<AtlasMeta>, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let c = open(path)?;
    let mut st = c
        .prepare(
            "SELECT id,title,category,family,tiling,tiling_group,rule,seed,score,mature_gen,
             mature IS NOT NULL FROM entries ORDER BY score DESC, id",
        )
        .map_err(e)?;
    let rows = st
        .query_map([], |r| {
            Ok(AtlasMeta {
                id: r.get(0)?,
                title: r.get(1)?,
                category: r.get(2)?,
                family: r.get(3)?,
                tiling: r.get(4)?,
                tiling_group: r.get(5)?,
                rule: r.get(6)?,
                seed: r.get(7)?,
                score: r.get(8)?,
                mature_gen: r.get::<_, i64>(9)? as u32,
                has_mature: r.get(10)?,
            })
        })
        .map_err(e)?;
    Ok(rows.flatten().collect())
}

/// What a card needs to start: the preview scene and the mature state, still deflated.
pub struct AtlasCard {
    pub preview: Scene,
    pub mature_gen: u32,
    pub mature: Option<Vec<u8>>,
}

/// Read one card by entry id.
pub fn load_card(conn: &AtlasConn, id: i64) -> Result<AtlasCard, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let (preview, mature_gen, mature): (String, i64, Option<Vec<u8>>) = conn
        .query_row(
            "SELECT preview, mature_gen, mature FROM entries WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(e)?;
    Ok(AtlasCard {
        preview: serde_json::from_str(&preview).map_err(|x| x.to_string())?,
        mature_gen: mature_gen as u32,
        mature,
    })
}

/// The full scene of an entry: what "open" loads.
pub fn load_full(path: &Path, id: i64) -> Result<Scene, String> {
    let c = open(path)?;
    let full: String = c
        .query_row("SELECT full FROM entries WHERE id=?1", params![id], |r| r.get(0))
        .map_err(|x| x.to_string())?;
    serde_json::from_str(&full).map_err(|x| x.to_string())
}

/// A readable account of a seed (the search's JSON), for captions and text search: `noise
/// pattern, density 0.85` or `random, density 0.35`. Never fails: an unknown seed stays as it is.
pub fn describe_seed(json: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return json.to_string();
    };
    let num = |x: Option<&serde_json::Value>, name: &str| {
        x.and_then(|d| d.as_f64())
            .map(|d| format!(", {name} {}", (d * 100.0).round() / 100.0))
    };
    match &v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(o) => match o.iter().next() {
            Some((kind, inner)) if kind == "pattern" => {
                let shape = inner
                    .pointer("/shape/kind")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                let d = num(inner.pointer("/fill/density"), "density").unwrap_or_default();
                format!("{shape} pattern{d}")
            }
            Some((kind, inner)) => {
                let d = num(inner.get("density"), "density")
                    .or_else(|| num(inner.get("count"), "count"))
                    .or_else(|| num(inner.get("radius"), "radius"))
                    .unwrap_or_default();
                format!("{kind}{d}")
            }
            None => "default seed".to_string(),
        },
        _ => json.to_string(),
    }
}

/// Load all entries of an atlas.
pub fn load(path: &Path) -> Result<Vec<AtlasEntry>, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let c = open(path)?;
    let mut st = c
        .prepare(
            "SELECT id,title,category,family,tiling,tiling_group,rule,seed,score,preview,full,
             mature_gen,mature FROM entries ORDER BY score DESC",
        )
        .map_err(e)?;
    let rows = st
        .query_map([], |r| {
            let (prev, full): (String, String) = (r.get(9)?, r.get(10)?);
            Ok(AtlasEntry {
                id: r.get(0)?,
                title: r.get(1)?,
                category: r.get(2)?,
                family: r.get(3)?,
                tiling: r.get(4)?,
                tiling_group: r.get(5)?,
                rule: r.get(6)?,
                seed: r.get(7)?,
                score: r.get(8)?,
                preview: serde_json::from_str(&prev).unwrap_or_else(|_| Scene::new(crate::topology::TilingSpec::new("square"))),
                full: serde_json::from_str(&full).unwrap_or_else(|_| Scene::new(crate::topology::TilingSpec::new("square"))),
                mature_gen: r.get::<_, i64>(11)? as u32,
                mature: r.get(12)?,
            })
        })
        .map_err(e)?;
    Ok(rows.flatten().collect())
}

/// The world of an entry at its mature state (from the seed when it has none).
pub fn preview_world(entry: &AtlasEntry) -> Result<World, String> {
    let mut w = World::from_scene(&entry.preview)?;
    if let Some(blob) = &entry.mature
        && let Some(state) = inflate(blob)
    {
        apply_mature(&mut w, &state, entry.mature_gen);
    }
    Ok(w)
}

/// Inflate a deflated mature state (a card keeps the result only while it is on screen).
pub fn inflate_state(blob: &[u8]) -> Option<Vec<u8>> {
    inflate(blob)
}

/// Put a mature state into a freshly built world, when it fits.
pub fn apply_mature(w: &mut World, state: &[u8], generation: u32) {
    if let Some(f) = w.fields.first_mut()
        && f.state.len() == state.len()
    {
        f.state.copy_from_slice(state);
        for (a, &s) in f.age.iter_mut().zip(state) {
            *a = (s != 0) as u16;
        }
        w.generation = generation as u64;
    }
}

/// Worst-seed confirmed score of each parent run.
fn confirmed_scores(conn: &Connection) -> Result<HashMap<i64, f64>, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let mut st = conn
        .prepare("SELECT parent, MIN(score) FROM runs WHERE stage=2 GROUP BY parent")
        .map_err(e)?;
    let rows: Vec<(i64, f64)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(e)?
        .flatten()
        .collect();
    Ok(rows.into_iter().collect())
}

fn tiling_group(key: &str) -> String {
    super::space::all_tilings()
        .into_iter()
        .find(|t| t.key == key)
        .map(|t| t.group)
        .unwrap_or_else(|| "other".into())
}

/// Select runs for the atlas: a list of `(parent run, confirmed score, category)`.
pub fn select(conn: &Connection, opts: &AtlasOpts) -> Result<Vec<(RunRow, f64, &'static str)>, String> {
    let scores = confirmed_scores(conn)?;
    let runs = query_runs(
        conn,
        "SELECT id FROM runs WHERE stage=1 AND (seed_kind='point' OR id IN (SELECT DISTINCT parent FROM runs WHERE stage=2))",
    )?;
    // Best-first by the worst-seed score.
    let mut scored: Vec<(RunRow, f64, &'static str)> = runs
        .into_iter()
        .filter_map(|r| {
            // A single starting cell has nothing random to confirm.
            let s = match scores.get(&r.id) {
                Some(&s) => s,
                None if point_row(r.cand.fam, &r.cand.seed) => r.score,
                None => return None,
            };
            // WireWorld needs wiring, and thin circuits go dark. Contrast picks fill in the rest.
            if r.cand.fam == Fam::WireWorld && !busy_circuit(&r.cand.seed, &r.fate) {
                return None;
            }
            let f = feat_from_blob(&r.feats);
            let c = categorize(&r.fate, &f, r.cand.fam, r.cand.gens);
            // Plain stripes from one cell don't earn the lift the fractal ones do.
            let s = if c == QUIET_ROWS { s } else { seed_adjusted(r.cand.fam, &r.cand.seed, &r.fate, s) };
            Some((r, s, c))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut per: HashMap<(&'static str, Fam, String), usize> = HashMap::new();
    let mut seen_rules: HashMap<(String, String), usize> = HashMap::new();
    let mut out = Vec::new();
    for (r, s, c) in scored {
        if s < opts.min_score {
            continue;
        }
        let key = (c, r.cand.fam, r.cand.tiling.clone());
        let n = per.entry(key).or_insert(0);
        let niche_cap = if c != QUIET_ROWS && point_row(r.cand.fam, &r.cand.seed) {
            opts.per_niche * POINT_ROW_NICHES
        } else {
            opts.per_niche
        };
        if *n >= niche_cap {
            continue;
        }
        // Avoid several entries of a rule on one tiling.
        let rk = (r.cand.rule.clone(), r.cand.tiling.clone());
        let rc = seen_rules.entry(rk).or_insert(0);
        if *rc >= 2 {
            continue;
        }
        *n += 1;
        *rc += 1;
        out.push((r, s, c));
    }
    let contrast = contrast_picks(conn, &out)?;
    let budget = opts.max_entries.saturating_sub(contrast.len());
    let mut out = trim_keeping_floor(out, budget, THIN_FAMILY, |x| x.0.cand.fam);
    out.extend(contrast);
    Ok(out)
}

/// A row or layered rule grown from one cell. Each rule gives one picture, and people kept nearly
/// all of the good ones, so these get more room per niche than the usual run.
fn point_row(fam: Fam, seed: &SeedSpec) -> bool {
    fam.is_rows() && matches!(seed, SeedSpec::TopCenter | SeedSpec::Center)
}

/// A circuit with enough wiring and signals to stay busy: every one of these rated as a keep.
fn busy_circuit(seed: &SeedSpec, fate: &str) -> bool {
    match seed {
        SeedSpec::Circuit { sites, electrons } => {
            *sites >= 40 && *electrons >= sites * 3 && fate != "froze" && fate != "died"
        }
        _ => false,
    }
}

/// Adjust a score for what the fit can't see: how the run was seeded. Rated by eye, one-cell row
/// rules beat random top rows, patterned layered starts were nearly all dropped, and busy circuits
/// score low on the fit only because they were not in its training set.
fn seed_adjusted(fam: Fam, seed: &SeedSpec, fate: &str, s: f64) -> f64 {
    match fam {
        Fam::WireWorld if busy_circuit(seed, fate) => s + 0.6,
        Fam::Layered if point_row(fam, seed) => s + 0.25,
        _ if point_row(fam, seed) => s + 0.5,
        Fam::Layered => s * 0.6,
        _ => s,
    }
}

/// Cut a best-first list to `budget`, keeping each family's best `floor` entries first, so a
/// family that scores low overall isn't cut out entirely. The floors can exceed a tiny budget.
fn trim_keeping_floor<T>(
    items: Vec<T>,
    budget: usize,
    floor: usize,
    fam: impl Fn(&T) -> Fam,
) -> Vec<T> {
    if items.len() <= budget {
        return items;
    }
    let mut seen: HashMap<Fam, usize> = HashMap::new();
    let protected: Vec<bool> = items
        .iter()
        .map(|x| {
            let n = seen.entry(fam(x)).or_insert(0);
            *n += 1;
            *n <= floor
        })
        .collect();
    let mut spare = budget.saturating_sub(protected.iter().filter(|p| **p).count());
    items
        .into_iter()
        .zip(protected)
        .filter_map(|(x, p)| {
            if p {
                Some(x)
            } else if spare > 0 {
                spare -= 1;
                Some(x)
            } else {
                None
            }
        })
        .collect()
}

/// Entries a family gets from its unconfirmed runs when confirmation left it short.
const THIN_FAMILY: usize = 60;
/// The row rules whose pictures are only stripes.
const QUIET_ROWS: &str = "Quiet rows";
/// How many times the usual niche limit a one-cell row rule gets.
const POINT_ROW_NICHES: usize = 12;
/// Examples of each kind of early end, per family and tiling group.
const ENDINGS_PER_NICHE: usize = 2;

/// Entries the score would never pick: the best screened runs of families with few confirmed
/// ones, so every family appears, and runs that die or freeze, for contrast.
fn contrast_picks(
    conn: &Connection,
    chosen: &[(RunRow, f64, &'static str)],
) -> Result<Vec<(RunRow, f64, &'static str)>, String> {
    let mut used: HashSet<i64> = chosen.iter().map(|(r, _, _)| r.id).collect();
    let mut have: HashMap<Fam, usize> = HashMap::new();
    for (r, _, _) in chosen {
        *have.entry(r.cand.fam).or_insert(0) += 1;
    }
    let mut out = Vec::new();
    for fam in ALL_FAMS {
        let want = THIN_FAMILY.saturating_sub(have.get(&fam).copied().unwrap_or(0));
        if want == 0 {
            continue;
        }
        // WireWorld contrast means circuits, the busy ones first.
        let order = if fam == Fam::WireWorld { "(seed_kind='circuit') DESC, score DESC" } else { "score DESC" };
        let mut runs = query_runs(
            conn,
            &format!(
                "SELECT id FROM runs WHERE stage=1 AND fam='{}' ORDER BY {order} LIMIT 3000",
                fam.name()
            ),
        )?;
        if fam == Fam::WireWorld {
            runs.sort_by_key(|r| !busy_circuit(&r.cand.seed, &r.fate));
        }
        // A family tied to one or two grids can fill its share there.
        let tilings = runs.iter().map(|r| r.cand.tiling.as_str()).collect::<HashSet<_>>().len();
        let tiling_cap = if tilings < 4 { THIN_FAMILY } else { 8 };
        let mut rules: HashSet<(String, String)> = HashSet::new();
        let mut per_tiling: HashMap<String, usize> = HashMap::new();
        let mut added = 0;
        for r in runs {
            if added >= want {
                break;
            }
            let n = per_tiling.entry(r.cand.tiling.clone()).or_insert(0);
            if *n >= tiling_cap
                || used.contains(&r.id)
                || !rules.insert((r.cand.rule.clone(), r.cand.tiling.clone()))
            {
                continue;
            }
            *n += 1;
            added += 1;
            used.insert(r.id);
            let c = categorize(&r.fate, &feat_from_blob(&r.feats), r.cand.fam, r.cand.gens);
            let s = r.score;
            out.push((r, s, c));
        }
    }
    let groups: HashMap<String, String> = super::space::all_tilings()
        .into_iter()
        .map(|t| (t.key, t.group))
        .collect();
    let endings = query_runs(
        conn,
        "SELECT id FROM runs WHERE stage=1 AND fate IN ('died','froze') AND id % 97 = 0
         ORDER BY score DESC LIMIT 60000",
    )?;
    let mut per: HashMap<(&'static str, Fam, String), usize> = HashMap::new();
    for r in endings {
        let c = categorize(&r.fate, &feat_from_blob(&r.feats), r.cand.fam, r.cand.gens);
        if !matches!(c, "Quick deaths" | "Long fades" | "Frozen at once") || used.contains(&r.id) {
            continue;
        }
        let group = groups.get(&r.cand.tiling).cloned().unwrap_or_default();
        let n = per.entry((c, r.cand.fam, group)).or_insert(0);
        if *n >= ENDINGS_PER_NICHE {
            continue;
        }
        *n += 1;
        used.insert(r.id);
        let s = r.score;
        out.push((r, s, c));
    }
    Ok(out)
}

fn make_entry(run: &RunRow, score: f64, cat: &str, group: &str) -> Result<AtlasEntry, String> {
    let c: &Cand = &run.cand;
    let f = feat_from_blob(&run.feats);
    let states = c.rule().map_or(2, |r| match r.field_kind() {
        crate::rules::FieldKind::Multi(n) => n as usize,
        _ => 2,
    });
    let (pal, pal_neg) = pick_palette(cat, c.fam, states, run.id);
    let scene_for = |cells: u32, rng: u64| -> Result<Scene, String> {
        let (spec, _) = sized_spec(&c.tiling, cells)?;
        let mut cc = c.clone();
        cc.rng = rng;
        let mut s = cc.scene(&spec);
        s.cas[0].palette = Some(pal.to_string());
        s.cas[0].palette_neg = pal_neg.map(String::from);
        s.name = format!("{} — {}", c.rule, c.tiling);
        Ok(s)
    };
    let preview = scene_for(c.cells, c.rng)?;
    let full = scene_for(64_000, c.rng)?;
    // Skip to where the action is: just before the run settles, or halfway.
    let (mature_gen, mature) = if c.fam.is_rows() || matches!(c.fam, Fam::Turmite | Fam::Sandpile | Fam::Lenia) {
        (0, None)
    } else {
        let at = if run.fate == "died" {
            0
        } else if run.fate == "running" {
            c.gens / 2
        } else {
            f.settle.saturating_sub(200).min(c.gens)
        };
        if at == 0 {
            (0, None)
        } else {
            let mut w = World::from_scene(&preview)?;
            for _ in 0..at {
                w.step();
            }
            (at, w.fields.first().map(|x| deflate(&x.state)))
        }
    };
    Ok(AtlasEntry {
        id: run.id,
        title: format!("{} on {}", c.rule, c.tiling),
        category: cat.to_string(),
        family: c.fam.name().to_string(),
        tiling: c.tiling.clone(),
        tiling_group: group.to_string(),
        rule: c.rule.clone(),
        seed: serde_json::to_string(&c.seed).unwrap_or_default(),
        score,
        preview,
        full,
        mature_gen,
        mature,
    })
}

/// Build an atlas file from a search database.
pub fn build(conn: &Connection, out: &Path, opts: &AtlasOpts) -> Result<usize, String> {
    let picks = select(conn, opts)?;
    let groups: HashMap<String, String> = super::space::all_tilings()
        .into_iter()
        .map(|t| (t.key, t.group))
        .collect();
    let entries: Vec<AtlasEntry> = picks
        .par_iter()
        .filter_map(|(r, s, c)| {
            let group = groups.get(&r.cand.tiling).cloned().unwrap_or_else(|| "other".into());
            make_entry(r, *s, c, &group).ok()
        })
        .collect();
    let mut db = create(out)?;
    let e = |x: rusqlite::Error| x.to_string();
    let tx = db.transaction().map_err(e)?;
    {
        let mut st = tx
            .prepare(
                "INSERT INTO entries(id,title,category,family,tiling,tiling_group,rule,seed,score,
                 preview,full,mature_gen,mature) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
            )
            .map_err(e)?;
        for a in &entries {
            st.execute(params![
                a.id,
                a.title,
                a.category,
                a.family,
                a.tiling,
                a.tiling_group,
                a.rule,
                a.seed,
                a.score,
                serde_json::to_string(&a.preview).unwrap_or_default(),
                serde_json::to_string(&a.full).unwrap_or_default(),
                a.mature_gen,
                a.mature,
            ])
            .map_err(e)?;
        }
    }
    tx.commit().map_err(e)?;
    let _ = tiling_group;
    Ok(entries.len())
}

/// A stratified sample of screened runs for a person to rate: spread over categories and
/// families, half strong and half random, so ratings can calibrate the score.
pub fn taste_sample(conn: &Connection, n: usize, seed: u64) -> Result<Vec<(RunRow, &'static str)>, String> {
    let strong = query_runs(
        conn,
        "SELECT id FROM runs WHERE stage=1 ORDER BY score DESC LIMIT 12000",
    )?;
    let any = query_runs(
        conn,
        "SELECT id FROM runs WHERE stage=1 AND score>0.2 ORDER BY RANDOM() LIMIT 12000",
    )?;
    let mut rng = crate::rng::Rng::new(seed);
    let mut by: HashMap<&'static str, (Vec<RunRow>, Vec<RunRow>)> = HashMap::new();
    let cat = |r: &RunRow| {
        let f = feat_from_blob(&r.feats);
        categorize(&r.fate, &f, r.cand.fam, r.cand.gens)
    };
    for r in strong {
        by.entry(cat(&r)).or_default().0.push(r);
    }
    for r in any {
        by.entry(cat(&r)).or_default().1.push(r);
    }
    let mut cats: Vec<&'static str> = by.keys().copied().collect();
    cats.sort_unstable();
    let per = (n / cats.len().max(1)).max(1);
    let mut seen: std::collections::HashSet<(Fam, String)> = Default::default();
    let mut out = Vec::new();
    for c in cats {
        let (a, b) = by.get_mut(c).unwrap();
        let mut got = 0;
        for k in 0..per * 40 {
            if got >= per {
                break;
            }
            let v: &mut Vec<RunRow> = if k % 2 == 0 && !a.is_empty() { a } else { b };
            if v.is_empty() {
                continue;
            }
            let i = rng.below(v.len() as u64) as usize;
            let r = v.swap_remove(i);
            if !seen.insert((r.cand.fam, r.cand.tiling.clone())) {
                continue;
            }
            out.push((r, c));
            got += 1;
        }
    }
    Ok(out)
}

/// Stored runs with their categories, in database order.
pub fn runs_by_id(conn: &Connection, ids: &[i64]) -> Result<Vec<(RunRow, &'static str)>, String> {
    let list = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let runs = query_runs(conn, &format!("SELECT id FROM runs WHERE id IN ({list})"))?;
    Ok(runs
        .into_iter()
        .map(|r| {
            let c = categorize(&r.fate, &feat_from_blob(&r.feats), r.cand.fam, r.cand.gens);
            (r, c)
        })
        .collect())
}

/// An animated clip of a run: `frames` pictures spread over the whole run.
pub fn clip(run: &RunRow, cat: &str, frames: u32, px: u32) -> Result<Vec<u8>, String> {
    let c = &run.cand;
    let states = c.rule().map_or(2, |r| match r.field_kind() {
        crate::rules::FieldKind::Multi(n) => n as usize,
        _ => 2,
    });
    let (pal, pal_neg) = pick_palette(cat, c.fam, states, run.id);
    let (_, topo) = sized_spec(&c.tiling, c.cells)?;
    let mut w = World::on_topology(topo, c.rng);
    let mut ca = c.ca();
    ca.palette = Some(pal.to_string());
    ca.palette_neg = pal_neg.map(String::from);
    w.add_ca(ca)?;
    let opts = crate::render::RenderOptions {
        outline_min_px: 0.0,
        ..Default::default()
    };
    let mut imgs = Vec::with_capacity(frames as usize);
    let mut at = 0u32;
    for k in 0..frames {
        // Spread frames on a square-root scale: early generations change fastest.
        let t = (k as f64 / (frames - 1).max(1) as f64).powi(2);
        let target = (t * c.gens as f64) as u32;
        while at < target {
            w.step();
            at += 1;
        }
        imgs.push(crate::render::render_fitted(&w, px, px, &opts));
    }
    let refs: Vec<&crate::render::Image> = imgs.iter().collect();
    crate::media::encode::gif(&refs, 110)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeding_shifts_the_atlas_score() {
        let busy = SeedSpec::Circuit { sites: 60, electrons: 300 };
        let thin = SeedSpec::Circuit { sites: 8, electrons: 8 };
        let soup = SeedSpec::Random { density: 0.3 };
        assert_eq!(seed_adjusted(Fam::WireWorld, &busy, "cycle", 0.6), 1.2);
        assert_eq!(seed_adjusted(Fam::WireWorld, &busy, "froze", 0.6), 0.6);
        assert_eq!(seed_adjusted(Fam::WireWorld, &thin, "cycle", 0.6), 0.6);
        assert_eq!(seed_adjusted(Fam::Wolfram, &SeedSpec::TopCenter, "froze", 1.0), 1.5);
        assert_eq!(seed_adjusted(Fam::Layered, &SeedSpec::Center, "froze", 1.0), 1.25);
        assert_eq!(seed_adjusted(Fam::Layered, &soup, "froze", 1.0), 0.6);
        assert_eq!(seed_adjusted(Fam::Jaca, &soup, "cycle", 1.0), 1.0);
    }

    #[test]
    fn trimming_keeps_a_low_scoring_family() {
        // Best-first: ten Life-like entries, then three Layered ones that a plain cut would lose.
        let mut items = vec![Fam::LifeLike; 10];
        items.extend([Fam::Layered; 3]);
        let kept = trim_keeping_floor(items, 6, 2, |f| *f);
        assert_eq!(kept.len(), 6);
        assert_eq!(kept.iter().filter(|f| **f == Fam::Layered).count(), 2);
        assert_eq!(kept.iter().filter(|f| **f == Fam::LifeLike).count(), 4);
    }

    #[test]
    fn trimming_leaves_a_short_list_alone() {
        let items = vec![Fam::Layered; 3];
        assert_eq!(trim_keeping_floor(items, 6, 1, |f| *f).len(), 3);
    }
}

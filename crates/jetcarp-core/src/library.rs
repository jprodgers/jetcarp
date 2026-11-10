//! Pattern library: verified still lifes, oscillators, and spaceships in the SQLite catalog.
//!
//! **Store.** [`Library`] is the catalog file (`JETCARP_LIBRARY`, else
//! `<Documents>/jetcarp-out/catalog.sqlite`); [`store`] documents its tables and migrations. JSON
//! records ([`PatternRecord::to_json`], one file per pattern) remain the exchange format for
//! [`Library::export`] and [`Library::import`].
//!
//! **Identity and dedup.** A pattern's `id` is rule-free ([`range::identity`]): a hash of the
//! tiling's identity (family, shape parameters, edges; not size or boundary), neighbourhood,
//! steps per generation, cell-state model, and the canonical evolution (every phase of one period
//! in the canonical orientation, plus the displacement). The canonical orientation is the minimum
//! over every phase and every lattice symmetry of the tiling (8 on squares, 12 on hexagons, the
//! tiling's own point group with its tile-type permutation on Archimedean and k-uniform tilings;
//! only translations for rule tables) of the normalized cell list, so the same object found in
//! another phase, orientation, or rule with the same evolution gets the same id. Adding a pattern
//! that is already stored adds to its `seen` count and its rules, and keeps the earliest
//! provenance, the curation fields, and the name.
//!
//! **Rule ranges.** B/S and Generations patterns carry the range of rules they run identically in
//! ([`range::rule_range`]), so the catalog can answer "every pattern that works in rule R".
//!
//! **Verification.** Nothing is stored unverified ([`verify`]): the pattern is pasted alone on an
//! empty dead-edged field with room for N periods of travel, run, and every generation is compared
//! exactly (states and positions) with the stored phases, shifted by the displacement for
//! spaceships. A live cell next to the frame edge fails the check, so the run equals the
//! infinite plane. Aperiodic patterns are run on the patch they were found on.
//!
//! **Aperiodic tilings** (Spectre, hat, Penrose) have no lattice, so a pattern is stored with its
//! surrounding tiles ([`Site`]: every tile within a few neighbour steps, as rigid placements
//! relative to an anchor tile) and can be pasted only where a patch has the same configuration.
//!
//! The main entry points: [`Library`] (add, get, list, query, update, remove, export, import,
//! sightings, jobs, Quick Start rows), [`record_from_object`], [`verify`],
//! [`search`](search::search), [`collide`](collide::collide), [`promote`](promote::promote) and
//! the [`Command::PastePattern`](crate::world::Command::PastePattern) command.

use crate::objects::{LCell, ObjClass, Object, Phase};
use crate::topology::{Nb, TilingSpec, Topology};
use crate::world::World;
use serde::{Deserialize, Serialize};

pub mod collide;
pub mod movers;
pub mod paste;
pub mod promote;
pub mod range;
pub mod search;
pub mod site;
pub mod site_graph;
pub mod split;
pub mod store;
pub mod symmetry;
pub mod verify;

pub use paste::{PasteAt, PatternSource};
pub use range::{RuleRange, identity};
pub use site::Site;
pub use store::{
    AddOutcome, AntCensus, AntQuery, AntRow, AntUpdate, FlushFinish, JobStatus, Library,
    NewSetting, QsStatus, QuickStartRow, RatingRow, RescoreReport, SettingRow, SettingStatus,
    SettingsQuery, Sighting, SightingRow, Stage1Flush, StoredSearch, ThinOptions, ThinReport,
    ant_id, ant_score, parse_setting_id,
};
pub use store::{FindRow, HeatQuery, HeatRow, PruneReport, UnstoredFlush, UnstoredRow, UsageRow};
pub use symmetry::CellMap;
pub use verify::{Verification, default_periods, verify};

/// Version of the record format (2: rule-free ids, `rules`, `range`).
pub const RECORD_FORMAT: u32 = 2;

// ---------------------------------------------------------------------------------------------
// Record

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternClass {
    StillLife,
    Oscillator,
    Spaceship,
    /// Travels while leaving debris behind: its front recurs shifted by a lattice vector.
    Puffer,
    Other,
    /// Aperiodic tilings: a glider or other mover, verified to a distance (see `mover`).
    Mover,
    /// A stationary core that recurs every `period` generations while emitting a stream of
    /// identical spaceships (see `gun`).
    Gun,
}

impl PatternClass {
    pub fn name(self) -> &'static str {
        match self {
            PatternClass::StillLife => "still life",
            PatternClass::Oscillator => "oscillator",
            PatternClass::Spaceship => "spaceship",
            PatternClass::Puffer => "puffer",
            PatternClass::Other => "other",
            PatternClass::Mover => "mover",
            PatternClass::Gun => "gun",
        }
    }
    /// Parse `still_life`, `still life`, `still`, `osc`, `oscillator`, `ship`, `spaceship`, ...
    pub fn parse(s: &str) -> Result<PatternClass, String> {
        Ok(
            match s
                .trim()
                .to_ascii_lowercase()
                .replace([' ', '-'], "_")
                .as_str()
            {
                "still_life" | "still" | "sl" | "stilllife" => PatternClass::StillLife,
                "oscillator" | "osc" => PatternClass::Oscillator,
                "spaceship" | "ship" => PatternClass::Spaceship,
                "puffer" => PatternClass::Puffer,
                "other" => PatternClass::Other,
                "mover" | "glider" => PatternClass::Mover,
                "gun" => PatternClass::Gun,
                _ => {
                    return Err(format!(
                        "unknown class '{s}' (still_life, oscillator, spaceship, puffer, gun, mover)"
                    ));
                }
            },
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CurationState {
    #[default]
    New,
    Reviewed,
    Promoted,
    Rejected,
}

impl CurationState {
    pub fn parse(s: &str) -> Result<CurationState, String> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "new" => CurationState::New,
            "reviewed" => CurationState::Reviewed,
            "promoted" => CurationState::Promoted,
            "rejected" => CurationState::Rejected,
            _ => {
                return Err(format!(
                    "unknown state '{s}' (new, reviewed, promoted, rejected)"
                ));
            }
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            CurationState::New => "new",
            CurationState::Reviewed => "reviewed",
            CurationState::Promoted => "promoted",
            CurationState::Rejected => "rejected",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveredBy {
    #[default]
    Search,
    Script,
    Llm,
    User,
}

impl DiscoveredBy {
    pub fn parse(s: &str) -> Result<DiscoveredBy, String> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "search" => DiscoveredBy::Search,
            "script" => DiscoveredBy::Script,
            "llm" => DiscoveredBy::Llm,
            "user" => DiscoveredBy::User,
            _ => {
                return Err(format!(
                    "unknown discoverer '{s}' (search, script, llm, user)"
                ));
            }
        })
    }
}

/// Where and how a pattern was first found.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Provenance {
    /// `search`, `collide <a> + <b> ...`, a scene path, `script`, `user`.
    pub source: String,
    /// The tiling of the run (with its size and boundary).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tiling: Option<TilingSpec>,
    /// RNG state of the soup (search: seed + soup).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rng_state: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soup: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub density: Option<f64>,
    /// Generation of the run when the object was taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    pub jetcarp_version: String,
    /// UTC time, `YYYY-MM-DDTHH:MM:SSZ`.
    pub date: String,
}

/// One pattern. Cells are `i,j,k,s` tuples separated by spaces: lattice column `i` and row `j`
/// (unwrapped, unsheared lattice coordinates), tile type `k` within the unit cell, and state `s`.
/// On aperiodic tilings `i` is an index into [`Site::tiles`] and `j = k = 0`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternRecord {
    #[serde(default = "fmt1")]
    pub format: u32,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Rule notation as the CA reports it (`B3/S23`, `B2/S/C3`): the rule the pattern runs in
    /// by default (the first rule it was verified in; the one this record was found in).
    pub rule: String,
    /// Every rule it has been verified in (from the catalog), first first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
    /// B/S and Generations patterns: every rule it runs identically in ([`range`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<RuleRange>,
    pub neighborhood: Nb,
    #[serde(default = "one32")]
    pub steps_per_gen: u32,
    /// Tiling identity: family, parameters, and edges (no size; boundary unused).
    pub tiling: TilingSpec,
    pub tiling_name: String,
    /// Tile types per unit cell (aperiodic: 0).
    pub tile_types: u32,
    pub class: PatternClass,
    pub period: u64,
    /// Spaceships: lattice vector `[di, dj]` moved per period (a translation of the unit cell, so
    /// every tile returns to a tile of the same type).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub displacement: Option<[i32; 2]>,
    /// The same displacement in world units (tile edges are about 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub displacement_xy: Option<[f64; 2]>,
    /// `c/4 diagonal`, `c/2 orthogonal`, in lattice steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<String>,
    /// Live cells in phase 0, and the fewest and most over all phases.
    pub cells: usize,
    pub min_cells: usize,
    pub max_cells: usize,
    /// Phase-0 bounding box in lattice coordinates `[i0, j0, i1, j1]` (`i0 = j0 = 0`).
    pub bbox: [i32; 4],
    /// Canonical form (rows `.o./..o/ooo` on one-tile lattices, `k@i,j` lists otherwise, a
    /// configuration descriptor on aperiodic patches). Hashed into `id`.
    pub canonical: String,
    /// One period of phases in the canonical orientation, phase 0 first.
    pub phases: Vec<String>,
    /// Aperiodic tilings: the surrounding tiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<Site>,
    pub verified: Verification,
    /// Movers on aperiodic tilings: how far the tracker verified it (`library::movers`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mover: Option<movers::MoverRecord>,
    /// Guns: the emitted spaceship, its direction, and how many leave per period.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gun: Option<GunInfo>,
    /// How many times it has been found (objects counted over every search and collision).
    pub seen: u64,
    pub provenance: Provenance,
    pub discovered_by: DiscoveredBy,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Interest score from [`interest_score`] (recomputed when the record changes).
    pub score: f64,
    /// A curator's rating, 1 (dull) to 5 (remarkable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<u8>,
    pub state: CurationState,
    pub created: String,
    pub updated: String,
}
/// What a gun emits. Directions are in the gun's canonical orientation (as its `phases`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GunInfo {
    /// The emitted spaceship's catalog id (rule-free) and label.
    pub ship: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ship_label: String,
    /// The ship's period and lattice displacement per ship period, as it leaves this gun.
    pub ship_period: u64,
    pub ship_displacement: [i32; 2],
    /// `c/4 diagonal` style speed of the ship.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ship_speed: String,
    /// Ships emitted per gun period.
    pub per_period: u32,
    /// One period of the ship's phases in the direction it leaves (`i,j,k,s` tuples, each phase
    /// at the origin), so the stream can be checked without the catalog.
    pub ship_phases: Vec<String>,
}

fn fmt1() -> u32 {
    RECORD_FORMAT
}
fn one32() -> u32 {
    1
}

impl PatternRecord {
    /// Phase `t` (taken modulo the period) as cells; spaceship phases past the first period are
    /// shifted by the displacement. Puffers and movers store only their starting phase.
    pub fn phase(&self, t: u64) -> Result<Phase, String> {
        let p = self.phases.len() as u64;
        if p == 0 {
            return Err(format!("{}: no phases", self.id));
        }
        let mut v = decode_cells(&self.phases[(t % p) as usize])?;
        if let (Some([di, dj]), PatternClass::Spaceship) = (self.displacement, self.class) {
            let n = (t / p) as i32;
            for c in &mut v {
                c.i += di * n;
                c.j += dj * n;
            }
        }
        Ok(v)
    }
    /// Every stored phase as cells.
    pub fn all_phases(&self) -> Result<Vec<Phase>, String> {
        self.phases.iter().map(|p| decode_cells(p)).collect()
    }
    pub fn is_aperiodic(&self) -> bool {
        self.site.is_some()
    }
    /// Does it run exactly as stored in this rule (same neighbourhood)? True when it was
    /// verified there or the rule is inside its rule range.
    pub fn works_in(&self, rule: &str) -> bool {
        let Ok((r, _)) = crate::rules::Rule::parse(rule) else {
            return false;
        };
        let n = r.notation();
        n == crate::rules::Rule::parse(&self.rule)
            .map(|x| x.0.notation())
            .unwrap_or_default()
            || self.rules.contains(&n)
            || self.range.as_ref().is_some_and(|x| x.contains(&r))
    }
    /// The record set to run in another rule it works in ([`works_in`](Self::works_in)).
    pub fn in_rule(&self, rule: &str) -> Result<PatternRecord, String> {
        if !self.works_in(rule) {
            return Err(format!(
                "{} does not run as stored in {rule} (it works in {}{})",
                self.label(),
                self.rule,
                self.range
                    .as_ref()
                    .map(|r| format!(", {}", r.describe()))
                    .unwrap_or_default()
            ));
        }
        let mut r = self.clone();
        r.rule = crate::rules::Rule::parse(rule)?.0.notation();
        Ok(r)
    }
    /// Display name: the name, or a generated label like `p2 oscillator 12c #3fa9`.
    pub fn label(&self) -> String {
        if let Some(n) = &self.name {
            return n.clone();
        }
        let short = &self.id[..self.id.len().min(6)];
        match self.class {
            PatternClass::StillLife => format!("still life {}c #{short}", self.cells),
            PatternClass::Oscillator => {
                format!("p{} oscillator {}c #{short}", self.period, self.cells)
            }
            PatternClass::Spaceship => format!(
                "{} spaceship {}c #{short}",
                self.speed.as_deref().unwrap_or("?"),
                self.cells
            ),
            PatternClass::Gun => format!(
                "p{} {} gun {}c #{short}",
                self.period,
                self.gun
                    .as_ref()
                    .map(|g| g.ship_label.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("ship"),
                self.cells
            ),
            c => format!("{} {}c #{short}", c.name(), self.cells),
        }
    }
    /// One line for listings.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "{}  {:<34} {:<12} {:<26} p{:<3} {:>4}c  seen {:<5} score {:>5.1}  {}",
            self.id,
            truncate(&self.label(), 34),
            truncate(&self.rule, 12),
            truncate(&self.tiling_name, 26),
            self.period,
            self.cells,
            self.seen,
            self.score,
            self.state.name()
        );
        if let Some(sp) = &self.speed {
            s.push_str(&format!("  {sp}"));
        }
        s
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }
    pub fn from_json(s: &str) -> Result<PatternRecord, String> {
        serde_json::from_str(s).map_err(|e| format!("pattern record JSON: {e}"))
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(n.saturating_sub(1)).collect();
        t.push('~');
        t
    }
}

/// `i,j,k,s` tuples separated by spaces.
pub fn encode_cells(cells: &[LCell]) -> String {
    cells
        .iter()
        .map(|c| format!("{},{},{},{}", c.i, c.j, c.k, c.s))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse `i,j,k,s` tuples (`i,j` alone means tile type 0, state 1); the result is sorted.
pub fn decode_cells(s: &str) -> Result<Phase, String> {
    let mut v = Vec::new();
    for t in s.split_whitespace() {
        let n: Vec<i64> = t
            .split(',')
            .map(|x| x.trim().parse::<i64>())
            .collect::<Result<_, _>>()
            .map_err(|_| format!("bad cell '{t}' (want i,j,k,s)"))?;
        let (i, j, k, st) = match n.as_slice() {
            [i, j] => (*i, *j, 0, 1),
            [i, j, k] => (*i, *j, *k, 1),
            [i, j, k, st] => (*i, *j, *k, *st),
            _ => return Err(format!("bad cell '{t}' (want i,j,k,s)")),
        };
        if !(0..=255).contains(&st) || k < 0 {
            return Err(format!("bad cell '{t}': state 0-255, tile type >= 0"));
        }
        v.push(LCell {
            i: i as i32,
            j: j as i32,
            k: k as u32,
            s: st as u8,
        });
    }
    v.sort_unstable();
    Ok(v)
}

// ---------------------------------------------------------------------------------------------
// Interest score

/// The interest heuristic, 0 to 100 (higher = more interesting):
///
/// | Term | Value |
/// | --- | --- |
/// | class | gun 70 + 4·log2(p), mover (aperiodic glider) 65, spaceship 60, puffer 55, oscillator 15, still life 5 |
/// | period | spaceship 4·log2(p); oscillator 8·log2(p), at most 40 |
/// | direction | +5 for diagonal or oblique spaceships |
/// | size | still life: min(cells, 24)/2; others: min(cells, 40)/8 |
/// | rarity | 15 / (1 + log2(seen)): 15 when seen once, 7.5 twice, 1.4 after 1,000 |
///
/// So spaceships rank far above high-period oscillators, which rank above rare still lifes, and
/// among equals the rarer and larger pattern ranks first. A curator's `rating` is kept apart.
pub fn interest_score(
    class: PatternClass,
    period: u64,
    cells: usize,
    seen: u64,
    speed: Option<&str>,
) -> f64 {
    let lp = (period.max(1) as f64).log2();
    let mut s = match class {
        PatternClass::Spaceship => 60.0 + 4.0 * lp,
        PatternClass::Puffer => 55.0 + 4.0 * lp,
        PatternClass::Mover => 65.0,
        PatternClass::Gun => 70.0 + 4.0 * lp,
        PatternClass::Oscillator => 15.0 + (8.0 * lp).min(40.0),
        PatternClass::StillLife => 5.0,
        PatternClass::Other => 0.0,
    };
    if class == PatternClass::Spaceship && speed.is_some_and(|s| !s.contains("orthogonal")) {
        s += 5.0;
    }
    s += match class {
        PatternClass::StillLife => cells.min(24) as f64 / 2.0,
        _ => cells.min(40) as f64 / 8.0,
    };
    s += 15.0 / (1.0 + (seen.max(1) as f64).log2());
    (s.clamp(0.0, 100.0) * 10.0).round() / 10.0
}

impl PatternRecord {
    /// Recompute `score` from the record's fields.
    pub fn rescore(&mut self) {
        self.score = interest_score(
            self.class,
            self.period,
            self.cells,
            self.seen,
            self.speed.as_deref(),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Dates (UTC, no time zone database)

/// Current UTC time as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn now_utc() -> String {
    utc_of(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    )
}

/// Seconds since 1970 as `2026-11-06T12:00:00Z`.
pub fn utc_of(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

// ---------------------------------------------------------------------------------------------
// Tiling identity

/// The parts of a tiling spec that define its cells: family, parameters, and edges.
pub fn tiling_identity(t: &TilingSpec) -> TilingSpec {
    TilingSpec {
        family: t.family.clone(),
        params: t.params.clone(),
        edges: t.edges.clone(),
        size: None,
        boundary: Default::default(),
    }
}

/// Text used in ids and comparisons.
pub fn tiling_key(t: &TilingSpec) -> String {
    let id = tiling_identity(t);
    serde_json::to_string(&id).unwrap_or_default()
}

/// A tiling's display name: the family name, or the catalog name for `periodic` catalog tilings.
pub fn tiling_display_name(t: &TilingSpec) -> String {
    if t.family == "periodic"
        && let Some(id) = t
            .params
            .as_ref()
            .and_then(|p| p.get("catalog"))
            .and_then(|v| v.as_str())
        && let Some(c) = crate::catalog::get(id)
    {
        return format!("{} ({})", c.name, c.id);
    }
    crate::quickstart::tiling_name(t)
}

/// A tiling from a short key: a family id (`square`, `hex`, `oct`, `ih28`, `spectre`) or
/// `catalog:<id>` / a bare catalog id (`k2-001`) for the k-uniform and Archimedean catalog.
pub fn tiling_from_key(key: &str) -> Result<TilingSpec, String> {
    let key = key.trim();
    let cat = key.strip_prefix("catalog:").unwrap_or(key);
    if crate::families::family(key).is_none() && crate::catalog::get(cat).is_some() {
        let mut spec = TilingSpec::new("periodic");
        spec.params = Some(serde_json::json!({ "catalog": cat }));
        return Ok(spec);
    }
    if crate::families::family(key).is_some() {
        return Ok(TilingSpec::new(key));
    }
    if let Some(id) = crate::families::alias(key) {
        return Ok(TilingSpec::new(id));
    }
    Err(format!(
        "unknown tiling '{key}' (a family id from `jetcarp tilings`, or catalog:<id> such as catalog:k2-001)"
    ))
}

/// Expand a tiling list: comma or space separated keys, plus the sets `all-periodic` (every
/// periodic family: square, hex, the Archimedean and other fixed tilings, the reshapeable
/// families at their default shape; not the isohedral types), `archimedean` (the 11),
/// `kuniform:<k>` (every k-uniform catalog tiling with that k), `isohedral` (all 81 types).
pub fn tiling_list(text: &str) -> Result<Vec<TilingSpec>, String> {
    let mut out: Vec<TilingSpec> = Vec::new();
    for key in text
        .split([',', ' ', ';', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let lower = key.to_ascii_lowercase();
        if lower == "all-periodic" || lower == "periodic-all" {
            for f in crate::families::FAMILIES {
                if f.kind != crate::families::FamilyKind::Aperiodic && f.id != "periodic" {
                    out.push(TilingSpec::new(f.id));
                }
            }
        } else if lower == "archimedean" {
            for id in [
                "square",
                "hex",
                "triangle",
                "oct",
                "trihex",
                "elongtri",
                "snubsq",
                "snubhex",
                "rhombitrihex",
                "trunchex",
                "trunctrihex",
            ] {
                out.push(TilingSpec::new(id));
            }
        } else if let Some(k) = lower
            .strip_prefix("kuniform:")
            .or_else(|| lower.strip_prefix("k-uniform:"))
        {
            let k: u32 = k.parse().map_err(|_| format!("bad k in '{key}'"))?;
            for t in crate::catalog::all()
                .iter()
                .filter(|t| t.k == k && t.id.starts_with('k'))
            {
                out.push(tiling_from_key(&format!("catalog:{}", t.id))?);
            }
        } else if lower == "isohedral" {
            for f in crate::isohedral::IH_FAMILIES.iter() {
                out.push(TilingSpec::new(f.id));
            }
        } else {
            out.push(tiling_from_key(key)?);
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| seen.insert(tiling_key(t)));
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Records from detected objects

/// Lattice symmetries used for canonical forms: the tiling's symmetries as maps on lattice
/// cells (see [`symmetry`]); only the identity for rule tables (not isotropic) and aperiodic
/// patches.
pub fn canonical_symmetries(topo: &Topology, nb: Nb, isotropic: bool) -> Vec<CellMap> {
    symmetry::cell_maps(topo, nb, isotropic)
}

/// Transform a phase by a symmetry and move its minimum to (0, 0). Returns the shift.
pub fn orient(cells: &[LCell], m: &CellMap) -> (Phase, [i32; 2]) {
    let mut v: Phase = cells.iter().map(|c| m.apply(c)).collect();
    let mi = v.iter().map(|c| c.i).min().unwrap_or(0);
    let mj = v.iter().map(|c| c.j).min().unwrap_or(0);
    for c in &mut v {
        c.i -= mi;
        c.j -= mj;
    }
    v.sort_unstable();
    (v, [mi, mj])
}

/// The symmetry (index into `syms`) and phase that give the canonical form: the minimum over
/// every phase and symmetry of the normalized cells (first such pair in order).
pub fn canonical_choice(phases: &[Phase], syms: &[CellMap]) -> Option<(usize, usize)> {
    let mut best: Option<(Phase, usize, usize)> = None;
    for (mi, m) in syms.iter().enumerate() {
        for (t, ph) in phases.iter().enumerate() {
            let (v, _) = orient(ph, m);
            if best.as_ref().is_none_or(|b| v < b.0) {
                best = Some((v, mi, t));
            }
        }
    }
    best.map(|b| (b.1, b.2))
}

/// Canonical phases of a periodic lattice object: `phases` is one period as run (phase 0
/// first), `d` its displacement per period. Returns (phases in the canonical orientation with
/// phase 0 at the origin, the displacement in that orientation).
pub fn canonical_phases(phases: &[Phase], d: [i32; 2], syms: &[CellMap]) -> (Vec<Phase>, [i32; 2]) {
    let p = phases.len();
    let Some((mi, t0)) = canonical_choice(phases, syms) else {
        return (vec![], d);
    };
    let m = &syms[mi];
    let (_, shift) = orient(&phases[t0], m);
    let out = (0..p)
        .map(|u| {
            let q = t0 + u;
            let n = (q / p) as i32;
            let mut v: Phase = phases[q % p]
                .iter()
                .map(|c| {
                    let mut c = *c;
                    c.i += d[0] * n;
                    c.j += d[1] * n;
                    let mut c = m.apply(&c);
                    c.i -= shift[0];
                    c.j -= shift[1];
                    c
                })
                .collect();
            v.sort_unstable();
            v
        })
        .collect();
    (out, m.vector(d))
}

/// Build a record from an object found by [`crate::objects::find_objects`] in `world`, using the
/// CA it was found with. The record is not verified yet (`verified.periods == 0`).
pub fn record_from_object(
    world: &World,
    ca_id: u32,
    obj: &Object,
    provenance: Provenance,
    discovered_by: DiscoveredBy,
) -> Result<PatternRecord, String> {
    let ca = world
        .ca(ca_id)
        .ok_or_else(|| format!("no CA with id {ca_id}"))?;
    let (class, period, d) = match obj.class {
        ObjClass::StillLife => (PatternClass::StillLife, 1, [0, 0]),
        ObjClass::Oscillator { period } => (PatternClass::Oscillator, period, [0, 0]),
        ObjClass::Spaceship { period, dx, dy } => (PatternClass::Spaceship, period, [dx, dy]),
        ObjClass::Puffer { period, dx, dy } => (PatternClass::Puffer, period, [dx, dy]),
        ObjClass::Other { .. } => {
            return Err("only still lifes, oscillators, and spaceships are stored".into());
        }
    };
    let want = if class == PatternClass::Puffer {
        1
    } else {
        period
    };
    if obj.phases.len() as u64 != want {
        return Err(format!(
            "object has {} phases for period {period}",
            obj.phases.len()
        ));
    }
    let isotropic = crate::objects::supported(&ca.rule)?;
    if let crate::rules::Rule::Table(name) = &ca.rule
        && crate::table::TableSet::with_builtins().get(name).is_none()
    {
        return Err(format!(
            "rule table '{name}' is not built in, so the library cannot replay it"
        ));
    }
    let what = RecordParts {
        rule: ca.notation(),
        nb: ca.nb,
        steps_per_gen: ca.steps_per_gen,
        isotropic,
        class,
        period,
        d,
        name: obj.known_name.then(|| obj.name.clone()),
    };
    build_record(&world.topo, &what, &obj.phases, provenance, discovered_by)
}

/// Everything about a pattern except its cells, for [`build_record`].
#[derive(Clone, Debug)]
pub struct RecordParts {
    pub rule: String,
    pub nb: Nb,
    pub steps_per_gen: u32,
    pub isotropic: bool,
    pub class: PatternClass,
    pub period: u64,
    /// Displacement per period (spaceships).
    pub d: [i32; 2],
    pub name: Option<String>,
}

/// A record (canonical form, id, score; not verified) from one period of phases on `topo` (any
/// grid of the tiling; explicit patches: cell ids in `i`).
pub fn build_record(
    topo: &Topology,
    what: &RecordParts,
    phases: &[Phase],
    provenance: Provenance,
    discovered_by: DiscoveredBy,
) -> Result<PatternRecord, String> {
    let mut rec = build_record_with(topo, None, what, phases, provenance, discovered_by)?;
    // A candidate that later fails verification may have no consistent range.
    rec.range = range::rule_range(&rec).ok().flatten();
    Ok(rec)
}

/// [`build_record`] without the rule range, with the tiling's symmetries given (lattices) so a
/// hunter that builds many records computes them once.
pub fn build_record_with(
    topo: &Topology,
    syms: Option<&[CellMap]>,
    what: &RecordParts,
    phases: &[Phase],
    provenance: Provenance,
    discovered_by: DiscoveredBy,
) -> Result<PatternRecord, String> {
    let RecordParts {
        class, period, d, ..
    } = *what;
    // Puffers keep only their starting phase (their later phases include the debris).
    let want = if class == PatternClass::Puffer {
        1
    } else {
        period
    };
    if phases.len() as u64 != want || phases.is_empty() {
        return Err(format!("{} phases for period {period}", phases.len()));
    }
    let moving = matches!(class, PatternClass::Spaceship | PatternClass::Puffer);
    let rule = what.rule.clone();
    let isotropic = what.isotropic;
    let now = now_utc();
    let tiling = tiling_identity(&topo.spec);
    let (phases_c, d, canonical, site, tile_types, disp_xy) = match topo.lattice() {
        Some(g) => {
            let own;
            let syms = match syms {
                Some(s) => s,
                None => {
                    own = canonical_symmetries(topo, what.nb, isotropic);
                    &own
                }
            };
            let (ph, d) = canonical_phases(phases, d, syms);
            let text = crate::objects::picture(&ph[0], g.ntypes);
            let xy = [
                d[0] as f64 * g.a1[0] + d[1] as f64 * g.a2[0],
                d[0] as f64 * g.a1[1] + d[1] as f64 * g.a2[1],
            ];
            (ph, d, text, None, g.ntypes, xy)
        }
        None => {
            let rule = crate::rules::Rule::parse(&what.rule)?.0;
            let (site, ph, text) = site::build_site_for(topo, what.nb, &rule, phases)?;
            (ph, [0, 0], text, Some(site), 0, [0.0, 0.0])
        }
    };
    let counts: Vec<usize> = phases_c.iter().map(|p| p.len()).collect();
    let p0 = &phases_c[0];
    let bbox = [
        0,
        0,
        p0.iter().map(|c| c.i).max().unwrap_or(0) - p0.iter().map(|c| c.i).min().unwrap_or(0),
        p0.iter().map(|c| c.j).max().unwrap_or(0) - p0.iter().map(|c| c.j).min().unwrap_or(0),
    ];
    // Speed from the displacement in the canonical orientation (the same ship found in any
    // orientation gets the same description).
    let speed = moving
        .then(|| {
            ObjClass::Spaceship {
                period,
                dx: d[0],
                dy: d[1],
            }
            .speed()
        })
        .flatten();
    let mut rec = PatternRecord {
        format: RECORD_FORMAT,
        id: String::new(),
        name: what.name.clone(),
        rule,
        rules: vec![],
        range: None,
        neighborhood: what.nb,
        steps_per_gen: what.steps_per_gen,
        tiling_name: tiling_display_name(&tiling),
        tiling,
        tile_types,
        class,
        period,
        displacement: moving.then_some(d),
        displacement_xy: moving.then_some([
            (disp_xy[0] * 1e6).round() / 1e6,
            (disp_xy[1] * 1e6).round() / 1e6,
        ]),
        speed,
        cells: p0.len(),
        min_cells: counts.iter().copied().min().unwrap_or(0),
        max_cells: counts.iter().copied().max().unwrap_or(0),
        bbox,
        canonical,
        phases: phases_c.iter().map(|p| encode_cells(p)).collect(),
        site,
        verified: Verification::default(),
        mover: None,
        gun: None,
        seen: 1,
        provenance,
        discovered_by,
        tags: vec![],
        notes: String::new(),
        score: 0.0,
        rating: None,
        state: CurationState::New,
        created: now.clone(),
        updated: now,
    };
    rec.id = identity(&rec)?;
    rec.rescore();
    Ok(rec)
}

/// For a spaceship: the lattice vector it moves per period in each orientation (index = the
/// `orientation` of `PastePattern`). Empty for other classes and aperiodic patterns.
pub fn orientation_moves(rec: &PatternRecord) -> Result<Vec<[i32; 2]>, String> {
    let (Some(d), None) = (rec.displacement, &rec.site) else {
        return Ok(vec![]);
    };
    let mut t = tiling_identity(&rec.tiling);
    t.size = Some(crate::topology::SizeSpec::Rect { w: 8, h: 8 });
    let topo = Topology::build(&t)?;
    Ok(paste::orientations(&topo, rec.neighborhood)
        .iter()
        .map(|m| m.vector(d))
        .collect())
}

// ---------------------------------------------------------------------------------------------
// ASCII drawings (for listings and text-only curators)

/// A text drawing of phase `t`: rows of `.` and `O` (digits for higher states) on the square
/// grid; on the hexagon grid each row is shifted half a cell per row, cells two characters apart.
/// Other tilings: `None`.
pub fn ascii(rec: &PatternRecord, t: u64) -> Result<Option<String>, String> {
    if rec.tile_types != 1 || rec.site.is_some() {
        return Ok(None);
    }
    let shear = match rec.tiling.family.as_str() {
        "square" => None,
        "hex" => Some(verify::lattice_info(&rec.tiling, rec.neighborhood)?.shear),
        _ => return Ok(None),
    };
    let mut cells = rec.phase(t)?;
    let mi = cells.iter().map(|c| c.i).min().unwrap_or(0);
    let mj = cells.iter().map(|c| c.j).min().unwrap_or(0);
    for c in &mut cells {
        c.i -= mi;
        c.j -= mj;
    }
    let ch = |s: u8| -> char {
        match s {
            1 => 'O',
            s if s < 10 => (b'0' + s) as char,
            s => (b'A' + (s - 10).min(25)) as char,
        }
    };
    let h = cells.iter().map(|c| c.j).max().unwrap_or(0) + 1;
    let mut out = String::new();
    match shear {
        None => {
            let w = cells.iter().map(|c| c.i).max().unwrap_or(0) + 1;
            let mut rows = vec![vec!['.'; w as usize]; h as usize];
            for c in &cells {
                rows[c.j as usize][c.i as usize] = ch(c.s);
            }
            for r in rows {
                out.extend(r);
                out.push('\n');
            }
        }
        Some(s) => {
            // Column in half cells: 2 i + 2 s j.
            let col = |c: &LCell| -> i32 { 2 * c.i + (2.0 * s * c.j as f64).round() as i32 };
            let lo = (0..h)
                .map(|j| (2.0 * s * j as f64).round() as i32)
                .min()
                .unwrap_or(0)
                .min(cells.iter().map(col).min().unwrap_or(0));
            let hi = cells.iter().map(col).max().unwrap_or(0);
            let w = (hi - lo + 1) as usize;
            let mut rows = vec![vec![' '; w]; h as usize];
            for (j, row) in rows.iter_mut().enumerate() {
                // Empty cell positions in this row.
                let off = (2.0 * s * j as f64).round() as i32 - lo;
                let mut x = off.rem_euclid(2);
                while (x as usize) < w {
                    row[x as usize] = '.';
                    x += 2;
                }
            }
            for c in &cells {
                rows[c.j as usize][(col(c) - lo) as usize] = ch(c.s);
            }
            for r in rows {
                let line: String = r.into_iter().collect();
                out.push_str(line.trim_end());
                out.push('\n');
            }
        }
    }
    Ok(Some(out))
}

// ---------------------------------------------------------------------------------------------
// Updates and queries (the store is in `store`)

/// Changes for [`Library::update`]. `None` leaves a field alone.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Update {
    /// New name (empty string clears it).
    #[serde(default)]
    pub name: Option<String>,
    /// Replace all tags.
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub add_tags: Vec<String>,
    #[serde(default)]
    pub remove_tags: Vec<String>,
    /// Replace the notes.
    #[serde(default)]
    pub notes: Option<String>,
    /// Append a paragraph to the notes.
    #[serde(default)]
    pub append_notes: Option<String>,
    /// 1 to 5, or 0 to clear.
    #[serde(default)]
    pub rating: Option<u8>,
    #[serde(default)]
    pub state: Option<CurationState>,
    #[serde(default)]
    pub discovered_by: Option<DiscoveredBy>,
}

/// Filters for [`Library::query`]. Every set field must match.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Query {
    /// Patterns verified in a rule whose notation is this or contains it (case-insensitive;
    /// `B3/S23` matches `b3/s23`).
    #[serde(default)]
    pub rule: Option<String>,
    /// Patterns that work in this rule: its notation is inside the pattern's rule range (B/S and
    /// Generations), or the pattern was verified in it. A `V` suffix (or `neighborhood`) picks
    /// edge neighbours; B/S rules default to touching neighbours.
    #[serde(default)]
    pub works_in: Option<String>,
    /// Only this neighbourhood (`touch`, `edge`).
    #[serde(default)]
    pub neighborhood: Option<Nb>,
    /// Tiling family id (`hex`), catalog id (`k2-001`), or a substring of the tiling name.
    #[serde(default)]
    pub tiling: Option<String>,
    #[serde(default)]
    pub class: Option<PatternClass>,
    #[serde(default)]
    pub min_period: Option<u64>,
    #[serde(default)]
    pub max_period: Option<u64>,
    #[serde(default)]
    pub min_score: Option<f64>,
    #[serde(default)]
    pub state: Option<CurationState>,
    #[serde(default)]
    pub tag: Option<String>,
    /// Substring of the name or label.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub discovered_by: Option<DiscoveredBy>,
    /// Sort order: `score` (default, highest first), `seen`, `rare` (fewest sightings first),
    /// `newest`, `updated`, `cells`, `period`, `rating`, `id`.
    #[serde(default)]
    pub order: Option<String>,
    /// At most this many (after sorting).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Skip this many first (paging).
    #[serde(default)]
    pub offset: Option<usize>,
}

impl Query {
    /// The same test as the catalog's SQL, on a record in memory (ignores order, limit, offset).
    pub fn matches(&self, r: &PatternRecord) -> bool {
        let norm = |s: &str| s.to_ascii_lowercase().replace(' ', "");
        let rules = || std::iter::once(&r.rule).chain(r.rules.iter());
        if let Some(q) = &self.rule {
            let a = norm(q);
            if !rules().any(|b| norm(b).contains(&a)) {
                return false;
            }
        }
        if self.neighborhood.is_some_and(|n| n != r.neighborhood) {
            return false;
        }
        if let Some(q) = &self.works_in {
            let Ok((rule, nb)) = crate::rules::Rule::parse(q) else {
                return false;
            };
            let n = rule.notation();
            let exact = rules().any(|b| *b == n);
            let ok = match range::bs_parts(&rule) {
                Some(_) => {
                    self.neighborhood.or(nb).unwrap_or(Nb::Touch) == r.neighborhood
                        && (exact || r.range.as_ref().is_some_and(|x| x.contains(&rule)))
                }
                None => exact,
            };
            if !ok {
                return false;
            }
        }
        if let Some(q) = &self.tiling {
            let q = q.to_ascii_lowercase();
            let cat = r
                .tiling
                .params
                .as_ref()
                .and_then(|p| p.get("catalog"))
                .and_then(|v| v.as_str());
            let qc = q.trim_start_matches("catalog:");
            // A family or catalog id matches exactly ("hex" is not "trihex"); other text matches
            // part of the tiling name.
            let ok = if crate::families::family(&q).is_some() || crate::catalog::get(qc).is_some() {
                (r.tiling.family.eq_ignore_ascii_case(&q) && cat.is_none())
                    || cat.is_some_and(|c| c.eq_ignore_ascii_case(qc))
            } else {
                r.tiling_name.to_ascii_lowercase().contains(&q)
            };
            if !ok {
                return false;
            }
        }
        if self.class.is_some_and(|c| c != r.class)
            || self.min_period.is_some_and(|p| r.period < p)
            || self.max_period.is_some_and(|p| r.period > p)
            || self.min_score.is_some_and(|s| r.score < s)
            || self.state.is_some_and(|s| s != r.state)
            || self.discovered_by.is_some_and(|d| d != r.discovered_by)
        {
            return false;
        }
        if let Some(t) = &self.tag
            && !r.tags.iter().any(|x| x.eq_ignore_ascii_case(t))
        {
            return false;
        }
        if let Some(t) = &self.text
            && !r
                .label()
                .to_ascii_lowercase()
                .contains(&t.to_ascii_lowercase())
        {
            return false;
        }
        true
    }
}

/// Highest score first, then rating, then id.
pub fn sort_records(v: &mut [PatternRecord]) {
    v.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.rating.cmp(&a.rating))
            .then(a.id.cmp(&b.id))
    });
}

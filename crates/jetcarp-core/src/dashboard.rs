//! The hunt dashboard's logic (the app's *Hunts…* window draws it): every catalog job as a
//! [`JobSummary`], pause, resume, and stop across processes, the rule-space heat map, and the
//! finds to show, animate, and rate.
//!
//! **Job kinds.** Everything kind-specific (a job's summary from its saved progress, how to
//! stop and run it, its heat maps and finds) is the job runner's `crate::jobs::StrategyKind`;
//! [`summaries`] asks each job's kind ([`crate::jobs::kind`]). Jobs running in this process are
//! `crate::jobs::JobHandle`s in the process's `crate::jobs::Registry`; a handle's
//! [`summary`](crate::jobs::JobHandle::summary) is the live one.
//!
//! **Heat map** ([`heat_map`]). Life-like rules on one tiling and neighbourhood, birth mask
//! across (B1..B8, B0 is never searched) and survival mask down (S0..S8), each axis in Gray-code
//! order: neighbouring columns (rows) differ in exactly one birth (survival) count, so the
//! rule-space neighbours of a rule (one count added or removed) are mostly beside it and
//! interesting rules show up as clusters rather than scattered dots. Squares with Moore
//! neighbours make 256 x 512 cells, hexagons 64 x 128, von Neumann squares 16 x 32. Tilings with
//! more than 8 neighbours fold the higher counts onto the cell of their first eight (the map
//! says so). A cell's value is the sum over its rules (finds, spaceships).

use crate::census;
use crate::library::store::JobRow;
use crate::library::{HeatQuery, HeatRow, JobStatus, Library, QsStatus, Update};
use crate::pattern::{CurationState, tiling_key};
use crate::topology::{Nb, SizeSpec, TilingSpec, Topology};
use serde::Serialize;
use std::collections::BTreeMap;

/// A running job that has not written its progress (nor, run by the job runner, its heartbeat)
/// for this long is shown as stalled: its process probably ended without marking it, and it may
/// be resumed here. (The runner's lock lets a crashed job be resumed at once; this is what the
/// list shows.)
pub const STALE_S: i64 = 300;

/// One job as the dashboard shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct JobSummary {
    pub id: i64,
    pub kind: String,
    /// `Hunt`, `Census`, `Ant search`, ...
    pub kind_label: String,
    /// `running`, `paused`, `done`, `failed`.
    pub status: String,
    /// Running, but no progress or heartbeat written for [`STALE_S`] seconds.
    pub stale: bool,
    /// Running in this process.
    pub here: bool,
    /// Rules and tilings, or spaces.
    pub title: String,
    /// Units done and in all (`None`: runs until stopped), and what a unit is.
    pub done: u64,
    pub total: Option<u64>,
    pub unit: String,
    /// Units per second in the last session.
    pub per_s: f64,
    pub elapsed_s: f64,
    /// Seconds left, when the job is running and has an end.
    pub eta_s: Option<f64>,
    /// Counters to show (`settled 1,204`), in order.
    pub counters: Vec<(String, String)>,
    /// Bars to show (finds per class, a score histogram), in order.
    pub bars: Vec<(String, u64)>,
    /// What the bars are.
    pub bars_title: String,
    pub created: String,
    pub updated: String,
    pub error: Option<String>,
    /// The kind can run it here (pause, then resume).
    pub resumable: bool,
    /// Stopped and resumed, it ends exactly as an uninterrupted run; otherwise (an explore
    /// search) a resumed run continues from its saved state.
    pub deterministic: bool,
}

impl JobSummary {
    /// The fraction done, if the job has an end.
    pub fn fraction(&self) -> Option<f64> {
        self.total
            .filter(|&t| t > 0)
            .map(|t| (self.done as f64 / t as f64).clamp(0.0, 1.0))
    }
    /// Can be resumed in this process now.
    pub fn can_resume(&self) -> bool {
        self.resumable
            && !self.here
            && (matches!(self.status.as_str(), "paused" | "failed") || self.stale)
    }
    /// Running somewhere (here or in another process) and not stalled.
    pub fn is_running(&self) -> bool {
        self.status == "running" && !self.stale
    }
}

/// A heat map a job feeds: one tiling, neighbourhood, and state count.
#[derive(Clone, Debug, PartialEq)]
pub struct HeatSpace {
    pub tiling: TilingSpec,
    pub tiling_name: String,
    pub nb: Nb,
    pub states: u32,
    pub query: HeatQuery,
}

impl HeatSpace {
    pub fn label(&self) -> String {
        format!(
            "{} ({}){}",
            self.tiling_name,
            match self.nb {
                Nb::Touch => "touch",
                Nb::Edge => "edge",
            },
            if self.states > 2 {
                format!(" C{}", self.states)
            } else {
                String::new()
            }
        )
    }
}

/// What a find is, for rating and rejecting.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Pattern(String),
}

/// A find to show (and animate, rate, reject).
#[derive(Clone, Debug, PartialEq)]
pub struct FindItem {
    pub target: Target,
    pub title: String,
    pub subtitle: String,
    /// The rule it was found in.
    pub rule: String,
    /// The user's rating.
    pub rating: Option<u8>,
    pub rejected: bool,
}

/// Newest and rarest finds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Finds {
    pub newest: Vec<FindItem>,
    pub other: Vec<FindItem>,
    /// `Rarest`.
    pub other_title: String,
}

/// Every job but the "manual" job 0, newest first, with `stale` set for running jobs whose
/// progress and heartbeat are older than [`STALE_S`] at `now` (seconds since 1970).
pub fn summaries(rows: &[JobRow], now: i64) -> Vec<JobSummary> {
    let mut v: Vec<JobSummary> = rows
        .iter()
        .filter(|r| r.id != 0 && r.kind != "manual")
        .map(|r| {
            let mut s = crate::jobs::kind(&r.kind).summary(r);
            if let Some(l) = r.label() {
                s.title = if s.title.is_empty() {
                    l.to_string()
                } else {
                    format!("{l}: {}", s.title)
                };
            }
            let alive = utc_seconds(&r.updated).max(r.heartbeat);
            s.stale = r.status == JobStatus::Running && alive.is_some_and(|t| now - t > STALE_S);
            s
        })
        .collect();
    v.sort_by_key(|s| std::cmp::Reverse(s.id));
    v
}

/// Seconds since 1970 now.
pub fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `YYYY-MM-DDTHH:MM:SSZ` (as `pattern::now_utc` writes) as seconds since 1970.
pub fn utc_seconds(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |a: usize, n: usize| -> Option<i64> { s.get(a..a + n)?.parse().ok() };
    let (y, m, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hh, mm, ss) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    // Howard Hinnant's days-from-civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

/// `1,234,567`.
pub fn digits(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// `3 d 4 h`, `12 min 5 s`, `40 s`.
pub fn duration(s: f64) -> String {
    let s = s.max(0.0).round() as u64;
    let (d, h, m, sec) = (s / 86400, s / 3600 % 24, s / 60 % 60, s % 60);
    if d > 0 {
        format!("{d} d {h} h")
    } else if h > 0 {
        format!("{h} h {m} min")
    } else if m > 0 {
        format!("{m} min {sec} s")
    } else {
        format!("{sec} s")
    }
}

// ---------------------------------------------------------------------------------------------
// Actions on finds

/// Rate a find as the user (1 to 5): a `ratings` row with rater `user`, and for a pattern its
/// own `rating` too.
pub fn rate(lib: &Library, t: &Target, stars: u8) -> Result<(), String> {
    match t {
        Target::Pattern(id) => lib.rate_pattern(id, stars, "").map(|_| ()),
    }
}

/// Reject a find: a pattern's curation state becomes `rejected` (and its Quick Start entry, if
/// it has one, is hidden).
pub fn reject(lib: &Library, t: &Target) -> Result<(), String> {
    match t {
        Target::Pattern(id) => {
            let r = lib.update(
                id,
                &Update {
                    state: Some(CurationState::Rejected),
                    ..Default::default()
                },
            )?;
            if lib.get_quickstart(&r.id).is_ok() {
                lib.set_quickstart_status(&r.id, QsStatus::Rejected)?;
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Heat map

/// What colours the heat map.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HeatColour {
    /// Distinct patterns found.
    #[default]
    Finds,
    /// Distinct spaceships found.
    Ships,
}

impl HeatColour {
    pub const ALL: [(HeatColour, &'static str); 2] = [
        (HeatColour::Finds, "finds"),
        (HeatColour::Ships, "spaceships"),
    ];
    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|x| x.0 == self)
            .map(|x| x.1)
            .unwrap_or("")
    }
    /// The value of a row.
    pub fn value(self, r: &HeatRow) -> f64 {
        match self {
            HeatColour::Finds => r.finds as f64,
            HeatColour::Ships => r.ships as f64,
        }
    }
}

/// The binary-reflected Gray code of `n`.
pub fn gray(n: u32) -> u32 {
    n ^ (n >> 1)
}

/// The `n` whose Gray code is `g`.
pub fn gray_inverse(g: u32) -> u32 {
    let mut n = g;
    let mut s = 1;
    while s < 32 {
        n ^= n >> s;
        s <<= 1;
    }
    n
}

/// Birth and survival bits shown for a neighbour count (`degree`): B1..B8 and S0..S8 at most.
pub fn heat_bits(degree: u32) -> (u32, u32) {
    (degree.clamp(1, 8), (degree + 1).clamp(1, 9))
}

/// The cell `(x, y)` of a rule's birth and survival masks (bit `k` = count `k`).
pub fn heat_cell(birth: u32, survive: u32, degree: u32) -> (usize, usize) {
    let (bb, sb) = heat_bits(degree);
    let b = (birth >> 1) & ((1 << bb) - 1);
    let s = survive & ((1 << sb) - 1);
    (gray_inverse(b) as usize, gray_inverse(s) as usize)
}

/// The birth and survival masks of cell `(x, y)` (the shown bits only).
pub fn heat_masks(x: usize, y: usize, degree: u32) -> (u32, u32) {
    let (bb, sb) = heat_bits(degree);
    let b = gray(x as u32) & ((1 << bb) - 1);
    let s = gray(y as u32) & ((1 << sb) - 1);
    (b << 1, s)
}

/// A binned heat map.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeatMap {
    pub degree: u32,
    /// Columns (2^birth bits) and rows (2^survival bits).
    pub w: usize,
    pub h: usize,
    /// Row-major, `w * h`.
    pub values: Vec<f64>,
    /// Rows binned into each cell (indices into the rows given), only for cells with any.
    pub rules: std::collections::HashMap<usize, Vec<usize>>,
    pub max: f64,
    /// Counts above 8 were folded onto the first eight.
    pub folded: bool,
    pub colour: HeatColour,
}

impl HeatMap {
    pub fn at(&self, x: usize, y: usize) -> f64 {
        if x < self.w && y < self.h {
            self.values[y * self.w + x]
        } else {
            0.0
        }
    }
    pub fn rules_at(&self, x: usize, y: usize) -> &[usize] {
        self.rules
            .get(&(y * self.w + x))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// Bin rules into the B x S map (see the module notes).
pub fn heat_map(rows: &[HeatRow], degree: u32, colour: HeatColour) -> HeatMap {
    let (bb, sb) = heat_bits(degree);
    let (w, h) = (1usize << bb, 1usize << sb);
    let mut m = HeatMap {
        degree,
        w,
        h,
        values: vec![0.0; w * h],
        folded: degree > 8,
        colour,
        ..Default::default()
    };
    for (i, r) in rows.iter().enumerate() {
        let (x, y) = heat_cell(r.birth, r.survive, degree);
        let k = y * w + x;
        m.values[k] += colour.value(r);
        m.rules.entry(k).or_default().push(i);
    }
    m.max = m.values.iter().cloned().fold(0.0, f64::max);
    m
}

/// The largest neighbour count on a tiling (from a small copy of it); falls back to the
/// highest count used by `rows`.
pub fn tiling_degree(t: &TilingSpec, nb: Nb, rows: &[HeatRow]) -> u32 {
    let mut small = t.clone();
    small.size = Some(SizeSpec::Rect { w: 8, h: 8 });
    let built = Topology::build(&small)
        .map(|topo| census::max_degree(&topo, nb) as u32)
        .unwrap_or(0);
    let used = rows
        .iter()
        .map(|r| 32 - (r.birth | r.survive).leading_zeros())
        .max()
        .unwrap_or(0)
        .saturating_sub(1);
    built.max(used).max(1)
}

/// Read and bin one heat map.
pub fn load_heat(
    lib: &Library,
    space: &HeatSpace,
    colour: HeatColour,
) -> Result<(HeatMap, Vec<HeatRow>), String> {
    let rows = lib.heat_rows(
        &space.query,
        &tiling_key(&space.tiling),
        space.nb,
        space.states,
    )?;
    let degree = tiling_degree(&space.tiling, space.nb, &rows);
    Ok((heat_map(&rows, degree, colour), rows))
}

/// Finds per class over all jobs, for the job list's footer.
pub fn class_totals(rows: &[JobSummary]) -> BTreeMap<String, u64> {
    let mut m = BTreeMap::new();
    for r in rows.iter().filter(|r| r.kind == "hunt") {
        for (k, v) in &r.bars {
            *m.entry(k.clone()).or_default() += v;
        }
    }
    m
}

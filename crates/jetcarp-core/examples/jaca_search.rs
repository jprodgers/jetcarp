//! Long-running search for JACA presets on every kind of grid, with every run kept in SQLite.
//!
//! ```text
//! cargo run --release -p jetcarp-core --example jaca_search -- run DB.sqlite HOURS
//! cargo run --release -p jetcarp-core --example jaca_search -- sheet DB.sqlite "SQL" OUT.png
//! cargo run --release -p jetcarp-core --example jaca_search -- tilings
//! ```
//!
//! `run` loops until HOURS have passed. Each batch is a mix of:
//! - fresh random rule pairs on a random tiling;
//! - mutations of the best configurations so far (a count toggled, the blast, the seed);
//! - crossovers: one good configuration's +1 rule against another's -1 rule;
//! - transplants: a good pair moved to another tiling with similar neighbour counts;
//! - confirmations: the best unconfirmed configurations rerun on a bigger grid, longer, with
//!   three RNG seeds (stage `confirm`, scored by the worst seed).
//!
//! A configuration is a tiling, a neighbourhood (touch or edge), a JACA rule (a B/S rule per sign
//! and a blast radius), and a start (a mixed soup at some density, or halves). Every run's
//! measurements go to the `runs` table. `sheet` renders the configurations a query returns (it
//! must select `id`) at three generations each, for looking at them.
//!
//! Scores: `score` rewards a lasting battle (both signs alive and balanced, contact along fronts,
//! territories that change hands, activity); `conquest` rewards a long, contested war that one
//! sign wins in the end. The search breeds from the better of the two.

use jetcarp_core::jaca::{JacaRule, NEG, POS};
use jetcarp_core::render::{Image, RenderOptions, render_fitted, write_png};
use jetcarp_core::rng::Rng;
use jetcarp_core::topology::{Nb, SizeSpec, Topology};
use jetcarp_core::{Boundary, CaSpec, Command, SeedSpec, TilingSpec, World};
use rayon::prelude::*;
use rusqlite::{Connection, params};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Tilings searched, with a sampling weight.
const TILINGS: &[(&str, f64)] = &[
    ("square", 3.0),
    ("hex", 3.0),
    ("triangle", 3.0),
    ("cairo", 1.5),
    ("oct", 1.0),
    ("trihex", 1.5),
    ("elongtri", 1.0),
    ("snubsq", 1.0),
    ("snubhex", 1.0),
    ("rhombitrihex", 1.5),
    ("trunchex", 0.7),
    ("trunctrihex", 1.0),
    ("herring", 0.7),
    ("pyth", 0.7),
    ("brick", 0.5),
    ("lizard", 0.7),
    ("catalog:k2-001", 0.7),
    ("catalog:k3-011", 0.7),
    ("ih28", 0.5),
    ("ih41", 0.5),
    ("penrose-rhomb", 1.5),
    ("penrose-kd", 1.0),
    ("spectre", 1.5),
    ("hat", 1.0),
];

const SCREEN_CELLS: u32 = 16_000;
const SCREEN_GENS: u64 = 1500;
const CONFIRM_CELLS: u32 = 30_000;
const CONFIRM_GENS: u64 = 4000;

struct Grid {
    topo: Arc<Topology>,
    /// Spatial bin per cell (about 8 tiles across) and the bin count.
    bins: Vec<u32>,
    nbins: usize,
    /// Largest neighbour count and mean neighbour count, per neighbourhood (touch, edge).
    max_deg: [usize; 2],
    mean_deg: [f64; 2],
    /// Touch and edge differ.
    has_edge: bool,
}

fn base_spec(key: &str) -> TilingSpec {
    if let Some(id) = key.strip_prefix("catalog:") {
        let mut spec = TilingSpec::new("periodic");
        spec.params = Some(serde_json::json!({ "catalog": id }));
        return spec;
    }
    TilingSpec::new(key)
}

fn aperiodic(key: &str) -> bool {
    matches!(key, "penrose-rhomb" | "penrose-kd" | "spectre" | "hat")
}

/// The tiling spec for `key` at about `cells` cells.
fn sized_spec(key: &str, cells: u32) -> Result<(TilingSpec, Arc<Topology>), String> {
    let base = base_spec(key);
    if aperiodic(key) {
        let mut level = jetcarp_core::aperiodic::default_level(key);
        loop {
            let spec = base
                .clone()
                .with_size(SizeSpec::Level(level))
                .with_boundary(Boundary::Dead);
            let topo = Topology::build(&spec)?;
            if topo.len() as u32 >= cells * 3 / 4 || level > 12 {
                return Ok((spec, Arc::new(topo)));
            }
            level += 1;
        }
    }
    let spec = base
        .with_size(SizeSpec::Cells(cells))
        .with_boundary(Boundary::Wrap);
    let topo = Topology::build(&spec)?;
    Ok((spec, Arc::new(topo)))
}

fn grid(key: &str, cells: u32) -> Result<Grid, String> {
    let (_, topo) = sized_spec(key, cells)?;
    let n = topo.len();
    let tile = jetcarp_core::world::cell_size(&topo);
    let b = topo.bounds;
    let size = 8.0 * tile;
    let bw = (((b[2] - b[0]) / size).ceil() as usize).max(1);
    let bh = (((b[3] - b[1]) / size).ceil() as usize).max(1);
    let bins: Vec<u32> = (0..n)
        .map(|i| {
            let p = topo.centroid(i);
            let x = (((p[0] - b[0]) / size) as usize).min(bw - 1);
            let y = (((p[1] - b[1]) / size) as usize).min(bh - 1);
            (y * bw + x) as u32
        })
        .collect();
    let mut max_deg = [0; 2];
    let mut mean_deg = [0.0; 2];
    for (k, nb) in [Nb::Touch, Nb::Edge].into_iter().enumerate() {
        let mut sum = 0usize;
        for i in 0..n {
            let d = topo.degree(i, nb);
            max_deg[k] = max_deg[k].max(d);
            sum += d;
        }
        mean_deg[k] = sum as f64 / n.max(1) as f64;
    }
    let has_edge = max_deg[0] != max_deg[1] || (mean_deg[0] - mean_deg[1]).abs() > 0.01;
    Ok(Grid {
        topo,
        bins,
        nbins: bw * bh,
        max_deg: [max_deg[0].min(31), max_deg[1].min(31)],
        mean_deg,
        has_edge,
    })
}

#[derive(Clone, Debug, PartialEq)]
struct Cfg {
    tiling: String,
    edge: bool,
    rule: JacaRule,
    /// "mixed" or "halves".
    seed: String,
    density: f64,
}

impl Cfg {
    fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{:.2}",
            self.tiling,
            if self.edge { "edge" } else { "touch" },
            self.rule.notation(),
            self.seed,
            self.density
        )
    }
    fn seed_spec(&self) -> SeedSpec {
        if self.seed == "halves" {
            SeedSpec::Halves {
                density: self.density,
            }
        } else {
            SeedSpec::Signed {
                density: self.density,
                negative: 0.5,
            }
        }
    }
    fn world(&self, g: &Grid, rng: u64) -> World {
        let mut w = World::on_topology(g.topo.clone(), rng);
        let mut ca = CaSpec::new(&self.rule.notation()).seed(self.seed_spec());
        if self.edge {
            ca.neighborhood = Some(Nb::Edge);
        }
        if let Err(e) = w.apply(Command::AddCa { ca }) {
            panic!("{}: {e} ({:?})", self.rule.notation(), self.rule);
        }
        w.set_track_changes(true);
        w
    }
}

#[derive(Clone, Debug, Default)]
struct Metrics {
    both: f64,
    balance: f64,
    live: f64,
    front: f64,
    activity: f64,
    territory: f64,
    turnover: f64,
    swing: f64,
    leads: u32,
    extinct: u64,
    /// Generations until one sign held less than a tenth of the live cells (or the run's end).
    war_len: u64,
    /// Share of live cells older than 30 generations (second half): still rules colour by age.
    still: f64,
    pos: f64,
    neg: f64,
}

impl Metrics {
    /// `halves`: the run started from halves, where a front that does not move is dull, so the
    /// score scales with how much territory changes hands.
    fn score(&self, halves: bool) -> f64 {
        if self.both < 0.95 || self.activity < 0.002 || self.front < 2e-4 || self.live < 0.02 {
            return 0.0;
        }
        let moving = (self.turnover * 10.0).min(1.0);
        let swing = (self.swing * 4.0).min(1.0);
        let leads = (self.leads as f64 / 4.0).min(1.0);
        let base = self.balance.sqrt()
            * (self.front * 200.0).min(1.0).sqrt()
            * self.territory
            * (0.3 + 0.5 * moving + 0.1 * swing + 0.1 * leads)
            * (self.activity * 10.0).min(1.0).sqrt();
        if halves {
            base * (self.turnover / 0.12).min(1.0)
        } else {
            base
        }
    }
    /// A long, contested war that one sign wins.
    fn conquest(&self, gens: u64) -> f64 {
        if self.extinct == 0 || self.war_len < gens / 5 {
            return 0.0;
        }
        (self.war_len as f64 / gens as f64).min(1.0).sqrt()
            * (self.front * 200.0).min(1.0).sqrt()
            * (self.activity * 10.0).min(1.0).sqrt()
            * (0.4 + 0.6 * (self.turnover * 25.0).min(1.0))
    }
}

fn measure(cfg: &Cfg, g: &Grid, gens: u64, rng: u64) -> Metrics {
    let mut w = cfg.world(g, rng);
    let n = g.topo.len() as f64;
    let mut m = Metrics::default();
    let mut samples = 0.0;
    let mut diffs = vec![];
    let mut lead = 0i32;
    let mut owners: std::collections::VecDeque<Vec<i8>> = Default::default();
    let mut turn_n = 0.0;
    let mut still_n = 0.0;
    let step = 10;
    let mut t = 0;
    let mut war_over = false;
    while t < gens {
        w.apply(Command::Step { gens: step }).unwrap();
        t += step;
        let f = &w.fields[0];
        let mut blocks = vec![(0u32, 0u32); g.nbins];
        let (mut p, mut q, mut old) = (0usize, 0usize, 0usize);
        for (i, &c) in f.state.iter().enumerate() {
            if c == 0 {
                continue;
            }
            let b = &mut blocks[g.bins[i] as usize];
            if c == POS {
                p += 1;
                b.0 += 1;
            } else if c == NEG {
                q += 1;
                b.1 += 1;
            }
            if f.age[i] > 30 {
                old += 1;
            }
        }
        let (pf, nf) = (p as f64 / n, q as f64 / n);
        if p + q > 0 {
            let d = (pf - nf) / (pf + nf);
            let now = if d > 0.1 {
                1
            } else if d < -0.1 {
                -1
            } else {
                lead
            };
            if lead != 0 && now != lead {
                m.leads += 1;
            }
            lead = now;
            if !war_over && (p.min(q) as f64) < 0.1 * (p + q) as f64 && t > 50 {
                war_over = true;
                m.war_len = t;
            }
        }
        if m.extinct == 0 && (p == 0 || q == 0) {
            m.extinct = t;
            if !war_over {
                war_over = true;
                m.war_len = t;
            }
        }
        m.pos = pf;
        m.neg = nf;
        if p + q == 0 {
            break;
        }
        if t > gens / 2 {
            samples += 1.0;
            if pf >= 0.005 && nf >= 0.005 {
                m.both += 1.0;
            }
            m.balance += pf.min(nf) / pf.max(nf).max(1e-12);
            m.live += pf + nf;
            let heat = w.cas[0].heat.as_ref().map(|h| h.last).unwrap_or_default();
            m.front += heat.contacts as f64 / n;
            m.activity += w.last_changed().unwrap_or(0) as f64 / n;
            if p + q > 0 {
                m.still += old as f64 / (p + q) as f64;
                still_n += 1.0;
            }
            let (mut tt, mut tw) = (0.0, 0.0);
            let own: Vec<i8> = blocks
                .iter()
                .map(|&(a, b)| {
                    let s = (a + b) as f64;
                    if a + b >= 4 {
                        tt += (a as f64 - b as f64).abs();
                        tw += s;
                    }
                    if a + b < 4 || (a as f64 - b as f64).abs() < 0.3 * s {
                        0
                    } else if a > b {
                        1
                    } else {
                        -1
                    }
                })
                .collect();
            m.territory += if tw > 0.0 { tt / tw } else { 0.0 };
            if owners.len() == 10 {
                let o = owners.pop_front().unwrap();
                let (mut ch, mut ow) = (0.0, 0.0);
                for (x, y) in o.iter().zip(&own) {
                    if *x != 0 || *y != 0 {
                        ow += 1.0;
                        if x != y {
                            ch += 1.0;
                        }
                    }
                }
                m.turnover += ch / f64::max(ow, 1.0);
                turn_n += 1.0;
            }
            owners.push_back(own);
            diffs.push((pf - nf) / (pf + nf).max(1e-12));
        }
    }
    if !war_over {
        m.war_len = gens;
    }
    if samples > 0.0 {
        m.both /= samples;
        m.balance /= samples;
        m.live /= samples;
        m.front /= samples;
        m.activity /= samples;
        m.territory /= samples;
        m.turnover /= f64::max(turn_n, 1.0);
        m.still /= f64::max(still_n, 1.0);
        let mean = diffs.iter().sum::<f64>() / diffs.len().max(1) as f64;
        m.swing = (diffs.iter().map(|d| (d - mean) * (d - mean)).sum::<f64>()
            / diffs.len().max(1) as f64)
            .sqrt();
    }
    m
}

// ---------------------------------------------------------------------------------------------
// Sampling

/// Smallest birth count allowed (`JACA_MIN_BIRTH`, default 1): 3 keeps the search to rules
/// where Life-like structures can form.
fn min_birth() -> u32 {
    std::env::var("JACA_MIN_BIRTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
}

fn birth_ok(b: u32) -> bool {
    b != 0 && b & ((1u32 << min_birth()) - 1) == 0
}

fn random_rule(r: &mut Rng, max: usize, mean: f64) -> (u32, u32) {
    loop {
        let (mut b, mut s) = (0u32, 0u32);
        for k in min_birth() as usize..=max {
            let kf = k as f64;
            let p = if k == 1 {
                0.06
            } else if kf <= mean * 0.45 {
                0.45
            } else if kf <= mean * 0.8 {
                0.3
            } else {
                0.12
            };
            if r.next_f64() < p {
                b |= 1 << k;
            }
        }
        for k in 0..=max {
            if r.next_f64() < 0.33 {
                s |= 1 << k;
            }
        }
        if birth_ok(b) {
            return (b, s);
        }
    }
}

fn flip_bit(r: &mut Rng, (b, s): (u32, u32), max: usize) -> (u32, u32) {
    loop {
        let (mut b2, mut s2) = (b, s);
        if r.next_f64() < 0.45 {
            let k = 1 + r.below(max as u64) as u32;
            b2 ^= 1 << k;
        } else {
            let k = r.below(max as u64 + 1) as u32;
            s2 ^= 1 << k;
        }
        if birth_ok(b2) {
            return (b2, s2);
        }
    }
}

fn clamp_rule((b, s): (u32, u32), max: usize) -> (u32, u32) {
    let m = if max >= 31 {
        u32::MAX
    } else {
        (2u32 << max) - 1
    };
    let b = b & m & !1;
    (if b == 0 { 1 << 2 } else { b }, s & m)
}

fn random_seed(r: &mut Rng) -> (String, f64) {
    if r.next_f64() < 0.2 {
        ("halves".into(), [0.2, 0.3, 0.45][r.below(3) as usize])
    } else {
        ("mixed".into(), [0.1, 0.2, 0.3, 0.45][r.below(4) as usize])
    }
}

fn env_flag(name: &str) -> Option<u64> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

fn random_blast(r: &mut Rng) -> u8 {
    if let Some(m) = env_flag("JACA_MIN_BLAST") {
        return (m + r.below(3)) as u8;
    }
    let x = r.next_f64();
    if x < 0.5 {
        0
    } else if x < 0.75 {
        1
    } else if x < 0.9 {
        2
    } else {
        3
    }
}

fn make_rule(p: (u32, u32), n: (u32, u32), blast: u8) -> JacaRule {
    JacaRule {
        pos_birth: p.0,
        pos_survive: p.1,
        neg_birth: n.0,
        neg_survive: n.1,
        blast,
    }
}

fn pick_tiling(r: &mut Rng) -> &'static str {
    // JACA_EDGE=1: only tilings where the edge neighbourhood differs from touch.
    let edge_only = env_flag("JACA_EDGE") == Some(1);
    let skip = |k: &str| {
        edge_only
            && matches!(
                k,
                "hex"
                    | "oct"
                    | "trunchex"
                    | "trunctrihex"
                    | "herring"
                    | "pyth"
                    | "brick"
                    | "lizard"
            )
    };
    loop {
        let k = pick_tiling_any(r);
        if !skip(k) {
            return k;
        }
    }
}

fn pick_tiling_any(r: &mut Rng) -> &'static str {
    let total: f64 = TILINGS.iter().map(|t| t.1).sum();
    let mut x = r.next_f64() * total;
    for (k, w) in TILINGS {
        if x < *w {
            return k;
        }
        x -= w;
    }
    TILINGS[0].0
}

fn fresh(r: &mut Rng, grids: &HashMap<String, Grid>) -> Cfg {
    let t = pick_tiling(r);
    let g = &grids[t];
    let edge = g.has_edge && (env_flag("JACA_EDGE") == Some(1) || r.next_f64() < 0.4);
    let k = edge as usize;
    let a = random_rule(r, g.max_deg[k], g.mean_deg[k]);
    let b = if r.next_f64() < 0.3 {
        a
    } else if r.next_f64() < 0.5 {
        flip_bit(r, a, g.max_deg[k])
    } else {
        random_rule(r, g.max_deg[k], g.mean_deg[k])
    };
    let (seed, density) = random_seed(r);
    Cfg {
        tiling: t.into(),
        edge,
        rule: make_rule(a, b, random_blast(r)),
        seed,
        density,
    }
}

fn mutate(r: &mut Rng, c: &Cfg, grids: &HashMap<String, Grid>) -> Cfg {
    let g = &grids[&c.tiling];
    let mut c = c.clone();
    let max = g.max_deg[c.edge as usize];
    let mut p = (c.rule.pos_birth, c.rule.pos_survive);
    let mut n = (c.rule.neg_birth, c.rule.neg_survive);
    let sym = p == n;
    let x = r.next_f64();
    if x < 0.65 {
        let flips = 1 + (r.next_f64() < 0.3) as usize;
        for _ in 0..flips {
            if sym && r.next_f64() < 0.5 {
                p = flip_bit(r, p, max);
                n = p;
            } else if r.next_f64() < 0.5 {
                p = flip_bit(r, p, max);
            } else {
                n = flip_bit(r, n, max);
            }
        }
    } else if x < 0.75 {
        c.rule.blast = if c.rule.blast == 0 || r.next_f64() < 0.5 {
            (c.rule.blast + 1).min(4)
        } else {
            c.rule.blast - 1
        };
    } else if x < 0.92 {
        let (s, d) = random_seed(r);
        c.seed = s;
        c.density = d;
    } else if g.has_edge {
        c.edge = !c.edge;
        let m = g.max_deg[c.edge as usize];
        p = clamp_rule(p, m);
        n = clamp_rule(n, m);
    } else {
        std::mem::swap(&mut p, &mut n);
    }
    c.rule = make_rule(p, n, c.rule.blast);
    c
}

fn cross(r: &mut Rng, a: &Cfg, b: &Cfg, grids: &HashMap<String, Grid>) -> Cfg {
    let g = &grids[&a.tiling];
    let max = g.max_deg[a.edge as usize];
    let pa = (a.rule.pos_birth, a.rule.pos_survive);
    let nb = if r.next_f64() < 0.5 {
        (b.rule.neg_birth, b.rule.neg_survive)
    } else {
        (b.rule.pos_birth, b.rule.pos_survive)
    };
    let mut c = a.clone();
    c.rule = make_rule(pa, clamp_rule(nb, max), a.rule.blast);
    if r.next_f64() < 0.3 {
        c.seed = "mixed".into();
    }
    c
}

fn transplant(r: &mut Rng, a: &Cfg, grids: &HashMap<String, Grid>) -> Cfg {
    let t = pick_tiling(r);
    let g = &grids[t];
    let mut c = a.clone();
    c.tiling = t.into();
    if !g.has_edge {
        c.edge = false;
    }
    let max = g.max_deg[c.edge as usize];
    c.rule = make_rule(
        clamp_rule((a.rule.pos_birth, a.rule.pos_survive), max),
        clamp_rule((a.rule.neg_birth, a.rule.neg_survive), max),
        a.rule.blast,
    );
    c
}

// ---------------------------------------------------------------------------------------------
// Database

fn open(path: &str) -> Connection {
    let db = Connection::open(path).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS runs (
            id INTEGER PRIMARY KEY,
            stage TEXT NOT NULL,
            origin TEXT,
            cfg_key TEXT NOT NULL,
            tiling TEXT NOT NULL,
            nb TEXT NOT NULL,
            rule TEXT NOT NULL,
            pos_b INTEGER, pos_s INTEGER, neg_b INTEGER, neg_s INTEGER,
            blast INTEGER NOT NULL,
            seed TEXT NOT NULL,
            density REAL NOT NULL,
            rng INTEGER NOT NULL,
            gens INTEGER NOT NULL,
            cells INTEGER NOT NULL,
            score REAL, conquest REAL,
            both REAL, balance REAL, live REAL, front REAL, activity REAL, territory REAL,
            turnover REAL, swing REAL, leads INTEGER, extinct INTEGER, war_len INTEGER,
            still REAL, pos REAL, neg REAL,
            created REAL);
         CREATE INDEX IF NOT EXISTS runs_stage_score ON runs(stage, score);
         CREATE INDEX IF NOT EXISTS runs_stage_conquest ON runs(stage, conquest);
         CREATE INDEX IF NOT EXISTS runs_key ON runs(cfg_key);
         CREATE TABLE IF NOT EXISTS confirmed (
            cfg_key TEXT PRIMARY KEY,
            tiling TEXT, nb TEXT, rule TEXT, seed TEXT, density REAL,
            score_min REAL, score_mean REAL, conquest_min REAL, conquest_mean REAL,
            war_len_mean REAL, alive INTEGER, still REAL, activity REAL, created REAL);
         CREATE TABLE IF NOT EXISTS reviews (
            cfg_key TEXT PRIMARY KEY, verdict TEXT, note TEXT, created REAL);",
    )
    .unwrap();
    db
}

fn now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

#[allow(clippy::too_many_arguments)]
fn insert(
    db: &Connection,
    stage: &str,
    origin: &str,
    c: &Cfg,
    rng: u64,
    gens: u64,
    cells: usize,
    m: &Metrics,
) {
    db.execute(
        "INSERT INTO runs (stage, origin, cfg_key, tiling, nb, rule, pos_b, pos_s, neg_b, neg_s,
            blast, seed, density, rng, gens, cells, score, conquest, both, balance, live, front,
            activity, territory, turnover, swing, leads, extinct, war_len, still, pos, neg, created)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,
            ?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33)",
        params![
            stage,
            origin,
            c.key(),
            c.tiling,
            if c.edge { "edge" } else { "touch" },
            c.rule.notation(),
            c.rule.pos_birth,
            c.rule.pos_survive,
            c.rule.neg_birth,
            c.rule.neg_survive,
            c.rule.blast,
            c.seed,
            c.density,
            rng as i64,
            gens as i64,
            cells as i64,
            m.score(c.seed == "halves"),
            m.conquest(gens),
            m.both,
            m.balance,
            m.live,
            m.front,
            m.activity,
            m.territory,
            m.turnover,
            m.swing,
            m.leads,
            m.extinct as i64,
            m.war_len as i64,
            m.still,
            m.pos,
            m.neg,
            now()
        ],
    )
    .unwrap();
}

fn cfg_from_row(row: &rusqlite::Row) -> rusqlite::Result<Cfg> {
    Ok(Cfg {
        tiling: row.get("tiling")?,
        edge: row.get::<_, String>("nb")? == "edge",
        rule: JacaRule {
            pos_birth: row.get("pos_b")?,
            pos_survive: row.get("pos_s")?,
            neg_birth: row.get("neg_b")?,
            neg_survive: row.get("neg_s")?,
            blast: row.get("blast")?,
        },
        seed: row.get("seed")?,
        density: row.get("density")?,
    })
}

/// The best screened configurations by max(score, conquest), per tiling up to `per`.
fn elite(db: &Connection, per: usize) -> Vec<(f64, Cfg)> {
    let mut st = db
        .prepare(
            "SELECT *, MAX(score, conquest) * (1 + 0.8 * still) AS fit FROM runs
             WHERE stage = 'screen' AND fit > 0 ORDER BY fit DESC LIMIT 6000",
        )
        .unwrap();
    let rows = st
        .query_map([], |row| {
            Ok((row.get::<_, f64>("fit")?, cfg_from_row(row)?))
        })
        .unwrap();
    let mut count: HashMap<String, usize> = HashMap::new();
    let mut out = vec![];
    for r in rows.flatten() {
        let c = count.entry(r.1.tiling.clone()).or_default();
        if *c < per {
            *c += 1;
            out.push(r);
        }
    }
    out
}

fn run(db_path: &str, hours: f64) {
    let db = open(db_path);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(hours * 3600.0);
    eprintln!("building tilings");
    let build = |cells: u32| -> HashMap<String, Grid> {
        TILINGS
            .par_iter()
            .filter_map(|(k, _)| match grid(k, cells) {
                Ok(g) => Some((k.to_string(), g)),
                Err(e) => {
                    eprintln!("{k}: {e}");
                    None
                }
            })
            .collect()
    };
    let screen = build(SCREEN_CELLS);
    let big = build(CONFIRM_CELLS);
    for (k, g) in &screen {
        eprintln!(
            "{k}: {} cells, touch max {} mean {:.2}, edge max {} mean {:.2}",
            g.topo.len(),
            g.max_deg[0],
            g.mean_deg[0],
            g.max_deg[1],
            g.mean_deg[1]
        );
    }
    let mut seen: HashSet<String> = {
        let mut st = db
            .prepare("SELECT DISTINCT cfg_key FROM runs WHERE stage = 'screen'")
            .unwrap();
        st.query_map([], |r| r.get(0)).unwrap().flatten().collect()
    };
    let mut rng = Rng::new(now() as u64);
    let mut batch_no = 0;
    while std::time::Instant::now() < deadline {
        batch_no += 1;
        let t0 = std::time::Instant::now();
        let el = elite(&db, 120);
        // Configurations kept by eye breed too.
        let keeps: Vec<Cfg> = {
            let mut st = db
                .prepare(
                    "SELECT r.* FROM reviews v JOIN runs r ON r.cfg_key = v.cfg_key
                     WHERE v.verdict = 'KEEP' GROUP BY v.cfg_key",
                )
                .unwrap();
            st.query_map([], cfg_from_row).unwrap().flatten().collect()
        };
        // Configurations to screen.
        let mut todo: Vec<(String, Cfg)> = vec![];
        let want = 1600;
        let mut tries = 0;
        while todo.len() < want && tries < want * 20 {
            tries += 1;
            let x = rng.next_f64();
            let (origin, c) = if !keeps.is_empty() && x > 0.85 {
                let a = keeps[rng.below(keeps.len() as u64) as usize].clone();
                if rng.next_f64() < 0.75 {
                    ("keep-mutate", mutate(&mut rng, &a, &screen))
                } else {
                    ("keep-transplant", transplant(&mut rng, &a, &screen))
                }
            } else if el.len() < 50 || x < 0.3 {
                ("fresh", fresh(&mut rng, &screen))
            } else {
                // Pick parents by rank (better ones more often).
                let pick = |r: &mut Rng| {
                    let u = r.next_f64();
                    &el[((u * u) * el.len() as f64) as usize].1
                };
                if x < 0.7 {
                    let a = pick(&mut rng).clone();
                    ("mutate", mutate(&mut rng, &a, &screen))
                } else if x < 0.88 {
                    let a = pick(&mut rng).clone();
                    let same: Vec<&Cfg> = el
                        .iter()
                        .map(|e| &e.1)
                        .filter(|e| e.tiling == a.tiling && e.edge == a.edge && **e != a)
                        .collect();
                    if same.is_empty() {
                        ("mutate", mutate(&mut rng, &a, &screen))
                    } else {
                        let b = same[rng.below(same.len() as u64) as usize].clone();
                        ("cross", cross(&mut rng, &a, &b, &screen))
                    }
                } else {
                    let a = pick(&mut rng).clone();
                    ("transplant", transplant(&mut rng, &a, &screen))
                }
            };
            if !birth_ok(c.rule.pos_birth) || !birth_ok(c.rule.neg_birth) {
                continue;
            }
            if seen.insert(c.key()) {
                todo.push((origin.to_string(), c));
            }
        }
        // Configurations to confirm.
        let confirm: Vec<Cfg> = {
            let mut st = db
                .prepare(
                    "SELECT *, MAX(score, conquest) * (1 + 0.8 * still) AS fit FROM runs r
                     WHERE stage = 'screen' AND fit > 0.25 AND NOT EXISTS
                       (SELECT 1 FROM confirmed c WHERE c.cfg_key = r.cfg_key)
                     ORDER BY fit DESC LIMIT 60",
                )
                .unwrap();
            st.query_map([], cfg_from_row).unwrap().flatten().collect()
        };
        let screened: Vec<(String, Cfg, Metrics, usize)> = todo
            .par_iter()
            .map(|(o, c)| {
                let g = &screen[&c.tiling];
                (
                    o.clone(),
                    c.clone(),
                    measure(c, g, SCREEN_GENS, 1),
                    g.topo.len(),
                )
            })
            .collect();
        let confirmed: Vec<(Cfg, Vec<Metrics>, usize)> = confirm
            .par_iter()
            .map(|c| {
                let g = &big[&c.tiling];
                let ms: Vec<Metrics> = [21, 22, 23]
                    .par_iter()
                    .map(|&s| measure(c, g, CONFIRM_GENS, s))
                    .collect();
                (c.clone(), ms, g.topo.len())
            })
            .collect();
        db.execute_batch("BEGIN").unwrap();
        for (o, c, m, cells) in &screened {
            insert(&db, "screen", o, c, 1, SCREEN_GENS, *cells, m);
        }
        for (c, ms, cells) in &confirmed {
            for (m, s) in ms.iter().zip([21u64, 22, 23]) {
                insert(&db, "confirm", "confirm", c, s, CONFIRM_GENS, *cells, m);
            }
            let sc: Vec<f64> = ms.iter().map(|m| m.score(c.seed == "halves")).collect();
            let cq: Vec<f64> = ms.iter().map(|m| m.conquest(CONFIRM_GENS)).collect();
            let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
            let min = |v: &[f64]| v.iter().cloned().fold(f64::MAX, f64::min);
            let wl: Vec<f64> = ms.iter().map(|m| m.war_len as f64).collect();
            let still: Vec<f64> = ms.iter().map(|m| m.still).collect();
            let act: Vec<f64> = ms.iter().map(|m| m.activity).collect();
            db.execute(
                "INSERT OR REPLACE INTO confirmed VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                params![
                    c.key(),
                    c.tiling,
                    if c.edge { "edge" } else { "touch" },
                    c.rule.notation(),
                    c.seed,
                    c.density,
                    min(&sc),
                    mean(&sc),
                    min(&cq),
                    mean(&cq),
                    mean(&wl),
                    ms.iter().filter(|m| m.extinct == 0).count() as i64,
                    mean(&still),
                    mean(&act),
                    now()
                ],
            )
            .unwrap();
        }
        db.execute_batch("COMMIT").unwrap();
        let good = screened
            .iter()
            .filter(|s| s.2.score(s.1.seed == "halves") > 0.0)
            .count();
        eprintln!(
            "batch {batch_no}: screened {} ({good} scored), confirmed {}, {:.0} s, elite {}, total seen {}",
            screened.len(),
            confirmed.len(),
            t0.elapsed().as_secs_f64(),
            el.len(),
            seen.len()
        );
    }
}

/// Recompute `score` and `conquest` of every run, and the `confirmed` aggregates, from the
/// stored measurements (after a scoring change).
fn rescore(db_path: &str) {
    let db = open(db_path);
    let rows: Vec<(i64, bool, u64, Metrics)> = {
        let mut st = db.prepare("SELECT * FROM runs").unwrap();
        st.query_map([], |r| {
            Ok((
                r.get::<_, i64>("id")?,
                r.get::<_, String>("seed")? == "halves",
                r.get::<_, i64>("gens")? as u64,
                Metrics {
                    both: r.get("both")?,
                    balance: r.get("balance")?,
                    live: r.get("live")?,
                    front: r.get("front")?,
                    activity: r.get("activity")?,
                    territory: r.get("territory")?,
                    turnover: r.get("turnover")?,
                    swing: r.get("swing")?,
                    leads: r.get("leads")?,
                    extinct: r.get::<_, i64>("extinct")? as u64,
                    war_len: r.get::<_, i64>("war_len")? as u64,
                    still: r.get("still")?,
                    pos: r.get("pos")?,
                    neg: r.get("neg")?,
                },
            ))
        })
        .unwrap()
        .flatten()
        .collect()
    };
    db.execute_batch("BEGIN").unwrap();
    for (id, halves, gens, m) in &rows {
        db.execute(
            "UPDATE runs SET score = ?1, conquest = ?2 WHERE id = ?3",
            params![m.score(*halves), m.conquest(*gens), id],
        )
        .unwrap();
    }
    db.execute_batch(
        "UPDATE confirmed SET
           score_min = (SELECT MIN(score) FROM runs r WHERE r.cfg_key = confirmed.cfg_key AND stage = 'confirm'),
           score_mean = (SELECT AVG(score) FROM runs r WHERE r.cfg_key = confirmed.cfg_key AND stage = 'confirm'),
           conquest_min = (SELECT MIN(conquest) FROM runs r WHERE r.cfg_key = confirmed.cfg_key AND stage = 'confirm'),
           conquest_mean = (SELECT AVG(conquest) FROM runs r WHERE r.cfg_key = confirmed.cfg_key AND stage = 'confirm');
         COMMIT;",
    )
    .unwrap();
    eprintln!("rescored {} runs", rows.len());
}

/// A Quick Start tiling for `key`: about 24,000 cells, about 3:2 wide (aperiodic: a patch).
fn preset_tiling(key: &str) -> TilingSpec {
    if aperiodic(key) {
        return sized_spec(key, CONFIRM_CELLS).unwrap().0;
    }
    let base = base_spec(key).with_boundary(Boundary::Wrap);
    let probe = |w: u32, h: u32| {
        let spec = base.clone().with_size(SizeSpec::Rect { w, h });
        Topology::build(&spec).ok().map(|t| {
            let b = t.bounds;
            (spec, t.len() as f64, (b[2] - b[0]) / (b[3] - b[1]))
        })
    };
    let Some((_, n0, a0)) = probe(20, 20) else {
        return sized_spec(key, CONFIRM_CELLS).unwrap().0;
    };
    // cells = n0 (w h / 400); aspect = a0 (w / h).
    let per = n0 / 400.0;
    let ratio = 1.5 / a0;
    let h = (24_000.0 / per / ratio).sqrt();
    let w = h * ratio;
    let mut best: Option<(f64, TilingSpec)> = None;
    for dw in -2i32..=2 {
        for dh in -2i32..=2 {
            let (w, h) = (
                (w.round() as i32 + dw).max(2) as u32,
                (h.round() as i32 + dh).max(2) as u32,
            );
            if let Some((spec, n, a)) = probe(w, h) {
                let err = (a / 1.5).ln().abs() + (n / 24_000.0).ln().abs();
                if best.as_ref().is_none_or(|b| err < b.0) {
                    best = Some((err, spec));
                }
            }
        }
    }
    best.unwrap().1
}

/// `presets IN.json OUT.json DB`: IN is a list of {"id", "name", "description", "run"} (a run id
/// in DB) with optional "palette", "palette_neg", "rng"; OUT gets the Quick Start presets.
fn presets(input: &str, output: &str, db_path: &str) {
    let db = open(db_path);
    let list: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(input).unwrap()).unwrap();
    let out: Vec<serde_json::Value> = list
        .iter()
        .map(|p| {
            let run = p["run"].as_i64().unwrap();
            let c = db
                .query_row("SELECT * FROM runs WHERE id = ?1", [run], cfg_from_row)
                .unwrap();
            let tiling = preset_tiling(&c.tiling);
            let mut scene = jetcarp_core::Scene::new(tiling);
            scene.name = p["name"].as_str().unwrap().into();
            scene.description = p["description"].as_str().unwrap_or("").into();
            scene.rng_state = p["rng"].as_u64().unwrap_or(21);
            let mut ca = CaSpec::new(&c.rule.notation()).seed(c.seed_spec());
            if c.edge {
                ca.neighborhood = Some(Nb::Edge);
            }
            ca.palette = Some(p["palette"].as_str().unwrap_or("Tide").into());
            ca.palette_neg = Some(p["palette_neg"].as_str().unwrap_or("Ember").into());
            scene.cas = vec![ca];
            let cells = Topology::build(&scene.tiling).map(|t| t.len()).unwrap_or(0);
            eprintln!(
                "{}: {} on {} ({cells} cells)",
                p["id"],
                c.rule.notation(),
                c.tiling
            );
            serde_json::json!({ "id": p["id"], "scene": scene })
        })
        .collect();
    std::fs::write(output, serde_json::to_string_pretty(&out).unwrap()).unwrap();
}

/// `tune PRESETS.json OUT.json`: pick each preset's RNG seed (21..=32). Presets whose id ends
/// in `-conquest` get the seed whose war lasts longest while still ending by 6,000
/// generations; the rest get the seed with the most even split at 3,000 and 6,000 generations.
fn tune(input: &str, output: &str) {
    let mut list: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(input).unwrap()).unwrap();
    let picks: Vec<(u64, String)> = list
        .par_iter()
        .map(|p| {
            let conquest = p["id"].as_str().unwrap().ends_with("-conquest");
            let base: jetcarp_core::Scene = serde_json::from_value(p["scene"].clone()).unwrap();
            let topo = Arc::new(Topology::build(&base.tiling).unwrap());
            let n = topo.len() as f64;
            let tries: Vec<(f64, u64, String)> = (21u64..=32)
                .into_par_iter()
                .map(|rng| {
                    let mut sc = base.clone();
                    sc.rng_state = rng;
                    let mut w = World::from_scene(&sc).unwrap();
                    let (mut ends, mut mins) = (0u64, vec![]);
                    let mut t = 0;
                    while t < 6000 {
                        w.apply(Command::Step { gens: 100 }).unwrap();
                        t += 100;
                        let f = &w.fields[0];
                        let pc = f.state.iter().filter(|&&c| c == POS).count() as f64 / n;
                        let nc = f.state.iter().filter(|&&c| c == NEG).count() as f64 / n;
                        if t == 3000 || t == 6000 {
                            mins.push(pc.min(nc) / pc.max(nc).max(1e-9));
                        }
                        if ends == 0 && (pc == 0.0 || nc == 0.0) {
                            ends = t;
                            if conquest {
                                break;
                            }
                        }
                    }
                    let score = if conquest {
                        if ends == 0 { 0.0 } else { ends as f64 }
                    } else {
                        mins.iter().cloned().fold(1.0, f64::min) + mins.iter().sum::<f64>() * 0.1
                    };
                    (
                        score,
                        rng,
                        format!("rng {rng}: ends {ends}, balance {mins:.2?}"),
                    )
                })
                .collect();
            let best = tries
                .iter()
                .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
                .unwrap();
            (best.1, best.2.clone())
        })
        .collect();
    for (p, (rng, note)) in list.iter_mut().zip(&picks) {
        eprintln!("{}: {note}", p["id"]);
        p["scene"]["rng_state"] = serde_json::json!(rng);
    }
    std::fs::write(output, serde_json::to_string_pretty(&list).unwrap()).unwrap();
}

/// `validate PRESETS.json OUT_DIR`: run each preset scene at full size, report the +1 and -1
/// shares at a few generations, and save a frame at 600 and 3000 generations.
fn validate(input: &str, dir: &str) {
    #[derive(serde::Deserialize)]
    struct P {
        id: String,
        scene: jetcarp_core::Scene,
    }
    let list: Vec<P> = serde_json::from_str(&std::fs::read_to_string(input).unwrap()).unwrap();
    std::fs::create_dir_all(dir).unwrap();
    let opts = RenderOptions {
        outline_min_px: 0.0,
        ..Default::default()
    };
    let results: Vec<(String, Image)> = list
        .par_iter()
        .map(|p| {
            let mut w = World::from_scene(&p.scene).unwrap();
            let n = w.topo.len() as f64;
            let mut line = format!("{:<28} {:>6} cells ", p.id, w.topo.len());
            let mut thumb = None;
            for at in [300u64, 600, 1500, 3000, 6000] {
                let g = w.generation;
                w.apply(Command::Step { gens: at - g }).unwrap();
                let f = &w.fields[0];
                let pc = f.state.iter().filter(|&&c| c == POS).count() as f64 / n;
                let nc = f.state.iter().filter(|&&c| c == NEG).count() as f64 / n;
                line.push_str(&format!(" {at}:{:.2}/{:.2}", pc, nc));
                if at == 600 || at == 3000 {
                    let img = render_fitted(&w, 720, 480, &opts);
                    write_png(
                        &std::path::Path::new(dir).join(format!("{}-{at}.png", p.id)),
                        &img,
                        &[],
                    )
                    .unwrap();
                }
                if at == 1500 {
                    thumb = Some(render_fitted(&w, 372, 248, &opts));
                }
            }
            (line, thumb.unwrap())
        })
        .collect();
    for (l, _) in &results {
        println!("{l}");
    }
    // A sheet of every preset at 1,500 generations, 4 per row, in list order.
    let (tw, th, per) = (372usize, 248usize, 4usize);
    let rows = results.len().div_ceil(per);
    let (iw, ih) = (per * (tw + 4), rows * (th + 4));
    let mut img = vec![255u8; iw * ih * 4];
    for (k, (_, t)) in results.iter().enumerate() {
        let (x0, y0) = ((k % per) * (tw + 4), (k / per) * (th + 4));
        for y in 0..t.height as usize {
            let src = &t.rgba[y * t.width as usize * 4..][..t.width as usize * 4];
            let d = ((y0 + y) * iw + x0) * 4;
            img[d..d + src.len()].copy_from_slice(src);
        }
    }
    write_png(
        &std::path::Path::new(dir).join("sheet.png"),
        &Image {
            width: iw as u32,
            height: ih as u32,
            rgba: img,
        },
        &[],
    )
    .unwrap();
}

// ---------------------------------------------------------------------------------------------
// Sheets

fn sheet(db_path: &str, sql: &str, out: &str, frames: &[u64]) {
    let db = open(db_path);
    let mut st = db.prepare(sql).unwrap();
    let ids: Vec<i64> = st
        .query_map([], |r| r.get("id"))
        .unwrap()
        .flatten()
        .collect();
    let mut cfgs = vec![];
    for id in &ids {
        let c = db
            .query_row("SELECT * FROM runs WHERE id = ?1", [id], cfg_from_row)
            .unwrap();
        cfgs.push((*id, c));
    }
    let keys: HashSet<&str> = cfgs.iter().map(|c| c.1.tiling.as_str()).collect();
    let grids: HashMap<String, Grid> = keys
        .into_par_iter()
        .map(|k| (k.to_string(), grid(k, CONFIRM_CELLS).unwrap()))
        .collect();
    let (tw, th) = (250u32, 170u32);
    let opts = RenderOptions {
        outline_min_px: 0.0,
        ..Default::default()
    };
    let tiles: Vec<Vec<Image>> = cfgs
        .par_iter()
        .map(|(_, c)| {
            let g = &grids[&c.tiling];
            let mut w = c.world(g, 21);
            let mut out = vec![];
            for &f in frames {
                let at = w.generation;
                w.apply(Command::Step { gens: f - at }).unwrap();
                out.push(render_fitted(&w, tw, th, &opts));
            }
            out
        })
        .collect();
    let per = 2usize;
    let gap = 8usize;
    let cols = per * frames.len();
    let rows = tiles.len().div_ceil(per);
    let iw = cols * tw as usize + per * gap;
    let ih = rows * (th as usize + 4);
    let mut img = vec![255u8; iw * ih * 4];
    for (k, set) in tiles.iter().enumerate() {
        for (j, t) in set.iter().enumerate() {
            let x0 = (k % per) * (frames.len() * tw as usize + gap) + j * tw as usize;
            let y0 = (k / per) * (th as usize + 4);
            for y in 0..t.height as usize {
                let src = &t.rgba[y * t.width as usize * 4..][..t.width as usize * 4];
                let d = ((y0 + y) * iw + x0) * 4;
                img[d..d + src.len()].copy_from_slice(src);
            }
        }
    }
    write_png(
        std::path::Path::new(out),
        &Image {
            width: iw as u32,
            height: ih as u32,
            rgba: img,
        },
        &[],
    )
    .unwrap();
    for (k, (id, c)) in cfgs.iter().enumerate() {
        println!(
            "{:>3} r{}c{} id={} {} {} {} {} {:.2}",
            k + 1,
            k / per + 1,
            k % per + 1,
            id,
            c.tiling,
            if c.edge { "edge" } else { "touch" },
            c.rule.notation(),
            c.seed,
            c.density
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("run") => run(
            &args[1],
            args.get(2).and_then(|h| h.parse().ok()).unwrap_or(6.0),
        ),
        Some("sheet") => {
            let frames: Vec<u64> = args
                .get(4)
                .map(|f| f.split(',').map(|x| x.parse().unwrap()).collect())
                .unwrap_or(vec![300, 1500, 4000]);
            sheet(&args[1], &args[2], &args[3], &frames)
        }
        Some("rescore") => rescore(&args[1]),
        Some("presets") => presets(&args[1], &args[2], &args[3]),
        Some("validate") => validate(&args[1], &args[2]),
        Some("tune") => tune(&args[1], &args[2]),
        Some("tilings") => {
            for (k, _) in TILINGS {
                match grid(k, SCREEN_CELLS) {
                    Ok(g) => println!(
                        "{k}: {} cells, touch max {} mean {:.2}, edge max {} mean {:.2}, edge differs {}",
                        g.topo.len(),
                        g.max_deg[0],
                        g.mean_deg[0],
                        g.max_deg[1],
                        g.mean_deg[1],
                        g.has_edge
                    ),
                    Err(e) => println!("{k}: {e}"),
                }
            }
        }
        _ => eprintln!("usage: run DB HOURS | sheet DB SQL OUT.png [FRAMES] | tilings"),
    }
}

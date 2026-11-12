//! The atlas: the published result of a search, the Quick Start rows of source `atlas`
//! (`jetcarp_store::library::quickstart`). Entries are selected from a search's runs ([`Runs`]:
//! its sealed segments and working database), given a category, a title and a palette, and made
//! with a mature state so a card can start where the action is ([`rows`]). The library writes
//! them (`Library::publish_atlas`); the menu reads them like every other entry.

use crate::explore::job::feat_from_blob;
use crate::explore::runs::{RunFilter, Runs, top_n};
use crate::explore::segments::{col, cols};
use crate::explore::sheet::RunRow;
use jetcarp_core::explore::cand::Cand;
use jetcarp_core::explore::features::Features;
use jetcarp_core::explore::space::{ALL_FAMS, Fam, sized_spec};
use jetcarp_core::world::{Scene, SeedSpec, World};
use jetcarp_store::quickstart::{self as qs, QsSource, QuickStartRow};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
        return if f.bits > 0.3 {
            "Fractals & rows"
        } else {
            QUIET_ROWS
        };
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
pub fn pick_palette(
    cat: &str,
    fam: Fam,
    states: usize,
    id: i64,
) -> (&'static str, Option<&'static str>) {
    // Age-coloured rules show settled cells in a palette's last stop, so it has to stay bright or
    // a pattern that has found order fades into the background.
    let settled = [
        "Verdigris",
        "Marigold",
        "Coral",
        "Orchid",
        "Citrus",
        "Riso",
        "Bone",
        "Ultramarine",
        "Stained Glass",
        "Opal",
        "Synthwave",
        "Blueprint",
    ];
    let movers = [
        "Prism",
        "Synthwave",
        "Neon",
        "Koi",
        "Candy",
        "Riso",
        "Coral",
        "Citrus",
    ];
    // Whole-grid fields: strong, distinct stops. Palettes that open on white wash these out.
    let fields = [
        "Stained Glass",
        "Riso",
        "Ember",
        "Tide",
        "Synthwave",
        "Aurora",
        "Verdigris",
        "Thermal",
        "Bone",
        "Deep Sea",
    ];
    let wheels = ["Stained Glass", "Opal"];
    let jaca_pos = ["Aurora", "Ultramarine", "Verdigris", "Orchid"];
    let jaca_neg = ["Marigold", "Coral", "Citrus", "Bone"];
    // Run ids can share a factor with a list's length; mix them first.
    let mixed = ((id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) as usize;
    let pick = |set: &[&'static str]| set[mixed % set.len()];
    if fam == Fam::Jaca {
        return (
            pick(&jaca_pos),
            Some(jaca_neg[(mixed >> 8) % jaca_neg.len()]),
        );
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

/// Worst-seed confirmed score of each parent run.
fn confirmed_scores(runs: &Runs) -> Result<HashMap<i64, f64>, String> {
    let rows: Vec<(i64, f64)> = runs.scan(
        cols(&[col::STAGE, col::PARENT, col::SCORE]),
        &|s, _, rg| {
            s.int_range(rg, col::STAGE)
                .is_none_or(|(lo, hi)| lo <= 2 && 2 <= hi)
        },
        &|b, out| {
            for i in 0..b.len {
                if b.stage(i) == 2
                    && let Some(p) = b.opt_i64(col::PARENT, i)
                {
                    out.push((p, b.score(i)));
                }
            }
        },
    )?;
    let mut m: HashMap<i64, f64> = HashMap::new();
    for (p, s) in rows {
        let x = m.entry(p).or_insert(s);
        if s < *x {
            *x = s;
        }
    }
    Ok(m)
}

/// Screened runs only.
fn screened(s: &crate::explore::segments::Segment, rg: usize) -> bool {
    s.int_range(rg, col::STAGE)
        .is_none_or(|(lo, hi)| lo <= 1 && 1 <= hi)
}

/// Select runs for the atlas: a list of `(parent run, confirmed score, category)`.
pub fn select(db: &Runs, opts: &AtlasOpts) -> Result<Vec<(RunRow, f64, &'static str)>, String> {
    let scores = confirmed_scores(db)?;
    let ids: Vec<i64> = db.scan(
        cols(&[col::ID, col::STAGE, col::SEED_KIND]),
        &|s, _, rg| screened(s, rg),
        &|b, out| {
            for i in 0..b.len {
                if b.stage(i) == 1
                    && (b.bytes(col::SEED_KIND, i) == b"point" || scores.contains_key(&b.id(i)))
                {
                    out.push(b.id(i));
                }
            }
        },
    )?;
    let runs = db.rows(&ids)?;
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
            let s = if c == QUIET_ROWS {
                s
            } else {
                seed_adjusted(r.cand.fam, &r.cand.seed, &r.fate, s)
            };
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
    let contrast = contrast_picks(db, &out)?;
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
    db: &Runs,
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
        let ids = if fam == Fam::WireWorld {
            let mut v: Vec<(bool, f64, i64)> = db.scan(
                cols(&[col::ID, col::STAGE, col::FAM, col::SEED_KIND, col::SCORE]),
                &|s, _, rg| screened(s, rg),
                &|b, out| {
                    for i in 0..b.len {
                        if b.stage(i) == 1 && b.bytes(col::FAM, i) == fam.name().as_bytes() {
                            out.push((
                                b.bytes(col::SEED_KIND, i) != b"circuit",
                                -b.score(i),
                                b.id(i),
                            ));
                        }
                    }
                },
            )?;
            v.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
            v.truncate(3000);
            v.into_iter().map(|x| x.2).collect()
        } else {
            db.ids(
                &RunFilter {
                    family: Some(fam),
                    ..Default::default()
                },
                3000,
            )?
        };
        let mut runs = db.rows(&ids)?;
        if fam == Fam::WireWorld {
            runs.sort_by_key(|r| !busy_circuit(&r.cand.seed, &r.fate));
        }
        // A family tied to one or two grids can fill its share there.
        let tilings = runs
            .iter()
            .map(|r| r.cand.tiling.as_str())
            .collect::<HashSet<_>>()
            .len();
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
    let groups: HashMap<String, String> = jetcarp_core::explore::space::all_tilings()
        .into_iter()
        .map(|t| (t.key, t.group))
        .collect();
    let mut ends: Vec<(f64, i64)> = db.scan(
        cols(&[col::ID, col::STAGE, col::FATE, col::SCORE]),
        &|s, _, rg| screened(s, rg),
        &|b, out| {
            for i in 0..b.len {
                let f = b.bytes(col::FATE, i);
                if b.stage(i) == 1 && (f == b"died" || f == b"froze") && b.id(i) % 97 == 0 {
                    out.push((b.score(i), b.id(i)));
                }
            }
        },
    )?;
    top_n(&mut ends, 60_000);
    let endings = db.rows(&ends.iter().map(|x| x.1).collect::<Vec<_>>())?;
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

/// The ids an atlas row of a search has: (`atlas-<8 hex>-<run>`, `atlas:<8 hex>-<run>`).
pub fn row_ids(search_uuid: &str, run: i64) -> (String, String) {
    let short: String = search_uuid.chars().filter(|c| *c != '-').take(8).collect();
    (
        format!("atlas-{short}-{run}"),
        format!("atlas:{short}-{run}"),
    )
}

/// One run as an atlas row: the full scene (the rule and start on the larger grid it was
/// confirmed on) is what opening loads; the preview runs on the small grid it was found on,
/// from its mature state.
fn make_row(
    run: &RunRow,
    score: f64,
    cat: &str,
    group: &str,
    search_uuid: &str,
) -> Result<QuickStartRow, String> {
    let c: &Cand = &run.cand;
    let f = feat_from_blob(&run.feats);
    let states = c.rule().map_or(2, |r| match r.field_kind() {
        jetcarp_core::rules::FieldKind::Multi(n) => n as usize,
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
    let (mature_gen, mature) =
        if c.fam.is_rows() || matches!(c.fam, Fam::Turmite | Fam::Sandpile | Fam::Lenia) {
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
                (at, w.fields.first().map(|x| qs::deflate_state(&x.state)))
            }
        };
    let family = qs::family_label(c.fam).to_string();
    let seed = qs::describe_seed(&serde_json::to_string(&c.seed).unwrap_or_default());
    let (id, handle) = row_ids(search_uuid, run.id);
    let mut q = QuickStartRow::for_scene(&id, "search_run", full);
    q.handle = handle;
    q.source = QsSource::Atlas;
    q.source_ref = search_uuid.to_string();
    q.target_id = Some(format!("{search_uuid}:{}", run.id));
    q.title = qs::atlas_title(&c.rule, cat, &family);
    q.description = format!("{} on {}\nSeed: {seed}\nScore {score:.2}", c.rule, c.tiling);
    q.category = cat.to_string();
    q.behaviour = cat.to_string();
    q.grid_group = qs::group_label(group).to_string();
    q.family = family;
    q.tiling_key = c.tiling.clone();
    q.tiling_name = qs::tiling_label(&c.tiling);
    q.rule = c.rule.clone();
    q.preview = Some(preview);
    q.mature_gen = mature_gen;
    q.mature = mature;
    q.score = Some(score);
    Ok(q)
}

/// One stored run as an atlas row (its category, title, palette, preview and mature state, as
/// the atlas makes them), scored by its own screening score: what promoting a search run to the
/// Quick Start starts from.
pub fn run_row(run: &RunRow, search_uuid: &str) -> Result<QuickStartRow, String> {
    let cat = categorize(
        &run.fate,
        &feat_from_blob(&run.feats),
        run.cand.fam,
        run.cand.gens,
    );
    let group = jetcarp_core::explore::space::all_tilings()
        .into_iter()
        .find(|t| t.key == run.cand.tiling)
        .map(|t| t.group)
        .unwrap_or_else(|| "other".into());
    make_row(run, run.score, cat, &group, search_uuid)
}

/// An atlas from a search's runs: the rows of source `atlas` that [`select`] picks (see
/// [`AtlasOpts`]), best first (by score, then run id), ranked in that order, for the search
/// whose uuid is `search_uuid`.
pub fn rows(db: &Runs, opts: &AtlasOpts, search_uuid: &str) -> Result<Vec<QuickStartRow>, String> {
    rows_on(db, opts, search_uuid, None)
}

/// [`rows`], replaying the entries' runs on `pool` (a Quick Start build's own threads, which
/// leave the computer cores of its own) when one is given, else on every core.
pub fn rows_on(
    db: &Runs,
    opts: &AtlasOpts,
    search_uuid: &str,
    pool: Option<&rayon::ThreadPool>,
) -> Result<Vec<QuickStartRow>, String> {
    let t0 = std::time::Instant::now();
    let picks = select(db, opts)?;
    let t1 = std::time::Instant::now();
    let groups: HashMap<String, String> = jetcarp_core::explore::space::all_tilings()
        .into_iter()
        .map(|t| (t.key, t.group))
        .collect();
    let make = || -> Vec<(i64, QuickStartRow)> {
        picks
            .par_iter()
            .filter_map(|(r, s, c)| {
                let group = groups
                    .get(&r.cand.tiling)
                    .cloned()
                    .unwrap_or_else(|| "other".into());
                make_row(r, *s, c, &group, search_uuid)
                    .ok()
                    .map(|q| (r.id, q))
            })
            .collect()
    };
    let mut made = match pool {
        Some(p) => p.install(make),
        None => make(),
    };
    if std::env::var_os("JETCARP_EXPLORE_TRACE").is_some() {
        eprintln!(
            "atlas: {} picks in {:.2} s, their entries made in {:.2} s",
            picks.len(),
            (t1 - t0).as_secs_f64(),
            t1.elapsed().as_secs_f64()
        );
    }
    made.sort_by(|a, b| {
        b.1.score
            .unwrap_or(0.0)
            .total_cmp(&a.1.score.unwrap_or(0.0))
            .then(a.0.cmp(&b.0))
    });
    Ok(made
        .into_iter()
        .enumerate()
        .map(|(k, (_, mut q))| {
            q.rank = k as i64;
            q
        })
        .collect())
}

/// A stratified sample of screened runs for a person to rate: spread over categories and
/// families, half strong and half random (drawn with `seed`), so ratings can calibrate the score.
pub fn taste_sample(db: &Runs, n: usize, seed: u64) -> Result<Vec<(RunRow, &'static str)>, String> {
    let strong = db.rows(&db.ids(&RunFilter::default(), 12_000)?)?;
    let mut pool: Vec<i64> = db.scan(
        cols(&[col::ID, col::STAGE, col::SCORE]),
        &|s, side, rg| {
            screened(s, rg)
                && side
                    .and_then(|x| x.float_max(rg, col::SCORE))
                    .or_else(|| s.float_max(rg, col::SCORE))
                    .is_none_or(|m| m > 0.2)
        },
        &|b, out| {
            out.extend(
                (0..b.len)
                    .filter(|&i| b.stage(i) == 1 && b.score(i) > 0.2)
                    .map(|i| b.id(i)),
            )
        },
    )?;
    let mut rng = jetcarp_core::rng::Rng::new(seed);
    // A random 12,000 of them (the first part of a shuffle).
    let take = pool.len().min(12_000);
    for i in 0..take {
        let j = i + rng.below((pool.len() - i) as u64) as usize;
        pool.swap(i, j);
    }
    pool.truncate(take);
    let any = db.rows(&pool)?;
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

/// Stored runs with their categories, in id order.
pub fn runs_by_id(db: &Runs, ids: &[i64]) -> Result<Vec<(RunRow, &'static str)>, String> {
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    let runs = db.rows(&ids)?;
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
        jetcarp_core::rules::FieldKind::Multi(n) => n as usize,
        _ => 2,
    });
    let (pal, pal_neg) = pick_palette(cat, c.fam, states, run.id);
    let (_, topo) = sized_spec(&c.tiling, c.cells)?;
    let mut w = World::on_topology(topo, c.rng);
    let mut ca = c.ca();
    ca.palette = Some(pal.to_string());
    ca.palette_neg = pal_neg.map(String::from);
    w.add_ca(ca)?;
    let opts = jetcarp_core::render::RenderOptions {
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
        imgs.push(jetcarp_core::render::render_fitted(&w, px, px, &opts));
    }
    let refs: Vec<&jetcarp_core::render::Image> = imgs.iter().collect();
    jetcarp_core::media::encode::gif(&refs, 110)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeding_shifts_the_atlas_score() {
        let busy = SeedSpec::Circuit {
            sites: 60,
            electrons: 300,
        };
        let thin = SeedSpec::Circuit {
            sites: 8,
            electrons: 8,
        };
        let soup = SeedSpec::Random { density: 0.3 };
        assert_eq!(seed_adjusted(Fam::WireWorld, &busy, "cycle", 0.6), 1.2);
        assert_eq!(seed_adjusted(Fam::WireWorld, &busy, "froze", 0.6), 0.6);
        assert_eq!(seed_adjusted(Fam::WireWorld, &thin, "cycle", 0.6), 0.6);
        assert_eq!(
            seed_adjusted(Fam::Wolfram, &SeedSpec::TopCenter, "froze", 1.0),
            1.5
        );
        assert_eq!(
            seed_adjusted(Fam::Layered, &SeedSpec::Center, "froze", 1.0),
            1.25
        );
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

//! `library.soup`: the exact soup a hunt find came from, rebuilt as a scene that replays to it.
//!
//! **What makes a soup.** Soup `i` of a hunt is a pure function of the hunt
//! ([`jetcarp_core::hunt::soup`]): the tiling, the soup's size, density and shape, and the random
//! state `soup_state(seed, i)`. Its rows are sheared as the tiling's lattice is (`a2.x / a1.x`),
//! which the tiling itself gives, so the shear needs no record. A hunt find's record keeps the
//! tiling with the soup's size, the density, the soup's index and random state, and the
//! generation the soup ended at; records from 0.4.2 on also keep the soup's shape and the hunt's
//! seed.
//!
//! **What this does.** It rebuilds the soup, runs it again with the hunt's own engine (the
//! hunt's limits when its job is in the library) and checks that it makes the find. A record
//! that does not say its shape was a rect or a disc, so a rect is tried first, then a disc. The
//! scene is the soup pasted at the centre of a dead-edged grid with room for it to run.
//!
//! **The best soups** (`best`): the hunt's soups run again, from soup 0, and the ones that made
//! the most copies of the find (then the most spaceships) given. That needs the hunt's seed: the
//! record's own (0.4.2 on), or its job's spec while the job is in the library.

use super::library::catalog_err;
use crate::ctx::Ctx;
use crate::error::ApiError;
use crate::op::{Op, Progress, Report};
use jetcarp_core::hunt::separate::{Cache, Census};
use jetcarp_core::hunt::soup::{Layout, SoupShape, SoupSpec, run_soup, soup_state};
use jetcarp_core::pattern::paste::{PasteAt, PatternSource};
use jetcarp_core::pattern::{PatternClass, PatternRecord, encode_cells, tiling_identity};
use jetcarp_core::{Boundary, CaSpec, Command, Scene, SeedSpec, SizeSpec, World};
use jetcarp_search::hunt::HuntSpec;
use jetcarp_store::Library;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

/// The find, and which soups.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SoupIn {
    /// The hunt find: a pattern id (or a unique prefix, or a name).
    pub id: String,
    /// The N soups of its hunt that made the most copies of it (then the most spaceships), instead of the soup that found it first. Needs the hunt's seed: the record's (finds from 0.4.2 on) or its job's.
    #[serde(default)]
    pub best: Option<usize>,
    /// With `best`: how many of the hunt's soups to run again, from soup 0 (default: as many as the hunt ran, at most 10,000).
    #[serde(default)]
    pub soups: Option<u64>,
    /// Write the scene here; with `best`, the best soup's, and the others beside it as `<name>-2.json`, `<name>-3.json`, ...
    #[serde(default)]
    pub save: Option<String>,
}

/// One rebuilt soup.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SoupMade {
    /// Its index in the hunt.
    pub soup: u64,
    /// Its random state.
    #[serde(with = "jetcarp_core::json_int::uint")]
    #[schemars(schema_with = "crate::op::uint_schema")]
    pub rng_state: u64,
    /// Copies of the find it made, and spaceships of any kind (both counting ships that flew off).
    pub copies: u64,
    pub ships: u64,
    /// How it ended (`settled`, `died`, `exploded`, `unsettled`) and at which generation.
    pub end: String,
    pub generation: u64,
    /// Where its scene was written, with `save`.
    pub saved: Option<String>,
}

/// The soups rebuilt.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SoupScene {
    pub id: String,
    pub label: String,
    pub rule: String,
    pub tiling: String,
    /// The hunt's job, when it is in the library.
    pub job: Option<i64>,
    /// The soups' size in unit cells, density and shape (`rect` or `disc`).
    pub size: [u32; 2],
    pub density: f64,
    pub shape: String,
    /// The hunt's seed, when it is known.
    #[serde(with = "jetcarp_core::json_int::uint_opt")]
    #[schemars(schema_with = "crate::op::uint_opt_schema")]
    pub seed: Option<u64>,
    /// With `best`: the soups run again (none when none of them made it).
    pub looked: Option<u64>,
    /// The soup that found it first, or with `best` the best soups, best first.
    pub soups: Vec<SoupMade>,
    /// The first soup's scene.
    #[schemars(with = "serde_json::Value")]
    pub scene: Scene,
    /// What had to be worked out, and how far.
    pub notes: Vec<String>,
}

impl Report for SoupScene {
    fn text(&self) -> String {
        let mut s = format!(
            "{} ({} on {}): {}x{} {} soups at density {}{}\n",
            self.label,
            self.rule,
            self.tiling,
            self.size[0],
            self.size[1],
            self.shape,
            self.density,
            self.job
                .map(|j| format!(", hunt job {j}"))
                .unwrap_or_default()
        );
        if let Some(n) = self.looked {
            let _ = writeln!(s, "  the best {} of {n} soups run again:", self.soups.len());
        }
        for m in &self.soups {
            let _ = writeln!(
                s,
                "  soup {} (rng state {}): {} cop{}, {} ship{}; {} at generation {}{}",
                m.soup,
                m.rng_state,
                m.copies,
                if m.copies == 1 { "y" } else { "ies" },
                m.ships,
                if m.ships == 1 { "" } else { "s" },
                m.end,
                m.generation,
                m.saved
                    .as_ref()
                    .map(|p| format!("; scene: {p}"))
                    .unwrap_or_default()
            );
        }
        for n in &self.notes {
            let _ = writeln!(s, "  note: {n}");
        }
        s
    }
}

fn shape_name(s: SoupShape) -> &'static str {
    match s {
        SoupShape::Rect => "rect",
        SoupShape::Disc => "disc",
    }
}

/// The hunt job that ran the soup `soup` with random state `rng`, `w` by `h` unit cells, on the
/// record's tiling, and saw the record, if it is in the library: its id, spec, and soups run.
fn hunt_job(
    lib: &Library,
    rec: &PatternRecord,
    (soup, rng): (u64, u64),
    (w, h): (u32, u32),
) -> Result<Option<(i64, HuntSpec, u64)>, ApiError> {
    let key = jetcarp_core::pattern::tiling_key(&rec.tiling);
    let saw: Vec<i64> = lib
        .sightings(&rec.id)
        .map_err(catalog_err)?
        .iter()
        .map(|s| s.job)
        .collect();
    for j in lib.jobs().map_err(catalog_err)?.into_iter().rev() {
        if j.kind != "hunt" || !saw.contains(&j.id) {
            continue;
        }
        let Ok(spec) = serde_json::from_value::<HuntSpec>(j.spec.clone()) else {
            continue;
        };
        if soup_state(spec.seed, soup) == rng
            && (spec.soup.w, spec.soup.h) == (w, h)
            && spec
                .tilings
                .iter()
                .any(|t| jetcarp_core::pattern::tiling_key(t) == key)
        {
            let done = j
                .progress
                .get("soups_done")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            return Ok(Some((j.id, spec, done)));
        }
    }
    Ok(None)
}

/// A soup run with the hunt's engine: (copies of `id`, spaceships, end, generation).
fn run_one(
    c: &Census,
    cache: &mut Cache,
    layout: &Layout,
    limits: jetcarp_core::hunt::soup::Limits,
    index: u64,
    rng: u64,
    id: &str,
) -> (u64, u64, String, u64) {
    let out = run_soup(c, cache, layout, limits, index, rng);
    let copies = out.objects.iter().filter(|f| f.id == id).count() as u64;
    let ships = out
        .objects
        .iter()
        .filter(|f| f.class == PatternClass::Spaceship)
        .count() as u64;
    (copies, ships, out.end.name().to_string(), out.gens)
}

/// The scene of a soup: its cells pasted at the centre of a dead-edged grid with room to run
/// `gens` generations (at most 1,024 unit cells, or 16 million cells, on a side).
#[allow(clippy::too_many_arguments)]
fn scene_of(
    rec: &PatternRecord,
    layout: &Layout,
    ntypes: u32,
    soup: u64,
    rng: u64,
    gens: u64,
    density: f64,
    notes: &mut Vec<String>,
) -> Result<Scene, ApiError> {
    let cells = layout.soup(rng);
    let (w, h) = (layout.spec.w, layout.spec.h);
    let fit = ((16_000_000 / ntypes.max(1) as u64) as f64).sqrt() as u64;
    let want = w.max(h) as u64 + 2 * (gens + 16);
    let side = want.clamp(64, 1024.min(fit)) as u32;
    if want > side as u64 && notes.iter().all(|n| !n.starts_with("the grid")) {
        notes.push(format!(
            "the grid is {side} unit cells across: ships that fly off reach its dead edge before \
             generation {gens}, and land there"
        ));
    }
    let mut tiling = tiling_identity(&rec.tiling);
    tiling.size = Some(SizeSpec::Rect { w: side, h: side });
    tiling.boundary = Boundary::Dead;
    let mut sc = Scene::new(tiling.clone());
    sc.name = format!("The soup that made {}", rec.label());
    sc.description = format!(
        "Soup {soup} of a JETCARP hunt (rng state {rng}, {w}x{h} unit cells, {} at density \
         {density}), rebuilt exactly; it made {} ({}) by generation {gens}.",
        shape_name(layout.spec.shape),
        rec.label(),
        rec.id
    );
    sc.cas = vec![
        CaSpec::new(&rec.rule)
            .nb(rec.neighborhood)
            .steps(rec.steps_per_gen)
            .seed(SeedSpec::Empty),
    ];
    sc.commands.push(Command::PastePattern {
        id: 1,
        pattern: PatternSource {
            library: None,
            cells: Some(encode_cells(&cells)),
            tiling: Some(tiling_identity(&rec.tiling)),
            site: None,
        },
        at: PasteAt::Center,
        orientation: 0,
        phase: 0,
    });
    // It must load: the paste fits.
    World::from_scene(&sc)?;
    Ok(sc)
}

/// `library.soup`.
pub struct LibrarySoup;
impl Op for LibrarySoup {
    const NAME: &'static str = "library.soup";
    const SUMMARY: &'static str = "Rebuild the exact soup a hunt find came from (or the soups of its hunt that made the most copies of it) as a scene that replays to the find.";
    const READ_ONLY: bool = false;
    type In = SoupIn;
    type Out = SoupScene;
    fn run(cx: &Ctx, a: SoupIn, p: &dyn Progress) -> Result<SoupScene, ApiError> {
        use rayon::prelude::*;
        let lib = cx.library();
        let rec = lib.get(&a.id).map_err(catalog_err)?;
        let prov = &rec.provenance;
        let not_hunt = || {
            ApiError::invalid(format!(
                "{} was not found by a hunt (its source is '{}'), so it has no soup to rebuild",
                rec.id, prov.source
            ))
        };
        if prov.source != "hunt" {
            return Err(not_hunt());
        }
        let (Some(tiling), Some(rng), Some(soup)) = (&prov.tiling, prov.rng_state, prov.soup)
        else {
            return Err(not_hunt());
        };
        let Some(SizeSpec::Rect { w, h }) = tiling.size else {
            return Err(ApiError::invalid(format!(
                "{}'s record does not give its soup's size",
                rec.id
            )));
        };
        let density = prov.density.unwrap_or(0.5);
        let mut notes = Vec::new();
        let job = hunt_job(&lib, &rec, (soup, rng), (w, h))?;
        let mut spec = job.as_ref().map(|j| j.1.clone()).unwrap_or_default();
        if job.is_none() {
            notes.push(
                "the hunt that found it is not in this library: its default limits are used".into(),
            );
        }
        spec.max_period = spec.max_period.max(rec.period);
        let seed = prov.seed.or(job.as_ref().map(|j| j.1.seed));
        let shapes: Vec<SoupShape> = match (prov.shape, &job) {
            (Some(s), _) => vec![s],
            (None, Some(j)) => vec![j.1.soup.shape],
            (None, None) => vec![SoupShape::Rect, SoupShape::Disc],
        };
        let census = Census::new(
            &rec.rule,
            &tiling_identity(&rec.tiling),
            Some(rec.neighborhood),
            spec.max_period,
        )
        .map_err(ApiError::engine)?;
        let ntypes = census.topo.lattice().map_or(1, |g| g.ntypes);
        // The soup that found it, run as the hunt ran it: until the generation it ended at.
        let mut made = None;
        for &shape in &shapes {
            let ss = SoupSpec {
                w,
                h,
                density,
                shape,
            };
            let layout = Layout::new(&census.topo, ss, spec.margin()).map_err(ApiError::engine)?;
            let mut limits = spec.limits(layout.cells.len());
            if let Some(g) = prov.generation {
                limits.max_gens = g.max(1);
            }
            let mut cache = Cache::with_budget(Cache::DEFAULT_BUDGET);
            let r = run_one(&census, &mut cache, &layout, limits, soup, rng, &rec.id);
            if r.0 > 0 {
                made = Some((layout, r));
                break;
            }
        }
        let Some((layout, first)) = made else {
            return Err(ApiError::engine(format!(
                "soup {soup} (rng state {rng}), rebuilt as a {} soup, does not make {}: the record \
                 may come from an engine whose soups or rules differ from this one's",
                shapes
                    .iter()
                    .map(|s| shape_name(*s))
                    .collect::<Vec<_>>()
                    .join(" or "),
                rec.id
            )));
        };
        if prov.shape.is_none() && job.is_none() {
            notes.push(format!(
                "the record does not say the soup's shape: a {} soup makes it",
                shape_name(layout.spec.shape)
            ));
        }
        let mut soups = vec![SoupMade {
            soup,
            rng_state: rng,
            copies: first.0,
            ships: first.1,
            end: first.2,
            generation: first.3,
            saved: None,
        }];
        let mut looked = None;
        if let Some(n) = a.best {
            let seed = seed.ok_or_else(|| {
                ApiError::invalid(format!(
                    "the hunt that found {} is not in this library and its record (from before \
                     0.4.2) does not keep the hunt's seed, so its other soups cannot be made again",
                    rec.id
                ))
            })?;
            let ran = job.as_ref().map(|j| j.2).filter(|&d| d > 0);
            let total = a.soups.unwrap_or(ran.unwrap_or(10_000).min(10_000)).max(1);
            let limits = spec.limits(layout.cells.len());
            let done = AtomicU64::new(0);
            let pool = rayon::ThreadPoolBuilder::new()
                .build()
                .map_err(|e| ApiError::engine(e.to_string()))?;
            let mut all: Vec<SoupMade> = pool.install(|| {
                (0..total)
                    .into_par_iter()
                    .map_init(
                        || Cache::with_budget(Cache::DEFAULT_BUDGET),
                        |cache, i| {
                            let st = soup_state(seed, i);
                            let r = if p.cancelled() {
                                (0, 0, String::new(), 0)
                            } else {
                                run_one(&census, cache, &layout, limits, i, st, &rec.id)
                            };
                            let k = done.fetch_add(1, Ordering::Relaxed) + 1;
                            if k.is_multiple_of(256) || k == total {
                                p.report(k, Some(total), &format!("soup {k} of {total}"));
                            }
                            SoupMade {
                                soup: i,
                                rng_state: st,
                                copies: r.0,
                                ships: r.1,
                                end: r.2,
                                generation: r.3,
                                saved: None,
                            }
                        },
                    )
                    .filter(|m| m.copies > 0)
                    .collect()
            });
            all.sort_by(|x, y| {
                (y.copies, y.ships)
                    .cmp(&(x.copies, x.ships))
                    .then(x.soup.cmp(&y.soup))
            });
            all.truncate(n.max(1));
            if all.is_empty() {
                notes.push(format!(
                    "none of soups 0 to {} made it; the soup that found it is given instead",
                    total - 1
                ));
            } else {
                soups = all;
                looked = Some(total);
            }
        }
        // The scenes.
        let mut scene = None;
        for (k, m) in soups.iter_mut().enumerate() {
            let sc = scene_of(
                &rec,
                &layout,
                ntypes,
                m.soup,
                m.rng_state,
                m.generation,
                density,
                &mut notes,
            )?;
            if let Some(path) = &a.save {
                let name = if k == 0 {
                    path.clone()
                } else {
                    let pb = std::path::Path::new(path);
                    let stem = pb
                        .file_stem()
                        .map(|s| s.to_string_lossy())
                        .unwrap_or_default();
                    let ext = pb
                        .extension()
                        .map(|e| format!(".{}", e.to_string_lossy()))
                        .unwrap_or_default();
                    pb.with_file_name(format!("{stem}-{}{ext}", k + 1))
                        .display()
                        .to_string()
                };
                let file = cx.fs.writable(&name)?;
                std::fs::write(&file, sc.to_json() + "\n")
                    .map_err(|e| ApiError::io(format!("writing {}: {e}", file.display())))?;
                m.saved = Some(file.display().to_string());
            }
            if k == 0 {
                scene = Some(sc);
            }
        }
        Ok(SoupScene {
            id: rec.id.clone(),
            label: rec.label(),
            rule: rec.rule.clone(),
            tiling: rec.tiling_name.clone(),
            job: job.as_ref().map(|j| j.0),
            size: [w, h],
            density,
            shape: shape_name(layout.spec.shape).into(),
            seed,
            looked,
            soups,
            scene: scene.expect("one soup at least"),
            notes,
        })
    }
}

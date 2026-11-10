//! `jetcarp census` and `jetcarp objects`.

use anyhow::{Context, Result, bail};
use jetcarp_core::census::{self, CensusOptions, CensusReport, CensusSpec, RunClass};
use jetcarp_core::objects::{ObjectOptions, find_objects};
use jetcarp_core::render::{RenderOptions, png_metadata, render_with_colors, write_png};
use jetcarp_core::*;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct CensusArgs {
    /// Tiling family (see `jetcarp tilings`).
    #[arg(long, default_value = "square")]
    family: String,
    /// WxH unit cells (64x64), about N cells (4000), or L<level> for aperiodic tilings (L4).
    #[arg(long, default_value = "64x64")]
    size: String,
    /// wrap, dead, or live.
    #[arg(long, default_value = "wrap")]
    boundary: String,
    /// Take the tiling from a scene or tile JSON file instead of --family/--size/--boundary.
    #[arg(long)]
    tiling: Option<PathBuf>,
    /// Neighbourhood for every rule: touch or edge (default: per rule; a V suffix means edge).
    #[arg(long)]
    nb: Option<String>,
    /// Rules separated by spaces or semicolons, e.g. "B3/S23 B36/S23 B2/S/C3".
    #[arg(long)]
    rules: Option<String>,
    /// File with one rule per line (# comments).
    #[arg(long)]
    rules_file: Option<PathBuf>,
    /// Every B/S rule whose digits come from --birth and --survive.
    #[arg(long)]
    all: bool,
    /// A deterministic random sample of N distinct B/S rules over --birth and --survive.
    #[arg(long)]
    sample: Option<usize>,
    #[arg(long, default_value_t = 1)]
    sample_seed: u64,
    /// Birth digits for --all/--sample, like 1-8 or 1,2,3,6 ("max" = the tiling's largest degree).
    #[arg(long, default_value = "1-max")]
    birth: String,
    /// Survival digits for --all/--sample.
    #[arg(long, default_value = "0-max")]
    survive: String,
    /// Generations rules with this many states (for --all/--sample).
    #[arg(long)]
    states: Option<u8>,
    /// Soups per rule (soup s uses RNG state seed + s).
    #[arg(long, default_value_t = 8)]
    soups: u32,
    #[arg(long, default_value_t = 0.37)]
    density: f64,
    /// Generation limit per run.
    #[arg(long, default_value_t = 2000)]
    gens: u64,
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Stop a run and call it exploded once its population exceeds this fraction of the cells.
    #[arg(long, default_value_t = 0.5)]
    explode: f64,
    /// Write per-rule summaries as CSV.
    #[arg(long)]
    csv: Option<PathBuf>,
    /// Write the full report (spec, seeds, every run) as JSON.
    #[arg(long)]
    json: Option<PathBuf>,
    /// Labelled contact sheet of each rule's first soup at the end of its run.
    #[arg(long)]
    sheet: Option<PathBuf>,
    #[arg(long, default_value_t = 8)]
    sheet_cols: u32,
    /// Thumbnail size in pixels for --sheet.
    #[arg(long, default_value_t = 160)]
    thumb: u32,
    /// Rerun the census recorded in a JSON report (its spec replaces the tiling and rule flags).
    #[arg(long)]
    rerun: Option<PathBuf>,
    /// Worker threads (default: all cores). Results do not depend on this.
    #[arg(long)]
    threads: Option<usize>,
    /// No progress on stderr.
    #[arg(long)]
    quiet: bool,
    /// Run as a resumable catalog job (Ctrl-C pauses; `--resume JOB` continues).
    #[arg(long)]
    job: bool,
    /// Continue a paused or interrupted census job (its spec is kept; rule and tiling flags are
    /// ignored), then write the outputs.
    #[arg(long, value_name = "JOB")]
    resume: Option<i64>,
    /// Print a census job's progress and exit.
    #[arg(long, value_name = "JOB")]
    status: Option<i64>,
    /// Ask a running census job (in any process) to pause, and exit.
    #[arg(long, value_name = "JOB")]
    stop: Option<i64>,
    /// With --job or --resume: pause after this many rules in this session.
    #[arg(long)]
    limit: Option<u64>,
    /// Catalog for census jobs (default: $JETCARP_LIBRARY, else <data>/catalog.sqlite).
    #[arg(long)]
    library: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct ObjectsArgs {
    scene: PathBuf,
    /// Generations to run after replaying the scene's commands.
    #[arg(long, default_value_t = 0)]
    gens: u64,
    /// Print the full report as JSON.
    #[arg(long)]
    json: bool,
    /// List every object, not just the counts.
    #[arg(long)]
    list: bool,
    /// PNG with each object's cells coloured by class.
    #[arg(long)]
    png: Option<PathBuf>,
    #[arg(long)]
    scale: Option<f64>,
    #[arg(long, default_value_t = 1600)]
    max_side: u32,
    /// CA id to analyse (default: the first enabled B/S, Generations, WireWorld, or table CA).
    #[arg(long)]
    ca: Option<u32>,
    /// Longest period looked for.
    #[arg(long, default_value_t = 64)]
    max_period: u64,
    /// Cells within this many neighbour steps start in one group.
    #[arg(long, default_value_t = 2)]
    merge: u32,
    /// Do not split merged groups into non-interacting parts.
    #[arg(long)]
    no_split: bool,
    #[arg(long)]
    threads: Option<usize>,
}

fn set_threads(t: Option<usize>) -> Result<()> {
    if let Some(t) = t {
        rayon::ThreadPoolBuilder::new()
            .num_threads(t)
            .build_global()
            .context("thread pool")?;
    }
    Ok(())
}

fn parse_size(s: &str) -> Result<SizeSpec> {
    let l = s.to_ascii_lowercase();
    if let Some((w, h)) = l.split_once('x') {
        return Ok(SizeSpec::Rect {
            w: w.trim().parse().context("width")?,
            h: h.trim().parse().context("height")?,
        });
    }
    if let Some(n) = l.strip_prefix("level").or_else(|| l.strip_prefix('l')) {
        return Ok(SizeSpec::Level(n.trim().parse().context("level")?));
    }
    Ok(SizeSpec::Cells(
        l.trim()
            .parse()
            .context("size is WxH, a cell count, or L<level>")?,
    ))
}

fn parse_nb(s: &str) -> Result<Nb> {
    match s.to_ascii_lowercase().as_str() {
        "touch" | "moore" | "t" => Ok(Nb::Touch),
        "edge" | "vonneumann" | "von_neumann" | "e" => Ok(Nb::Edge),
        _ => bail!("--nb is touch or edge"),
    }
}

fn tiling_from(a: &CensusArgs) -> Result<TilingSpec> {
    if let Some(p) = &a.tiling {
        let text =
            std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        let t = v.get("tiling").cloned().unwrap_or(v);
        return serde_json::from_value(t).context("tiling JSON");
    }
    let boundary = match a.boundary.to_ascii_lowercase().as_str() {
        "wrap" | "torus" => Boundary::Wrap,
        "dead" => Boundary::Dead,
        "live" => Boundary::Live,
        _ => bail!("--boundary is wrap, dead, or live"),
    };
    Ok(TilingSpec::new(&a.family)
        .with_size(parse_size(&a.size)?)
        .with_boundary(boundary))
}

fn digits(s: &str, max: usize) -> Result<Vec<u32>> {
    let d =
        census::parse_digits(&s.replace("max", &max.to_string())).map_err(anyhow::Error::msg)?;
    Ok(d.into_iter().filter(|&x| x as usize <= max).collect())
}

fn spec_from(a: &CensusArgs) -> Result<CensusSpec> {
    if let Some(p) = &a.rerun {
        let text =
            std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        let s = v
            .get("spec")
            .cloned()
            .context("no \"spec\" in that report")?;
        return serde_json::from_value(s).context("census spec");
    }
    let tiling = tiling_from(a)?;
    let neighborhood = a.nb.as_deref().map(parse_nb).transpose()?;
    let mut rules = Vec::new();
    if let Some(r) = &a.rules {
        rules.extend(
            r.split([';', ' ', '\t', '\n'])
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .map(String::from),
        );
    }
    if let Some(p) = &a.rules_file {
        let text =
            std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?;
        rules.extend(census::parse_rule_list(&text));
    }
    if a.all || a.sample.is_some() {
        let topo = Topology::build(&tiling).map_err(anyhow::Error::msg)?;
        let max = census::max_degree(&topo, neighborhood.unwrap_or(Nb::Touch));
        let (b, s) = (digits(&a.birth, max)?, digits(&a.survive, max)?);
        let mut set = match a.sample {
            Some(n) => census::sample_bs_rules(&b, &s, a.states, n, a.sample_seed),
            None => census::all_bs_rules(&b, &s, a.states).map_err(anyhow::Error::msg)?,
        };
        if neighborhood == Some(Nb::Edge) {
            // Keep the notation honest about the neighbourhood.
            for r in &mut set {
                r.push('V');
            }
        }
        rules.extend(set);
    }
    if rules.is_empty() {
        bail!("no rules: use --rules, --rules-file, --all, or --sample");
    }
    Ok(CensusSpec {
        tiling,
        neighborhood,
        rules,
        soups: a.soups,
        density: a.density,
        gens: a.gens,
        seed: a.seed,
        explode_fraction: a.explode,
        activity_window: 10,
    })
}

fn class_color(c: Option<RunClass>) -> [u8; 4] {
    match c {
        Some(RunClass::Died) => [0x8c, 0x8c, 0x8c, 0xff],
        Some(RunClass::Frozen) => [0x4d, 0x9d, 0xff, 0xff],
        Some(RunClass::Oscillating) => [0x3d, 0xdc, 0x6d, 0xff],
        Some(RunClass::Active) => [0xff, 0xb0, 0x2e, 0xff],
        Some(RunClass::Exploded) => [0xff, 0x4d, 0x4d, 0xff],
        None => [0xff, 0x4d, 0xc4, 0xff],
    }
}

pub fn census(a: CensusArgs) -> Result<()> {
    if a.job || a.resume.is_some() || a.status.is_some() || a.stop.is_some() {
        return census_job(a);
    }
    set_threads(a.threads)?;
    let spec = spec_from(&a)?;
    let t0 = std::time::Instant::now();
    let quiet = a.quiet;
    let step = (spec.rules.len() * spec.soups as usize / 100).max(1);
    let progress = move |d: usize, n: usize| {
        if !quiet && (d.is_multiple_of(step) || d == n) {
            eprint!("\r{d}/{n} runs");
            if d == n {
                eprintln!();
            }
        }
    };
    let opts = CensusOptions {
        thumb_px: a.sheet.as_ref().map(|_| a.thumb),
        ..Default::default()
    };
    let out = census::run_census(&spec, &opts, Some(&progress)).map_err(anyhow::Error::msg)?;
    let secs = t0.elapsed().as_secs_f64();
    write_outputs(&a, &out.report, &out.thumbs)?;
    println!(
        "time:     {secs:.2}s on {} threads",
        rayon::current_num_threads()
    );
    Ok(())
}

/// `jetcarp census --job`, `--resume`, `--status`, `--stop`: the census as a catalog job.
fn census_job(a: CensusArgs) -> Result<()> {
    use jetcarp_core::library::Library;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let lib = match &a.library {
        Some(p) => Library::open(p.clone()),
        None => Library::open_default(),
    };
    fn msg<T>(r: Result<T, String>) -> Result<T> {
        r.map_err(anyhow::Error::msg)
    }
    if let Some(j) = a.status {
        let (_, p) = msg(census::job_status(&lib, j))?;
        println!("{}", p.line());
        return Ok(());
    }
    if let Some(j) = a.stop {
        msg(census::request_stop(&lib, j))?;
        eprintln!("asked census job {j} to pause at its next chunk");
        return Ok(());
    }
    let job = match a.resume {
        Some(j) => j,
        None => msg(census::start_job(&lib, &spec_from(&a)?))?,
    };
    eprintln!("census job {job} in {}", lib.path().display());
    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        let _ = ctrlc::set_handler(move || {
            stop.store(true, Ordering::SeqCst);
            eprintln!(
                "\nCtrl-C: pausing at the next chunk (`jetcarp census --resume {job}` continues)"
            );
        });
    }
    let quiet = a.quiet;
    let t0 = std::time::Instant::now();
    let o = census::CensusJobOptions {
        threads: a.threads.unwrap_or(0),
        stop,
        limit: a.limit,
        on_progress: Some(Arc::new(move |p: &census::CensusProgress| {
            if !quiet {
                eprint!("\r{}   ", p.line());
            }
        })),
        ..Default::default()
    };
    let p = msg(census::run_job(&lib, job, &o))?;
    if !quiet {
        eprintln!();
    }
    if p.status != "done" {
        eprintln!(
            "census job {job} paused at rule {}/{}; `jetcarp census --resume {job}` continues",
            p.rules_done, p.rules_total
        );
        return Ok(());
    }
    let r = msg(census::job_report(&lib, job))?;
    let thumbs = match &a.sheet {
        Some(_) => msg(census::thumbnails(&r.spec, a.thumb))?,
        None => vec![],
    };
    write_outputs(&a, &r, &thumbs)?;
    println!(
        "time:     {:.2}s this session ({:.0}s over every session)",
        t0.elapsed().as_secs_f64(),
        p.elapsed_s
    );
    Ok(())
}

fn write_outputs(a: &CensusArgs, r: &CensusReport, thumbs: &[Option<render::Image>]) -> Result<()> {
    let out_thumbs = thumbs;
    if let Some(p) = &a.csv {
        std::fs::write(p, r.to_csv()).with_context(|| format!("writing {}", p.display()))?;
    }
    if let Some(p) = &a.json {
        std::fs::write(p, r.to_json()).with_context(|| format!("writing {}", p.display()))?;
    }
    if let Some(p) = &a.sheet {
        let items: Vec<(sheet::Label, &render::Image)> = r
            .rules
            .iter()
            .zip(out_thumbs)
            .filter_map(|(s, t)| {
                let t = t.as_ref()?;
                let label = vec![
                    (s.notation.clone(), [0xd0, 0xd4, 0xdc, 0xff]),
                    (CensusReport::label(s), class_color(s.majority)),
                ];
                Some((label, t))
            })
            .collect();
        let img = sheet::labelled_sheet(&items, a.sheet_cols);
        let meta = vec![
            ("jetcarp:version", VERSION.to_string()),
            ("jetcarp:census", serde_json::to_string(&r.spec)?),
            ("jetcarp:dead_boundary", r.dead_boundary.to_string()),
        ];
        write_png(p, &img, &meta).map_err(anyhow::Error::msg)?;
    }
    print_census(r);
    for (what, p) in [("csv", &a.csv), ("json", &a.json), ("sheet", &a.sheet)] {
        if let Some(p) = p {
            println!("{what:9} {}", p.display());
        }
    }
    Ok(())
}

fn pct(x: f64) -> String {
    format!("{:.0}%", x * 100.0)
}

fn print_census(r: &CensusReport) {
    let s = &r.spec;
    println!("tiling:   {}", r.tiling);
    println!(
        "runs:     {} rules x {} soups, density {}, up to {} generations, seeds {}..{}, dead boundary {}",
        r.rules.len(),
        s.soups,
        s.density,
        s.gens,
        s.seed,
        s.seed + s.soups as u64 - 1,
        r.dead_boundary
    );
    let mut totals = [0usize; 6];
    for x in &r.rules {
        let k = match x.majority {
            Some(RunClass::Died) => 0,
            Some(RunClass::Frozen) => 1,
            Some(RunClass::Oscillating) => 2,
            Some(RunClass::Active) => 3,
            Some(RunClass::Exploded) => 4,
            None => 5,
        };
        totals[k] += 1;
    }
    println!(
        "majority: died {}, frozen {}, oscillating {}, active {}, exploded {}, errors {}",
        totals[0], totals[1], totals[2], totals[3], totals[4], totals[5]
    );
    println!(
        "\n{:<18} {:>5} {:>6} {:>5} {:>5} {:>6} {:>8} {:>8} {:>7} {:>8}  name",
        "rule", "died", "frozen", "osc", "act", "expl", "med.per", "settle", "peak", "activity"
    );
    let show = 60;
    for x in r.rules.iter().take(show) {
        if let Some(e) = &x.error {
            println!("{:<18} error: {e}", x.rule);
            continue;
        }
        let o = |v: Option<f64>| v.map(|v| format!("{v}")).unwrap_or_else(|| "-".into());
        println!(
            "{:<18} {:>5} {:>6} {:>5} {:>5} {:>6} {:>8} {:>8} {:>7} {:>8}  {}",
            x.notation,
            pct(x.died),
            pct(x.frozen),
            pct(x.oscillating),
            pct(x.active),
            pct(x.exploded),
            o(x.median_period),
            o(x.median_settle),
            x.peak_population,
            x.mean_activity
                .map(|a| format!("{a:.4}"))
                .unwrap_or_else(|| "-".into()),
            x.name.as_deref().unwrap_or("")
        );
    }
    if r.rules.len() > show {
        println!(
            "... {} more rules (see --csv or --json)",
            r.rules.len() - show
        );
    }
}

pub fn objects(a: ObjectsArgs) -> Result<()> {
    set_threads(a.threads)?;
    let text = std::fs::read_to_string(&a.scene)
        .with_context(|| format!("reading {}", a.scene.display()))?;
    let sc = Scene::from_json(&text).map_err(anyhow::Error::msg)?;
    let mut w = World::from_scene(&sc).map_err(anyhow::Error::msg)?;
    if a.gens > 0 {
        w.apply(Command::Step { gens: a.gens })
            .map_err(anyhow::Error::msg)?;
    }
    let t0 = std::time::Instant::now();
    let opts = ObjectOptions {
        ca: a.ca,
        max_period: a.max_period,
        merge_distance: a.merge,
        separate: !a.no_split,
        ..Default::default()
    };
    let r = find_objects(&w, &opts).map_err(anyhow::Error::msg)?;
    let secs = t0.elapsed().as_secs_f64();
    if let Some(p) = &a.png {
        let img = render_with_colors(
            &w,
            &r.colors(w.topo.len()),
            &RenderOptions {
                scale: a.scale,
                max_side: a.max_side,
                ..Default::default()
            },
        );
        let mut meta = png_metadata(&w);
        meta.push(("jetcarp:objects", r.summary_line()));
        write_png(p, &img, &meta).map_err(anyhow::Error::msg)?;
    }
    if a.json {
        println!("{}", r.to_json());
        return Ok(());
    }
    let t = r.class_totals();
    println!("scene:      {}", a.scene.display());
    println!("tiling:     {}", r.tiling);
    println!("generation: {}  hash {}", r.generation, r.hash);
    println!(
        "rule:       {} (CA {}, field {}), {} live cells, dead boundary {}",
        r.rule, r.ca_id, r.field, r.live_cells, r.dead_boundary
    );
    println!(
        "objects:    {} (still lifes {}, oscillators {}, spaceships {}, other {}), {} symmetries, periods up to {}",
        r.objects.len(),
        t[0],
        t[1],
        t[2],
        t[3],
        r.symmetries,
        r.max_period
    );
    println!("census:     {}", r.summary_line());
    println!(
        "\n{:>6}  {:<11} {:>6} {:>6}  {:<8}  name",
        "count", "class", "period", "cells", "key"
    );
    for c in &r.counts {
        let first = &r.objects[c.objects[0]];
        println!(
            "{:>6}  {:<11} {:>6} {:>6}  {:<8}  {}{}",
            c.count,
            c.class.kind(),
            c.class
                .period()
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".into()),
            first.size,
            if c.key.starts_with("other") {
                ""
            } else {
                &c.key
            },
            c.label(),
            c.class
                .speed()
                .map(|s| format!(", {s}"))
                .unwrap_or_default()
        );
    }
    if a.list {
        println!();
        for o in &r.objects {
            println!(
                "#{:<5} {:<28} cells {:>4}  centroid ({:.1}, {:.1})  {}",
                o.id, o.name, o.size, o.centroid[0], o.centroid[1], o.canonical
            );
        }
    }
    if let Some(p) = &a.png {
        println!("png:        {}", p.display());
    }
    println!(
        "time:       {secs:.2}s on {} threads",
        rayon::current_num_threads()
    );
    Ok(())
}

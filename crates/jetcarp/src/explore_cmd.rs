//! `jetcarp explore`: the broad search over tilings, rules, seeds and palettes.

use anyhow::{Context, Result, anyhow};
use jetcarp_core::explore::job::{self, Cfg};
use jetcarp_core::explore::sheet;
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct ExploreArgs {
    #[command(subcommand)]
    cmd: Sub,
}

#[derive(clap::Subcommand)]
enum Sub {
    /// Run the search until the time is up or `stop` is called. Resumes from the database.
    Run {
        /// The database file.
        #[arg(long)]
        db: PathBuf,
        /// Hours to run.
        #[arg(long, default_value_t = 1.0)]
        hours: f64,
        /// Stop at this Unix time instead of after `hours` (so restarts keep one deadline).
        #[arg(long)]
        until: Option<u64>,
        /// CPU threads for the rules the GPU cannot run.
        #[arg(long, default_value_t = 12)]
        cpu_threads: usize,
        /// Skip the GPU.
        #[arg(long)]
        no_gpu: bool,
        #[arg(long, default_value_t = 16000)]
        screen_cells: u32,
        #[arg(long, default_value_t = 1024)]
        screen_gens: u32,
        #[arg(long, default_value_t = 64000)]
        confirm_cells: u32,
        #[arg(long, default_value_t = 4096)]
        confirm_gens: u32,
        /// Worlds in a GPU batch.
        #[arg(long, default_value_t = 1024)]
        batch: usize,
        /// Stop if the process uses more than this much memory.
        #[arg(long, default_value_t = 72.0)]
        max_rss_gb: f64,
        #[arg(long, default_value_t = 0)]
        seed: u64,
    },
    /// Publish a search as an atlas file for Quick Start.
    Atlas {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Entries per category, family and tiling.
        #[arg(long, default_value_t = 12)]
        per_niche: usize,
        /// Lowest confirmed score (contrast picks are exempt).
        #[arg(long, default_value_t = 1.25)]
        min_score: f64,
        #[arg(long, default_value_t = 12_000)]
        max_entries: usize,
    },
    /// Render animated clips of a stratified sample of runs, with a manifest (for reviewing).
    Clips {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value_t = 96)]
        sample: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
        #[arg(long, default_value_t = 150)]
        px: u32,
        #[arg(long, default_value_t = 28)]
        frames: u32,
        /// Render these runs instead of a sample: one `id[ bucket]` per line.
        #[arg(long)]
        ids: Option<PathBuf>,
    },
    /// Recompute stored scores from stored features, after the scoring changes. Stop the search first.
    Rescore {
        #[arg(long)]
        db: PathBuf,
    },
    /// Ask a running search to stop.
    Stop {
        #[arg(long)]
        db: PathBuf,
    },
    /// Print the status a running search keeps up to date.
    Status {
        #[arg(long)]
        db: PathBuf,
    },
    /// Render a contact sheet of the runs a query returns (`SELECT id FROM runs ...`).
    Sheet {
        #[arg(long)]
        db: PathBuf,
        /// SQL returning run ids.
        #[arg(long)]
        sql: String,
        /// Where to write the PNG.
        #[arg(long)]
        out: PathBuf,
        /// Pixels per thumbnail.
        #[arg(long, default_value_t = 220)]
        px: u32,
        /// Thumbnails per row.
        #[arg(long, default_value_t = 9)]
        cols: u32,
        /// Fractions of the run to show, comma separated.
        #[arg(long, default_value = "0.125,0.5,1")]
        fracs: String,
        #[arg(long)]
        palette: Option<String>,
    },
}

pub fn run(a: ExploreArgs) -> Result<()> {
    match a.cmd {
        Sub::Run {
            db,
            hours,
            until,
            cpu_threads,
            no_gpu,
            screen_cells,
            screen_gens,
            confirm_cells,
            confirm_gens,
            batch,
            max_rss_gb,
            seed,
        } => {
            let cfg = Cfg {
                db,
                hours,
                until,
                cpu_threads,
                screen_cells,
                screen_gens,
                confirm_cells,
                confirm_gens,
                batch,
                max_rss_gb,
                seed,
                ..Default::default()
            };
            let backend = if no_gpu { None } else { gpu_backend() };
            job::run(cfg, backend).map_err(|e| anyhow!(e))
        }
        Sub::Atlas {
            db,
            out,
            per_niche,
            min_score,
            max_entries,
        } => {
            let conn = rusqlite_open(&db)?;
            let opts = jetcarp_core::explore::atlas::AtlasOpts {
                per_niche,
                min_score,
                max_entries,
            };
            let n = jetcarp_core::explore::atlas::build(&conn, &out, &opts).map_err(|e| anyhow!(e))?;
            println!("{n} entries in {}", out.display());
            Ok(())
        }
        Sub::Clips {
            db,
            out,
            sample,
            seed,
            px,
            frames,
            ids,
        } => {
            use jetcarp_core::explore::atlas;
            use rayon::prelude::*;
            let conn = rusqlite_open(&db)?;
            let mut buckets = std::collections::HashMap::new();
            let picks = match &ids {
                Some(path) => {
                    let text = std::fs::read_to_string(path).context("reading the id list")?;
                    let mut list = Vec::new();
                    for line in text.lines() {
                        let mut parts = line.split_whitespace();
                        let Some(id) = parts.next().and_then(|x| x.parse::<i64>().ok()) else {
                            continue;
                        };
                        list.push(id);
                        buckets.insert(id, parts.next().unwrap_or("").to_string());
                    }
                    atlas::runs_by_id(&conn, &list).map_err(|e| anyhow!(e))?
                }
                None => atlas::taste_sample(&conn, sample, seed).map_err(|e| anyhow!(e))?,
            };
            std::fs::create_dir_all(&out)?;
            let rows: Vec<serde_json::Value> = picks
                .par_iter()
                .filter_map(|(r, cat)| {
                    let bytes = atlas::clip(r, cat, frames, px).ok()?;
                    let name = format!("{}.gif", r.id);
                    std::fs::write(out.join(&name), bytes).ok()?;
                    Some(serde_json::json!({
                        "id": r.id, "file": name, "category": cat, "family": r.cand.fam.name(),
                        "bucket": buckets.get(&r.id).cloned().unwrap_or_default(),
                        "tiling": r.cand.tiling, "rule": r.cand.rule, "fate": r.fate,
                        "score": (r.score * 100.0).round() / 100.0, "seed": r.cand.seed.describe(),
                    }))
                })
                .collect();
            std::fs::write(
                out.join("manifest.json"),
                serde_json::to_string_pretty(&rows)?,
            )?;
            println!("{} clips in {}", rows.len(), out.display());
            Ok(())
        }
        Sub::Rescore { db } => {
            let n = job::rescore(&db).map_err(|e| anyhow!(e))?;
            println!("rescored {n} runs");
            Ok(())
        }
        Sub::Stop { db } => {
            std::fs::write(db.with_extension("stop"), "stop")?;
            println!("asked the search to stop");
            Ok(())
        }
        Sub::Status { db } => {
            let t = std::fs::read_to_string(db.with_extension("status.json"))
                .context("no status file yet")?;
            println!("{t}");
            Ok(())
        }
        Sub::Sheet {
            db,
            sql,
            out,
            px,
            cols,
            fracs,
            palette,
        } => {
            let conn = rusqlite_open(&db)?;
            let runs = sheet::query_runs(&conn, &sql).map_err(|e| anyhow!(e))?;
            let f: Vec<f64> = fracs
                .split(',')
                .filter_map(|x| x.trim().parse().ok())
                .collect();
            // Several comma-separated palettes compare them side by side, one row per run.
            let names: Vec<&str> = palette
                .as_deref()
                .map_or(vec![], |p| p.split(',').collect());
            let img = if names.len() > 1 {
                sheet::palette_sheet(&runs, &names, f.first().copied().unwrap_or(1.0), px)
            } else {
                sheet::sheet(&runs, &f, px, cols, palette.as_deref())
            };
            jetcarp_core::render::write_png(&out, &img, &[]).map_err(|e| anyhow!(e))?;
            println!("{} runs, {}", runs.len(), out.display());
            Ok(())
        }
    }
}

fn rusqlite_open(db: &PathBuf) -> Result<jetcarp_core::explore::Db> {
    jetcarp_core::explore::open_read(db).map_err(|e| anyhow!(e))
}

/// The GPU batch stepper as a search backend.
struct Gpu(jetcarp_gpu::batch::BatchSim);

impl job::Backend for Gpu {
    fn name(&self) -> String {
        self.0.name().to_string()
    }
    fn supports(&self, rule: &jetcarp_core::rules::Rule) -> bool {
        jetcarp_gpu::batch::BatchSim::supports(rule)
    }
    fn max_cells(&self) -> usize {
        self.0.max_cells()
    }
    fn run(
        &mut self,
        job: &jetcarp_core::explore::batch::BatchJob,
    ) -> Result<Vec<jetcarp_core::explore::batch::WorldTrace>, String> {
        self.0.run(job)
    }
    fn reset(&mut self) -> bool {
        match jetcarp_gpu::batch::BatchSim::headless() {
            Ok(s) => {
                self.0 = s;
                true
            }
            Err(_) => false,
        }
    }
}

fn gpu_backend() -> Option<Box<dyn job::Backend>> {
    match jetcarp_gpu::batch::BatchSim::headless() {
        Ok(s) => {
            eprintln!("GPU: {}", s.name());
            Some(Box::new(Gpu(s)))
        }
        Err(e) => {
            eprintln!("no GPU ({e}): running on the CPU only");
            None
        }
    }
}

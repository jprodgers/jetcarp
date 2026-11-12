//! `jetcarp script`: run a Rhai script (see `jetcarp_script`), with progress on stderr.

use anyhow::{Context, Result, bail};
use clap::Args;
use jetcarp_script::{Event, RunOptions, Screen};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Args)]
#[command(
    after_help = "Example scripts are built in, as in the app's Examples menu: --examples lists them and --example NAME runs one (with --check, checks it; it writes into the output folder like any script)."
)]
pub struct ScriptArgs {
    /// The script (.rhai). Omit with --functions, --examples or --example.
    file: Option<PathBuf>,
    /// Run a built-in example script instead of a file (`hexw-all`, `wolfram-all`, ...; the
    /// `.rhai` may be left off).
    #[arg(long, value_name = "NAME", conflicts_with = "file")]
    example: Option<String>,
    /// List the built-in example scripts and exit.
    #[arg(long)]
    examples: bool,
    /// Script parameter, seen as `params.key` (repeatable): --set from=0 --set label=test
    #[arg(long = "set", value_name = "KEY=VALUE")]
    set: Vec<String>,
    /// Screen size that `size: "screen"` fills and images default to.
    #[arg(long, default_value = "1920x1080")]
    screen: String,
    /// Tile size in pixels for screen-sized worlds (the app's Fill the view tile size).
    #[arg(long, default_value_t = 8.0)]
    cell_px: f64,
    /// Worker threads (default: all cores). Results do not depend on it.
    #[arg(long)]
    threads: Option<usize>,
    /// Output root instead of $JETCARP_OUT or <Pictures>/JETCARP.
    #[arg(long)]
    out: Option<PathBuf>,
    /// Only check the script for syntax errors.
    #[arg(long)]
    check: bool,
    /// Stop the script after this many seconds.
    #[arg(long)]
    max_seconds: Option<f64>,
    /// Print the summary as JSON on stdout at the end.
    #[arg(long)]
    json: bool,
    /// Print the function reference and exit.
    #[arg(long)]
    functions: bool,
}

/// The first paragraph of an example's opening comment, on one line.
fn example_summary(src: &str) -> String {
    src.lines()
        .map(str::trim)
        .take_while(|l| l.starts_with("//") && l.trim_start_matches('/').trim() != "")
        .map(|l| l.trim_start_matches('/').trim())
        .collect::<Vec<_>>()
        .join(" ")
}

/// A built-in example by name, with or without `.rhai`.
fn example(name: &str) -> Result<(&'static str, &'static str)> {
    let want = name.trim().trim_end_matches(".rhai");
    jetcarp_script::EXAMPLES
        .iter()
        .find(|(n, _)| n.trim_end_matches(".rhai") == want)
        .copied()
        .with_context(|| {
            let names: Vec<&str> = jetcarp_script::EXAMPLES
                .iter()
                .map(|(n, _)| n.trim_end_matches(".rhai"))
                .collect();
            format!(
                "no example script '{name}'; the examples are {}",
                names.join(", ")
            )
        })
}

pub fn script(a: ScriptArgs) -> Result<()> {
    if a.functions {
        print!("{}", jetcarp_script::help_text());
        return Ok(());
    }
    if a.examples {
        for (n, src) in jetcarp_script::EXAMPLES {
            println!(
                "{}
    {}",
                n.trim_end_matches(".rhai"),
                example_summary(src)
            );
        }
        println!(
            "
Run one with: jetcarp script --example NAME"
        );
        return Ok(());
    }
    let (file, source) = match (&a.file, &a.example) {
        (Some(f), _) => (
            f.clone(),
            std::fs::read_to_string(f).with_context(|| format!("reading {}", f.display()))?,
        ),
        (None, Some(name)) => {
            let (n, src) = example(name)?;
            (PathBuf::from(n), src.to_string())
        }
        (None, None) => bail!(
            "give a script file, or --example NAME (--examples lists them), or --functions for the function reference"
        ),
    };
    let is_example = a.file.is_none();
    if a.check {
        return match jetcarp_script::check(&source) {
            Ok(()) => {
                println!("ok: {}", file.display());
                Ok(())
            }
            Err(e) => bail!("{}: {e}", file.display()),
        };
    }
    let (width, height) = Screen::parse_size(&a.screen).map_err(anyhow::Error::msg)?;
    let mut opts = RunOptions {
        screen: Screen {
            width,
            height,
            cell_px: a.cell_px,
        },
        threads: a.threads,
        out_root: a.out.clone(),
        // A built-in example has no file: run.json keeps its source.
        script_path: (!is_example).then(|| std::fs::canonicalize(&file).unwrap_or(file.clone())),
        ..Default::default()
    };
    opts.limits.max_seconds = a.max_seconds;
    for kv in &a.set {
        let (k, v) = kv
            .split_once('=')
            .with_context(|| format!("--set wants KEY=VALUE, got '{kv}'"))?;
        opts.params.insert(k.trim().to_string(), v.to_string());
    }
    let tty = std::io::stderr().is_terminal();
    let last = Mutex::new((Instant::now(), false));
    let json = a.json;
    let on_event = move |e: Event| match e {
        Event::Log(line) => {
            let mut l = last.lock().unwrap();
            if l.1 {
                eprint!("\r\x1b[2K");
                l.1 = false;
            }
            if json {
                eprintln!("{line}");
            } else {
                println!("{line}");
            }
        }
        // No live view here: `show(w)` already logged a line.
        Event::Show(_) => {}
        Event::Progress(p) => {
            let mut l = last.lock().unwrap();
            let end = p.done >= p.total;
            if tty {
                eprint!("\r\x1b[2K{}", p.line());
                let _ = std::io::stderr().flush();
                l.1 = true;
                if end {
                    eprintln!();
                    l.1 = false;
                }
            } else if end || l.0.elapsed().as_secs_f64() >= 5.0 {
                eprintln!("{}", p.line());
                l.0 = Instant::now();
            }
        }
    };
    // Ctrl-C: the first press cancels (sweeps finish the runs in flight and write their manifest,
    // so the folder can be resumed); a second press quits at once.
    let cancel = Arc::new(AtomicBool::new(false));
    let presses = Arc::new(AtomicU32::new(0));
    {
        let (cancel, presses) = (cancel.clone(), presses.clone());
        let _ = ctrlc::set_handler(move || {
            if presses.fetch_add(1, Ordering::SeqCst) == 0 {
                cancel.store(true, Ordering::SeqCst);
                eprintln!(
                    "\nCtrl-C: stopping after the runs in flight and saving the manifest (press Ctrl-C again to quit now)"
                );
            } else {
                eprintln!(
                    "\nCtrl-C again: quitting; the folder can still be resumed from manifest.partial.jsonl"
                );
                std::process::exit(130);
            }
        });
    }
    let s = jetcarp_script::run(&source, opts, on_event, cancel.clone());

    if json {
        println!("{}", serde_json::to_string_pretty(&s.to_json())?);
    } else {
        for d in &s.output_dirs {
            eprintln!("output: {}", d.display());
        }
        eprintln!("time:   {:.2}s", s.seconds);
    }
    if cancel.load(Ordering::SeqCst) {
        eprintln!("cancelled after {:.1}s", s.seconds);
        for sw in &s.sweeps {
            eprintln!(
                "  {}: {} of {} runs done",
                sw["folder"].as_str().unwrap_or("?"),
                sw["completed"],
                sw["runs"]
            );
        }
        let resumable: Vec<_> = s
            .output_dirs
            .iter()
            .filter(|d| d.join("manifest.partial.jsonl").exists())
            .collect();
        for d in &resumable {
            if source.contains("params.folder") {
                let run_as = if is_example {
                    format!(
                        "--example {}",
                        file.display().to_string().trim_end_matches(".rhai")
                    )
                } else {
                    file.display().to_string()
                };
                eprintln!(
                    "resume: jetcarp script {run_as} --set folder={}",
                    d.display()
                );
            } else {
                eprintln!(
                    "resume: run the same sweep with folder: \"{}\" (finished runs are skipped)",
                    d.display()
                );
            }
        }
        std::process::exit(130);
    }
    if let Some(e) = &s.error {
        bail!("{}: {e}", file.display());
    }
    Ok(())
}

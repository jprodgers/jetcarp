//! `run_script`: a Rhai script, bounded in time and world size, writing only inside the output
//! folder (and the folders the server was allowed), reading only there too.

use crate::tools::{Call, MAX_CELLS, MAX_SCRIPT_SECONDS, Res, Tool, ToolOutput};
use serde_json::{Value, json};
use std::fmt::Write as _;

pub(crate) fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "run_script",
        title: "Run a JETCARP script",
        read_only: false,
        destructive: false,
        description: || {
            format!(
                "Runs a JETCARP script (the Rhai language) and returns its log, errors with line numbers, output folders and sweep summaries. Scripts make their own worlds (the current world is not changed), run them until something happens, save PNGs, and run parallel rule sweeps into new folders under the output folder. Example: `let w = world(#{{ tiling: \"hex\", size: [120, 80], boundary: \"dead\" }}); w.add_ca(\"HexW3 8229\", #{{ seed: \"top_center\" }}); let r = w.run(#{{ until: \"done\" }}); print(r.generations); w.save_png(out_dir(\"test\") + \"/a.png\");` or `sweep(#{{ rules: \"W\", until: \"done\", png: #{{ size: [400, 300] }} }});`. help {{\"topic\": \"scripts\"}} lists every function. Limits: {} seconds at most (default 60, `max_seconds`), worlds of at most {} cells; files are written only inside the output folder and read only there. Stops early when the client cancels the call; with a progress token, sweeps report their progress.",
                MAX_SCRIPT_SECONDS, MAX_CELLS
            )
        },
        schema: || {
            json!({"type": "object", "properties": {
                "script": {"type": "string", "description": "The script text (Rhai)."},
                "params": {"type": "object", "description": "Values the script reads as `params.key`."},
                "max_seconds": {"type": "number", "minimum": 1, "maximum": MAX_SCRIPT_SECONDS, "description": "Stop the script after this many seconds (default 60)."},
                "screen": {"type": "string", "description": "Screen size for size: \"screen\" worlds and the default image size, e.g. \"1920x1080\" (default)."},
                "cell_px": {"type": "number", "minimum": 1, "maximum": 256, "description": "Tile size in pixels for screen-sized worlds (default 8)."},
                "log_lines": {"type": "integer", "minimum": 1, "maximum": 2000, "description": "Most log lines to return, the last ones (default 200)."},
                "check_only": {"type": "boolean", "description": "Only check the script for syntax errors."},
            }, "required": ["script"], "additionalProperties": false})
        },
        run: run_script,
    }]
}

fn num(a: &Value, k: &str) -> Result<Option<f64>, String> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .map(Some)
            .ok_or_else(|| format!("run_script: `{k}` must be a number, got {v}")),
    }
}

fn run_script(c: &mut Call) -> Res {
    let a = c.args;
    let src = a
        .get("script")
        .and_then(Value::as_str)
        .ok_or("run_script: give the script text in `script`")?;
    let check_only = match a.get("check_only") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) if s == "true" || s == "false" => s == "true",
        Some(v) => {
            return Err(format!(
                "run_script: `check_only` is true or false, not {v}"
            ));
        }
    };
    if check_only {
        return Ok(match jetcarp_script::check(src) {
            Ok(()) => ToolOutput::text("ok: no syntax errors"),
            Err(e) => ToolOutput::error(format!("syntax error at {e}")),
        });
    }
    let cx = crate::tools::ctx();
    let limits = &cx.limits;
    let mut opts = jetcarp_script::RunOptions::default();
    if let Some(s) = a.get("screen").and_then(Value::as_str) {
        let (w, h) = jetcarp_api::args::Screen::parse_size(s)?;
        opts.screen.width = w;
        opts.screen.height = h;
    }
    if let Some(px) = num(a, "cell_px")? {
        opts.screen.cell_px = px.clamp(1.0, 256.0);
    }
    if let Some(p) = a.get("params").filter(|p| !p.is_null()) {
        for (k, v) in p
            .as_object()
            .ok_or("run_script: `params` must be an object")?
        {
            let text = match v {
                Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            opts.params.insert(k.clone(), text);
        }
    }
    let secs = num(a, "max_seconds")?.unwrap_or(60.0);
    opts.limits.max_seconds = Some(secs.clamp(1.0, limits.max_script_seconds));
    opts.limits.max_cells = Some(limits.max_cells);
    opts.limits.restrict_reads = true;
    opts.allow_dirs = crate::tools::folders().allowed;
    let keep = num(a, "log_lines")?.unwrap_or(200.0).clamp(1.0, 2000.0) as usize;
    opts.limits.max_log_lines = 20_000;
    // Progress (sweeps, `progress()` in scripts) at most once a second, and at the end.
    let progress = c.progress.clone();
    let last = std::sync::Mutex::new(None::<std::time::Instant>);
    let on_event = move |e: jetcarp_script::Event| {
        if let (Some(f), jetcarp_script::Event::Progress(p)) = (&progress, e) {
            let mut l = last.lock().unwrap();
            let end = p.total > 0 && p.done >= p.total;
            if end || l.is_none_or(|t| t.elapsed().as_secs_f64() >= 1.0) {
                *l = Some(std::time::Instant::now());
                f(
                    p.done as f64,
                    (p.total > 0).then_some(p.total as f64),
                    p.line(),
                );
            }
        }
    };
    let stop = c.cancel.clone().unwrap_or_default();
    let s = jetcarp_script::run(src, opts, on_event, stop.clone());
    let dirs = || {
        s.output_dirs
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    if stop.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(ToolOutput::error(format!(
            "cancelled after {:.1} s ({} log lines; output folders: {})",
            s.seconds,
            s.log.len(),
            dirs()
        )));
    }
    let skip = s.log.len().saturating_sub(keep);
    let mut out = String::new();
    match &s.error {
        None => {
            let _ = writeln!(out, "ok: finished in {:.2} s", s.seconds);
        }
        Some(e) if s.timed_out => {
            let _ = writeln!(
                out,
                "stopped at the time limit after {:.1} s ({e})",
                s.seconds
            );
        }
        Some(e) => {
            let _ = writeln!(out, "error at {e}");
        }
    }
    for d in &s.output_dirs {
        let _ = writeln!(out, "output folder: {}", d.display());
    }
    for sw in &s.sweeps {
        let _ = writeln!(out, "sweep: {sw}");
    }
    if !s.result.is_empty() {
        let r: String = s.result.chars().take(2000).collect();
        let _ = writeln!(out, "result: {r}");
    }
    if skip > 0 || s.dropped_log_lines > 0 {
        let _ = writeln!(
            out,
            "log (last {keep} of {} lines):",
            s.log.len() as u64 + s.dropped_log_lines
        );
    } else if !s.log.is_empty() {
        let _ = writeln!(out, "log:");
    }
    for l in &s.log[skip..] {
        let _ = writeln!(out, "{l}");
    }
    Ok(if s.ok {
        ToolOutput::text(out)
    } else {
        ToolOutput::error(out)
    })
}

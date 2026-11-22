//! `jetcarp score` (measure this machine: the JETCARP Hardware Score) and `jetcarp limits`
//! (show, set and reset the limits set from it): the `machine.*` operations.
//!
//! Before measuring, `jetcarp score` asks for other heavy programs to be closed, holds the rest
//! of JETCARP still (#301), and says what would still lower the score: a JETCARP program that
//! can't hold still, or a busy processor or GPU. In a terminal it then asks whether to wait or
//! measure anyway; otherwise it measures anyway and says so, so scripts never wait on a
//! question. A score measured that way is marked as taken under load.

use crate::{ProgressLine, show};
use anyhow::Result;
use clap::{Args, Subcommand};
use jetcarp_api::Ctx;
use jetcarp_api::op::{NoInput, Op, Quiet};
use jetcarp_api::ops::machine::{
    Before, HEAVY_WARNING, LimitResetIn, LimitSetIn, MachineLimits, MachineLimitsReset,
    MachineLimitsSet, ScoreIn, measure,
};

#[derive(Args)]
pub struct ScoreArgs {
    /// Also write a report here, in Markdown and JSON (`report.md` and `report.json`), to
    /// compare machines. Nothing is sent anywhere.
    #[arg(long, value_name = "FILE")]
    pub report: Option<String>,
    /// Each workload once, for its answers alone (seconds, not a minute; the rates mean little).
    #[arg(long)]
    pub quick: bool,
    /// Leave the GPU out (default: measured when the GPU setting allows it).
    #[arg(long)]
    pub no_gpu: bool,
    /// Do not keep the score in machine.json.
    #[arg(long)]
    pub no_save: bool,
}

#[derive(Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct LimitsArgs {
    #[command(subcommand)]
    pub sub: Option<LimitsSub>,
}

#[derive(Subcommand)]
pub enum LimitsSub {
    /// Set a limit to a value of your own (kept in limits.json): memory as 24GB or 512MB, counts
    /// as 64000000 or 64M, an AI's share as 0.25, 25%, small or all. Setting reserved-system or
    /// reserved-jetcarp sets the other.
    Set {
        /// reserved-system, reserved-jetcarp, world, gpu-world, host-cells, host-memory,
        /// hashlife-nodes, hunt-cache, media-pixels, wallpaper-cells, animation-frames, ai-share.
        name: String,
        /// The value in the limit's unit: 24GB, 64M, 25%...
        value: String,
    },
    /// Put a limit back to the machine's suggestion (with no name: every one).
    Reset { name: Option<String> },
}

pub fn score(cx: &Ctx, a: ScoreArgs, json: bool) -> Result<()> {
    let i = ScoreIn {
        quick: a.quick,
        gpu: a.no_gpu.then_some(false),
        report: a.report,
        save: Some(!a.no_save),
    };
    if !json {
        eprintln!("{HEAVY_WARNING}\n");
    }
    // Ctrl-C, SIGTERM or SIGHUP: the rest of JETCARP carries on at once, rather than once the
    // hold goes stale, and the score ends as a shell expects a program the signal ended to (#278's
    // shared signal handling: `ctrlc` is Windows-only now).
    let _ = crate::stop_signal::on_stop(|sig| {
        jetcarp_core::machine::quiet::end_now();
        jetcarp_core::machine::pool::exit(crate::stop_signal::status(sig));
    });
    let mut before = Before::check(a.quick);
    if before.busy() {
        if !json {
            eprintln!("{}", before.text());
        }
        if !json && interactive() {
            if !wait_or_go(&mut before) {
                anyhow::bail!("not measured");
            }
        } else if !json {
            eprintln!("Measuring anyway (no terminal to ask in).");
        }
    }
    if !json {
        let held = before.paused_text();
        if !held.is_empty() {
            eprintln!("{held}");
        }
        eprintln!(
            "Measuring this computer ({}): JETCARP's own work on fixed scenes, each answer \
             checked. Nothing is drawn or sent anywhere.",
            if a.quick {
                "a short run, a few seconds"
            } else {
                "about a minute"
            }
        );
    }
    let r = if json {
        measure(cx, i, &before, &Quiet)
    } else {
        let p = ProgressLine::steps();
        let r = measure(cx, i, &before, &p);
        p.erase();
        r
    };
    drop(before);
    show(&r?, json)
}

/// Standard input and standard error are a terminal: someone can be asked.
fn interactive() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// Asked in a terminal, while something would lower the score: wait for it to go (looking
/// again each second, saying what is left when it changes), measure anyway, or stop. True to
/// measure.
fn wait_or_go(before: &mut Before) -> bool {
    loop {
        let a = crate::setup_cmd::ask("Wait for them (w), measure anyway (m), or stop (s)? [w] ");
        match a.as_deref().map(str::to_ascii_lowercase).as_deref() {
            None | Some("s" | "stop") => return false,
            Some("m" | "measure" | "anyway") => return true,
            Some("" | "w" | "wait") => break,
            Some(_) => eprintln!("  type w, m or s"),
        }
    }
    eprintln!("Waiting (Ctrl-C stops)...");
    let mut said = before.text();
    while before.busy() {
        before.look(false);
        let now = before.text();
        if before.busy() && now != said {
            eprintln!("{now}");
            said = now;
        }
    }
    eprintln!("They have gone.");
    true
}

pub fn limits(cx: &Ctx, a: LimitsArgs, json: bool) -> Result<()> {
    let out = match a.sub {
        None => return show(&MachineLimits::run(cx, NoInput {}, &Quiet)?, json),
        Some(LimitsSub::Set { name, value }) => {
            MachineLimitsSet::run(cx, LimitSetIn { name, value }, &Quiet)?
        }
        Some(LimitsSub::Reset { name }) => {
            MachineLimitsReset::run(cx, LimitResetIn { name }, &Quiet)?
        }
    };
    // A running desktop background reads its limits again (none running: nothing to tell).
    let _ = jetcarp_mcp::link::host_call(&serde_json::json!({"action": "reload"}));
    show(&out, json)
}

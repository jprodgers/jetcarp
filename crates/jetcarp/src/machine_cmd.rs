//! `jetcarp score` (measure this machine: the JETCARP Hardware Score) and `jetcarp limits`
//! (show, set and reset the limits set from it): the `machine.*` operations.

use crate::{ProgressLine, show};
use anyhow::Result;
use clap::{Args, Subcommand};
use jetcarp_api::Ctx;
use jetcarp_api::op::{NoInput, Op, Quiet};
use jetcarp_api::ops::machine::{
    LimitResetIn, LimitSetIn, MachineLimits, MachineLimitsReset, MachineLimitsSet, MachineScore,
    ScoreIn,
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
        MachineScore::run(cx, i, &Quiet)
    } else {
        let p = ProgressLine::steps();
        let r = MachineScore::run(cx, i, &p);
        p.erase();
        r
    };
    show(&r?, json)
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

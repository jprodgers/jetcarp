//! `jetcarp setup`: the first-run setup from a terminal, through the operations the app's
//! setup wizard calls (`setup.status`, `setup.apply`). With no `--profile` it asks, in a
//! terminal; with `--profile NAME --yes` it asks nothing (scripts, tests). Either way it prints
//! what it will do, then does it, the jobs' progress on standard error. The desktop offers
//! (Desktop art, Everything) are questions in a terminal and printed commands otherwise: setup
//! never changes the desktop unasked. Adding JETCARP to the system's apps, for every profile, is
//! like measuring the computer instead, as in the app: when it isn't there yet it is asked in a
//! terminal (yes by default) and done with `--yes`, unless `--no-launcher` (#105).
//!
//! Before anything else it asks the photosensitive seizure warning (#96), unless it was answered
//! before; `--seizure-warning` answers it instead, and `--yes` needs one or the other.

use crate::search_cmd::CliProgress;
use crate::show;
use anyhow::{Result, bail};
use jetcarp_api::op::{Op, Quiet, Report};
use jetcarp_api::ops::setup::{self, ApplyIn, Profile, SetupApply, SetupStatus};
use jetcarp_api::ops::system::human_bytes;
use jetcarp_app::desktop_offers::Offer;
use jetcarp_core::photosensitivity::{self as ps, Answer};
use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;

#[derive(clap::Args)]
pub struct SetupArgs {
    /// The profile: desktop-art, explorer, researcher, everything, or blank-slate. Asked for
    /// in a terminal when left out.
    #[arg(long)]
    profile: Option<String>,
    /// Keep JETCARP's data (the library, its media and search folders) in this folder from now
    /// on, for big libraries; recorded in paths.json in the settings folder. It must be empty, or
    /// hold a JETCARP data folder already: nothing is moved.
    #[arg(long, value_name = "DIR")]
    data_dir: Option<PathBuf>,
    /// Minutes of the Quick Start build: a search whose best runs are added to the Quick Start
    /// (default: the profile's, 10, 30 or 60; 0: none).
    #[arg(long, value_name = "N")]
    quickstart_minutes: Option<f64>,
    /// Import the packs in this folder too: a pack store, an export, or a folder holding them
    /// (repeatable).
    #[arg(long, value_name = "DIR")]
    packs: Vec<PathBuf>,
    /// Ask nothing: do what the profile does (the desktop offers are only printed). The
    /// photosensitive seizure warning must have been answered before, or with --seizure-warning.
    #[arg(long, short = 'y')]
    yes: bool,
    /// Answer the photosensitive seizure warning without being asked (saved in the settings
    /// folder). Setup asks it first in a terminal; with --yes, or no terminal, it needs this the
    /// first time. With no --profile, only the answer is saved, unless setup can go on to ask the
    /// rest (in a terminal, without --yes).
    #[arg(long, value_enum, value_name = "ANSWER")]
    seizure_warning: Option<WarningAnswer>,
    /// Only show where things stand: whether setup has run, the folders, the packs that came
    /// with JETCARP, the GPU, and what each profile takes here.
    #[arg(long)]
    status: bool,
    /// The Quick Start build runs on the CPU even when there is a GPU.
    #[arg(long)]
    no_gpu: bool,
    /// Worker threads (default: all cores, and all but two for the Quick Start build).
    #[arg(long)]
    threads: Option<usize>,
    /// No progress on standard error.
    #[arg(long)]
    quiet: bool,
    /// Do not measure this computer (the JETCARP Hardware Score, about a minute, which setup
    /// does first unless told not to: asked in a terminal, done with --yes).
    #[arg(long)]
    no_measure: bool,
    /// Do not add JETCARP to the system's apps (the Start menu, Launchpad, the apps menu), which
    /// setup does when it isn't there yet unless told not to: asked in a terminal, done with
    /// --yes (`jetcarp launcher install`).
    #[arg(long)]
    no_launcher: bool,
    /// Measure with the short run, each workload once (tests).
    #[arg(long, hide = true)]
    measure_quick: bool,
    /// The Quick Start build searches only these tilings (tests; default: every one).
    #[arg(long, hide = true)]
    build_tilings: Option<String>,
    /// The Quick Start build searches only these rule families (tests; default: every one).
    #[arg(long, hide = true)]
    build_families: Option<String>,
}

/// `--seizure-warning`'s answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum WarningAnswer {
    /// You don't have epilepsy or a seizure disorder.
    NoDisorder,
    /// You have, or may have, a seizure disorder, and go on at your own risk, against the
    /// warning's advice not to use JETCARP.
    AtRisk,
}

impl WarningAnswer {
    fn answer(self) -> Answer {
        match self {
            WarningAnswer::NoDisorder => Answer::NoDisorder,
            WarningAnswer::AtRisk => Answer::AtRisk,
        }
    }
}

/// How the photosensitive seizure warning came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Warned {
    /// Answered now, or before: setup goes on.
    Accepted,
    /// They have, or may have, a seizure disorder, and chose to quit.
    Quit,
}

/// The photosensitive seizure warning, before anything else (#96): an answer given with
/// `--seizure-warning` is saved; else one saved before stands; else it is asked in a terminal,
/// and with no terminal (or `--yes`) setup refuses, with the warning.
fn seizure_warning(a: &SetupArgs, interactive: bool) -> Result<Warned> {
    if let Some(w) = a.seizure_warning {
        let rec = ps::record(w.answer(), "setup").map_err(anyhow::Error::msg)?;
        if rec.answer == Answer::AtRisk {
            eprintln!("{}\n", ps::risk_text());
        }
        eprintln!(
            "saved your answer to the photosensitive seizure warning: {}",
            rec.answer.words()
        );
        return Ok(Warned::Accepted);
    }
    if ps::accepted() {
        return Ok(Warned::Accepted);
    }
    if !interactive {
        bail!(
            "JETCARP's photosensitive seizure warning has not been answered, and setup cannot ask \
             it here (--yes, or no terminal).\n\n{}\n\nIf you don't have \
             epilepsy or a seizure disorder, add `--seizure-warning no-disorder`. If you have, or \
             may have, one, please don't use JETCARP.",
            ps::text()
        );
    }
    eprintln!("{}\n", ps::text());
    eprintln!("{}", ps::QUESTION);
    eprintln!("  1. {}", ps::NO_DISORDER);
    eprintln!("  2. {}", ps::AT_RISK);
    loop {
        let Some(t) = ask("Choose 1 or 2: ") else {
            bail!("the photosensitive seizure warning was not answered; nothing changed");
        };
        match t.as_str() {
            "1" => {
                ps::record(Answer::NoDisorder, "setup").map_err(anyhow::Error::msg)?;
                eprintln!();
                return Ok(Warned::Accepted);
            }
            "2" => break,
            _ => eprintln!("  type 1 or 2"),
        }
    }
    eprintln!("\n{}\n", ps::risk_text());
    let t = ask("Press Enter to quit, or type c and Enter to continue at your own risk: ")
        .unwrap_or_default();
    if t.eq_ignore_ascii_case("c") {
        ps::record(Answer::AtRisk, "setup").map_err(anyhow::Error::msg)?;
        eprintln!();
        Ok(Warned::Accepted)
    } else {
        eprintln!("JETCARP was not set up.");
        Ok(Warned::Quit)
    }
}

/// A line from the terminal, trimmed (`None` at the end of input).
pub(crate) fn ask(prompt: &str) -> Option<String> {
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    let mut s = String::new();
    match std::io::stdin().lock().read_line(&mut s) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(s.trim().to_string()),
    }
}

/// A yes or no, with a default.
fn yes_no(prompt: &str, default: bool) -> bool {
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    match ask(&format!("{prompt} {hint} ")) {
        Some(a) if a.is_empty() => default,
        Some(a) => matches!(a.to_ascii_lowercase().as_str(), "y" | "yes"),
        None => default,
    }
}

/// The profiles, numbered, with what each takes here.
fn print_profiles(st: &SetupStatus) {
    eprintln!("What would you like JETCARP for?");
    for (i, p) in Profile::ALL.iter().enumerate() {
        let cost = st.info(*p).map(|i| i.cost.clone()).unwrap_or_default();
        eprintln!("  {}. {} ({})", i + 1, p.title(), p.name());
        eprintln!("     {}", p.summary());
        eprintln!("     {}", capitalise(&cost));
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// `s` with its first letter in lower case, to go on a sentence ("Add JETCARP to …" as "add …").
fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Ask for a profile (Explorer by default).
fn ask_profile(st: &SetupStatus) -> Result<Profile> {
    print_profiles(st);
    loop {
        let Some(a) = ask("Choose 1 to 5 [2]: ") else {
            bail!("no profile chosen");
        };
        if a.is_empty() {
            return Ok(Profile::Explorer);
        }
        if let Ok(n) = a.parse::<usize>()
            && (1..=5).contains(&n)
        {
            return Ok(Profile::ALL[n - 1]);
        }
        match Profile::parse(&a) {
            Ok(p) => return Ok(p),
            Err(_) => eprintln!("  type a number from 1 to 5, or a profile's name"),
        }
    }
}

pub fn run(a: SetupArgs, json: bool) -> Result<()> {
    let cx = crate::platform::ctx();
    if a.status {
        return show(&setup::status(&cx), json);
    }
    let terminal = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let interactive = terminal && !a.yes;
    // The photosensitive seizure warning comes first (#96).
    if seizure_warning(&a, interactive)? == Warned::Quit {
        return Ok(());
    }
    // `--seizure-warning` alone, with nothing to ask the rest in: only the answer is saved.
    if a.seizure_warning.is_some() && a.profile.is_none() && !interactive {
        return Ok(());
    }
    if a.yes && a.profile.is_none() {
        bail!(
            "--yes needs a --profile (desktop-art, explorer, researcher, everything, or blank-slate)"
        );
    }
    let st = setup::status(&cx);
    let profile = match &a.profile {
        Some(p) => Profile::parse(p)?,
        None if terminal => {
            eprintln!(
                "JETCARP runs cellular automata on almost any tiling of the plane. A few questions set it up\nfor what you want to do; `jetcarp setup` again changes your mind later.\n"
            );
            if let Some(r) = &st.record {
                eprintln!(
                    "(Setup last ran on {} with {}.)\n",
                    r.date.get(..10).unwrap_or(&r.date),
                    Profile::parse(&r.profile)
                        .map(|p| p.title())
                        .unwrap_or(&r.profile)
                );
            }
            ask_profile(&st)?
        }
        None => bail!(
            "no --profile given, and no terminal to ask in: use --profile NAME (desktop-art, explorer, researcher, everything, blank-slate), with --yes to ask nothing"
        ),
    };
    let mut data_dir = a.data_dir.clone().map(|d| d.display().to_string());
    if interactive && data_dir.is_none() && profile.chooses_data_dir() {
        let free = st
            .free_bytes
            .map(|b| format!(", {} free", human_bytes(b)))
            .unwrap_or_default();
        eprintln!(
            "\nWhere should JETCARP keep its data? A researcher's library can grow to many gigabytes.\nNow: {}{free}",
            st.folders.data.display()
        );
        if let Some(d) =
            ask("A folder (empty, or one with a JETCARP library), or Enter to keep it: ")
            && !d.is_empty()
        {
            data_dir = Some(d);
        }
    }
    let mut packs: Vec<String> = a.packs.iter().map(|p| p.display().to_string()).collect();
    if interactive && packs.is_empty() && profile.chooses_packs() {
        eprintln!(
            "\nImport packs from your own folders too? (a pack store, an export, or a folder of them)"
        );
        while let Some(d) = ask("A folder, or Enter for none (or no more): ") {
            if d.is_empty() {
                break;
            }
            packs.push(d);
        }
    }
    let mut minutes = a.quickstart_minutes;
    if interactive && minutes.is_none() && profile.quickstart_minutes() > 0.0 {
        let m = profile.quickstart_minutes();
        let on = if st.gpu.usable {
            format!("on the GPU ({})", st.gpu.name)
        } else {
            "on the CPU (no GPU found)".into()
        };
        eprintln!("\nA Quick Start build searches {on} and adds its best runs to the Quick Start.");
        while let Some(t) = ask(&format!("Minutes to search (0 for none) [{m}]: ")) {
            if t.is_empty() {
                break;
            }
            match t.parse::<f64>() {
                Ok(n) if n >= 0.0 && n.is_finite() => {
                    minutes = Some(n);
                    break;
                }
                _ => eprintln!("  a number of minutes, such as 10"),
            }
        }
    }
    let mut measure = !a.no_measure;
    if interactive && measure {
        eprintln!(
            "\nMeasure this computer? About a minute of JETCARP's own work, with nothing drawn, for\n\
             its JETCARP Hardware Score: to compare it with other computers, and for Fit to this\n\
             computer's first guess. Its limits come from its hardware either way\n\
             (`jetcarp score` measures again)."
        );
        measure = yes_no("Measure it?", true);
    }
    // JETCARP among the system's apps, for every profile, when it isn't there yet (#105).
    let mut add_launcher = !a.no_launcher && !jetcarp_display::launcher::installed();
    if interactive && add_launcher {
        eprintln!("\n{}", Offer::Launcher.detail());
        add_launcher = yes_no(&format!("{}?", Offer::Launcher.title()), true);
    }
    let input = ApplyIn {
        measure: Some(measure),
        measure_quick: a.measure_quick,
        profile: profile.name().into(),
        data_dir,
        quickstart_minutes: minutes,
        packs,
        gpu: a.no_gpu.then_some(false),
        threads: a.threads,
        search: (a.build_tilings.is_some() || a.build_families.is_some()).then(|| {
            jetcarp_api::ops::search::ExploreIn {
                tilings: a.build_tilings.clone().map(serde_json::Value::String),
                families: a.build_families.clone().map(serde_json::Value::String),
                ..Default::default()
            }
        }),
        ..Default::default()
    };
    // What it will do, then (asked, in a terminal) do it.
    let plan = SetupApply::run(
        &cx,
        ApplyIn {
            dry_run: true,
            ..input.clone()
        },
        &Quiet,
    )?;
    if !json {
        eprintln!();
        eprint!("{}", plan.text());
        if add_launcher {
            eprintln!(
                "and {} (`{}`)",
                lower_first(Offer::Launcher.title()),
                Offer::Launcher.commands()
            );
        }
        eprint!("{}", plan.notes());
    }
    // Before measuring: what lowers the score (#301). The rest of JETCARP holds still while it
    // measures, and setup measures anyway, saying how the score was taken.
    if measure && !json {
        eprintln!("\n{}", jetcarp_api::ops::machine::HEAVY_WARNING);
    }
    if interactive && !yes_no("Go ahead?", true) {
        eprintln!("nothing changed");
        return Ok(());
    }
    let p = CliProgress::new(a.quiet || json);
    // A new context: the data folder may have changed, and the library goes there.
    let cx = crate::platform::ctx();
    let done = SetupApply::run(&cx, input, &p);
    // Each job's last line follows, so the progress line goes.
    p.clear();
    let done = done?;
    if json {
        show(&done, true)?;
    } else {
        for j in &done.jobs {
            println!("{} ({})", j.line, j.status);
        }
        eprint!("{}", done.notes());
        if let Some(r) = &done.record {
            println!("recorded {} in {r}", profile.title());
        }
    }
    if add_launcher {
        // Reported on standard error with --json, which keeps standard output for the result.
        match Offer::Launcher.accept() {
            Ok(m) if json => eprintln!("{m}"),
            Ok(m) => println!("{m}"),
            Err(e) => eprintln!(
                "could not add JETCARP to the system's apps: {e} (`{}` tries again)",
                Offer::Launcher.commands()
            ),
        }
    }
    if profile.desktop_offers() {
        desktop(interactive, json);
    }
    if !json {
        println!("Open the app with `jetcarp`; `jetcarp setup` again changes your mind.");
    }
    Ok(())
}

/// The desktop offers: asked in a terminal, else printed as commands.
fn desktop(interactive: bool, json: bool) {
    if json {
        return;
    }
    let available: Vec<Offer> = Offer::ALL
        .into_iter()
        .filter(|o| o.available().is_ok())
        .collect();
    if !interactive {
        eprintln!("\nTo use JETCARP on your desktop (nothing was changed there):");
        for o in &available {
            eprintln!("  {}: {}", o.title(), o.commands());
        }
        return;
    }
    eprintln!("\nJETCARP can also run on your desktop. Each of these is off unless you say yes:");
    let mut wallpaper = false;
    for o in available {
        if o == Offer::Login && !wallpaper {
            continue;
        }
        eprintln!("  {}", o.detail());
        if yes_no(&format!("{}?", o.title()), false) {
            match o.accept() {
                Ok(m) => {
                    eprintln!("  {m}");
                    wallpaper |= o == Offer::Wallpaper;
                }
                Err(e) => eprintln!("  could not: {e} (`{}` tries again)", o.commands()),
            }
        }
    }
}

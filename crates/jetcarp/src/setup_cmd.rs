//! `jetcarp setup`: the first-run setup from a terminal, through the operations the app's
//! setup wizard calls (`setup.status`, `setup.apply`). With no `--profile` it asks, in a
//! terminal; with `--profile NAME --yes` it asks nothing (scripts, tests). Either way it prints
//! what it will do, then does it, the jobs' progress on standard error. The desktop offers
//! (Desktop art, Everything) are questions in a terminal and printed commands otherwise: setup
//! never changes the desktop unasked.

use crate::search_cmd::CliProgress;
use crate::show;
use anyhow::{Result, bail};
use jetcarp_api::op::{Op, Quiet, Report};
use jetcarp_api::ops::setup::{self, ApplyIn, Profile, SetupApply, SetupStatus};
use jetcarp_api::ops::system::human_bytes;
use jetcarp_app::desktop_offers::Offer;
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
    /// Ask nothing: do what the profile does (the desktop offers are only printed).
    #[arg(long, short = 'y')]
    yes: bool,
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
    /// The Quick Start build searches only these tilings (tests; default: every one).
    #[arg(long, hide = true)]
    build_tilings: Option<String>,
    /// The Quick Start build searches only these rule families (tests; default: every one).
    #[arg(long, hide = true)]
    build_families: Option<String>,
}

/// A line from the terminal, trimmed (`None` at the end of input).
fn ask(prompt: &str) -> Option<String> {
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
        loop {
            let Some(t) = ask(&format!("Minutes to search (0 for none) [{m}]: ")) else {
                break;
            };
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
    let input = ApplyIn {
        profile: profile.name().into(),
        data_dir,
        quickstart_minutes: minutes,
        packs,
        gpu: !a.no_gpu,
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
        eprint!("{}", plan.notes());
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

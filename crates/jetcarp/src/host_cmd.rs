//! `jetcarp host`: the background host, a separate program that keeps scenes running with no app
//! window. Settings go to its file and a running host is told to read them again.

use anyhow::{Result, anyhow, bail};
use clap::{Args, Subcommand};
use jetcarp_display::autostart;
use jetcarp_display::caption::{CaptionSettings, Corner};
use jetcarp_display::gnome;
use jetcarp_display::host_settings::{
    HostSettings, LockScreen, MAX_AWAKE_MINUTES, Place, ScreenSettings,
};
use jetcarp_display::saver;
use jetcarp_mcp::link::{host_call, host_gone};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[derive(Args, Debug)]
pub struct HostArgs {
    #[command(subcommand)]
    pub cmd: HostCmd,
    /// Print the host's reply as JSON.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Switch {
    On,
    Off,
}

impl Switch {
    fn b(self) -> bool {
        self == Switch::On
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum CornerArg {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum LockArg {
    Off,
    Live,
    Screensaver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum GnomeAction {
    /// Copy the extension into place and switch it on (takes effect at the next login).
    Install,
    /// Switch the extension off and delete it.
    Remove,
    /// Say whether the extension is running.
    Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum SaverAction {
    /// Windows: copy JETCARP to a `.scr` file and open Screen Saver Settings with it selected.
    /// macOS: build `JETCARP.saver` with the rotation inside it and open System Settings.
    Install,
    /// Delete it (Windows: and stop Windows from running it; close Screen Saver Settings first).
    Remove,
    /// Say whether it is installed (Windows: and selected).
    Status,
}

#[derive(Subcommand, Debug)]
pub enum HostCmd {
    /// Run the host in this process (what `start`, login and the GNOME menu run).
    Serve(jetcarp_host::HostArgs),
    /// Run as the Windows screen saver (what `JETCARP.scr` runs, with Windows' arguments).
    #[command(hide = true)]
    Scr {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Start the host (it keeps running after this command ends).
    Start {
        /// Run this rotation, for this run only.
        #[arg(long)]
        rotation: Option<String>,
        /// No window or tray.
        #[arg(long)]
        headless: bool,
    },
    /// Quit the host.
    Stop,
    /// What the host is showing.
    Status,
    /// The next item, on every screen or one.
    Next {
        #[arg(long)]
        screen: Option<usize>,
    },
    /// The previous item, on every screen or one.
    Previous {
        #[arg(long)]
        screen: Option<usize>,
    },
    /// Freeze generations and the rotation.
    Pause,
    /// Carry on.
    Resume,
    /// Choose the rotation for a screen (saved).
    Rotation {
        name: String,
        #[arg(long)]
        screen: Option<usize>,
    },
    /// Show or hide what is running, and how (saved).
    Caption {
        state: Switch,
        #[arg(long, value_enum)]
        corner: Option<CornerArg>,
        /// Height of the first line, in points.
        #[arg(long)]
        size: Option<f32>,
        /// Include the generation count.
        #[arg(long, value_enum)]
        generation: Option<Switch>,
        /// Seconds to stay up after each new item (0: always).
        #[arg(long)]
        seconds: Option<f32>,
    },
    /// Frames per second, at most (saved).
    Fps { fps: f32 },
    /// Leave a still of the scene as the desktop picture when the host stops, or while the live
    /// wallpaper can't be shown (saved; off by default). Turning it or the wallpaper off puts your
    /// own picture back.
    Still { state: Switch },
    /// Pause while the computer runs on battery (saved; on by default).
    Battery { state: Switch },
    /// Pause while the whole processor stays at least this busy, in percent, as during research
    /// runs (saved; default 80, 0 never).
    Busy { percent: u8 },
    /// Start the host at login, or stop doing that.
    Login { state: Switch },
    /// Show screens as the desktop background (saved). On GNOME this needs the extension.
    Wallpaper {
        state: Switch,
        /// Only this screen. A screen one past the last is added.
        #[arg(long)]
        screen: Option<usize>,
        /// Put that screen on this monitor (see `jetcarp host status`).
        #[arg(long, requires = "screen")]
        monitor: Option<usize>,
        /// The rotation for that screen.
        #[arg(long, requires = "screen")]
        rotation: Option<String>,
    },
    /// What a wallpaper does on the GNOME lock screen (saved).
    Lock {
        #[arg(value_enum)]
        mode: LockArg,
        /// In screensaver mode, let the screen sleep after this many minutes (0: never).
        #[arg(long)]
        awake: Option<u32>,
    },
    /// The GNOME Shell extension behind wallpapers and the top-bar menu.
    Gnome {
        #[arg(value_enum)]
        action: GnomeAction,
    },
    /// The screen saver (Windows and macOS).
    Saver {
        #[arg(value_enum)]
        action: SaverAction,
        /// The rotation it runs (saved). Default: the first screen's.
        #[arg(long)]
        rotation: Option<String>,
    },
}

/// Saves `s` and tells a running host; no host running is fine.
fn save(s: &HostSettings) -> Result<Option<Value>> {
    s.save().map_err(|e| anyhow!(e))?;
    Ok(host_call(&json!({"action": "reload"})).ok())
}

fn show(v: &Value, json: bool) {
    if json {
        println!("{v}");
        return;
    }
    for line in status_lines(v) {
        println!("{line}");
    }
}

/// The host's status, for people: what each screen shows and where, the monitors, the wallpaper,
/// the caption, why it is paused, and the host's notes.
fn status_lines(v: &Value) -> Vec<String> {
    let mut out = vec![format!(
        "running, pid {}, up {}s, {} MB, GPU {}",
        v["pid"],
        v["uptime_s"],
        v["memory_mb"],
        v["gpu"].as_str().unwrap_or("none")
    )];
    let monitors: Vec<&str> = v["monitors"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|n| n.as_str().unwrap_or("?"))
        .collect();
    for (i, s) in v["screens"].as_array().into_iter().flatten().enumerate() {
        let st = &s["stage"];
        let numbers = if st["building"] == true && st["cells"] == 0 {
            "starting".to_string()
        } else {
            format!("generation {} · {} cells", st["generation"], st["cells"])
        };
        let shown = if s["place"] == "wallpaper" {
            "wallpaper"
        } else if s["hidden"] == true {
            "window, not shown (started at login)"
        } else {
            "window"
        };
        let place = match s["monitor"].as_u64().map(|m| m as usize) {
            Some(m) => match monitors.get(m) {
                Some(name) => format!("{shown} on monitor {m} ({name})"),
                // A host just started may not have read its monitors yet.
                None if monitors.is_empty() => format!("{shown} on monitor {m}"),
                None => format!("{shown} on monitor {m} (not connected)"),
            },
            None if shown == "wallpaper" && monitors.len() > 1 => {
                "wallpaper on every monitor".into()
            }
            None => shown.into(),
        };
        out.push(format!(
            "screen {i}: {} · {} ({} of {}) · {numbers} · {place}{}",
            st["rotation"].as_str().unwrap_or("?"),
            st["label"].as_str().unwrap_or("?"),
            st["number"],
            st["count"],
            if st["paused"] == true {
                " · paused"
            } else {
                ""
            }
        ));
    }
    if !monitors.is_empty() {
        let names: Vec<String> = monitors
            .iter()
            .enumerate()
            .map(|(i, n)| format!("{i}: {n}"))
            .collect();
        out.push(format!("monitors: {}", names.join(", ")));
    }
    if v["wallpaper"] == true {
        let shown = if v["extension"] == true {
            "showing"
        } else {
            "waiting for the GNOME extension"
        };
        if cfg!(target_os = "linux") {
            let lock = v["lock_screen"].as_str().unwrap_or("?");
            out.push(format!("wallpaper: {shown} · lock screen: {lock}"));
        } else {
            out.push(format!("wallpaper: {shown}"));
        }
    }
    if let Ok(c) = serde_json::from_value::<CaptionSettings>(v["caption"].clone()) {
        let mut line = if c.show {
            format!(
                "caption: on, {}, {} pt",
                c.corner.label().to_lowercase(),
                c.size
            )
        } else {
            "caption: off".to_string()
        };
        if c.show && c.generation {
            line.push_str(", with the generation");
        }
        if c.show && c.seconds > 0.0 {
            line.push_str(&format!(", for {} s after each new item", c.seconds));
        }
        out.push(line);
    }
    let why: Vec<&str> = v["paused_why"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !why.is_empty() {
        out.push(format!("paused: {}", why.join("; ")));
    }
    for line in v["log"].as_array().into_iter().flatten() {
        out.push(format!("note: {}", line.as_str().unwrap_or("")));
    }
    out
}

fn start(rotation: Option<&str>, headless: bool, json: bool) -> Result<()> {
    if host_call(&serde_json::json!({"action": "status"})).is_ok() {
        bail!("the host is already running");
    }
    let mut cmd = autostart::host_command().map_err(|e| anyhow!(e))?;
    if let Some(r) = rotation {
        cmd.args.extend(["--rotation".to_string(), r.to_string()]);
    }
    if headless {
        cmd.args.push("--headless".to_string());
    }
    cmd.spawn_detached().map_err(|e| anyhow!(e))?;
    let t0 = Instant::now();
    loop {
        std::thread::sleep(Duration::from_millis(100));
        match host_call(&serde_json::json!({"action": "status"})) {
            Ok(v) => {
                show(&v, json);
                return Ok(());
            }
            Err(_) if t0.elapsed() < Duration::from_secs(10) => {}
            Err(e) => bail!("the host did not start: {e}"),
        }
    }
}

fn act(args: Value, json: bool) -> Result<()> {
    let v = host_call(&args).map_err(|e| anyhow!(e))?;
    show(&v, json);
    Ok(())
}

/// With the host not running, puts the user's desktop picture back once no still is wanted (the
/// host does this itself when it runs).
fn settle_picture(s: &HostSettings) {
    use jetcarp_display::desktop_picture::{self, Restored};
    match desktop_picture::settle(s) {
        Ok(Restored::PutBack) => println!("put the desktop's own picture back"),
        Ok(_) => {}
        Err(e) => eprintln!("the desktop's own picture could not be put back: {e}"),
    }
}

/// Whether the host has gone within a few seconds (its port closed: a status call could wait for
/// an answer from a host that is closing).
fn gone_soon() -> bool {
    for _ in 0..50 {
        if host_gone() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    false
}

pub fn run(a: HostArgs) -> Result<()> {
    let j = a.json;
    match a.cmd {
        HostCmd::Serve(args) => jetcarp_host::run(args),
        HostCmd::Scr { args } => {
            #[cfg(windows)]
            {
                jetcarp_host::run_scr(&args);
                Ok(())
            }
            #[cfg(not(windows))]
            {
                bail!(
                    "`.scr` screen savers exist only on Windows ({} arguments)",
                    args.len()
                )
            }
        }
        HostCmd::Start { rotation, headless } => start(rotation.as_deref(), headless, j),
        HostCmd::Stop => {
            if let Err(e) = host_call(&json!({"action": "quit"})) {
                // The answer to "quit" can be lost as the host closes its end (seen on Windows as
                // os error 10054) though the host has quit, so fail only if it is still there.
                if e.contains("not running") || !gone_soon() {
                    return Err(anyhow!(e));
                }
            } else if !gone_soon() {
                // It answers before it goes: a command right after this one would still reach
                // it (and `host start` would find it running).
                bail!("the host was asked to quit, but is still running");
            }
            println!("stopped");
            Ok(())
        }
        HostCmd::Status => act(json!({"action": "status"}), j),
        HostCmd::Next { screen } => act(json!({"action": "next", "screen": screen}), j),
        HostCmd::Previous { screen } => act(json!({"action": "previous", "screen": screen}), j),
        HostCmd::Pause => act(json!({"action": "pause"}), j),
        HostCmd::Resume => act(json!({"action": "resume"}), j),
        HostCmd::Rotation { name, screen } => {
            if host_call(&json!({"action": "status"})).is_ok() {
                return act(
                    json!({"action": "rotation", "name": name, "screen": screen}),
                    j,
                );
            }
            let mut s = HostSettings::load().0;
            let i = screen.unwrap_or(0);
            let slot = s
                .screens
                .get_mut(i)
                .ok_or_else(|| anyhow!("there is no screen {i}"))?;
            slot.rotation = name;
            save(&s)?;
            println!("saved; the host is not running");
            Ok(())
        }
        HostCmd::Caption {
            state,
            corner,
            size,
            generation,
            seconds,
        } => {
            let mut s = HostSettings::load().0;
            let c = &mut s.caption;
            c.show = state.b();
            if let Some(k) = corner {
                c.corner = match k {
                    CornerArg::BottomLeft => Corner::BottomLeft,
                    CornerArg::BottomRight => Corner::BottomRight,
                    CornerArg::TopLeft => Corner::TopLeft,
                    CornerArg::TopRight => Corner::TopRight,
                };
            }
            if let Some(x) = size {
                if !(8.0..=96.0).contains(&x) {
                    bail!("caption size is 8 to 96 points");
                }
                c.size = x;
            }
            if let Some(g) = generation {
                c.generation = g.b();
            }
            if let Some(x) = seconds {
                if !(0.0..=3600.0).contains(&x) {
                    bail!("caption seconds are 0 to 3600");
                }
                c.seconds = x;
            }
            match save(&s)? {
                Some(v) => show(&v, j),
                None => println!("saved; the host is not running"),
            }
            Ok(())
        }
        HostCmd::Fps { fps } => {
            if !(1.0..=240.0).contains(&fps) {
                bail!("frames per second are 1 to 240");
            }
            let mut s = HostSettings::load().0;
            s.fps = fps;
            match save(&s)? {
                Some(v) => show(&v, j),
                None => println!("saved; the host is not running"),
            }
            Ok(())
        }
        HostCmd::Battery { state } => {
            let mut s = HostSettings::load().0;
            s.pause_on_battery = state.b();
            match save(&s)? {
                Some(v) => show(&v, j),
                None => println!("saved; the host is not running"),
            }
            Ok(())
        }
        HostCmd::Busy { percent } => {
            if percent > 100 {
                bail!("the busy limit is 0 (never) to 100 percent");
            }
            let mut s = HostSettings::load().0;
            s.busy_percent = percent;
            match save(&s)? {
                Some(v) => show(&v, j),
                None => println!("saved; the host is not running"),
            }
            Ok(())
        }
        HostCmd::Login { state } => {
            let cmd = autostart::login_command().map_err(|e| anyhow!(e))?;
            autostart::set(state.b(), &cmd).map_err(|e| anyhow!(e))?;
            println!(
                "{}",
                if state.b() {
                    format!("{} starts at login", cmd.command_line())
                } else {
                    "the host no longer starts at login".to_string()
                }
            );
            Ok(())
        }
        HostCmd::Wallpaper {
            state,
            screen,
            monitor,
            rotation,
        } => {
            let mut s = HostSettings::load().0;
            let place = if state.b() {
                Place::Wallpaper
            } else {
                Place::Window
            };
            match screen {
                None => s.screens.iter_mut().for_each(|x| x.place = place),
                Some(i) => {
                    if i == s.screens.len() {
                        s.screens.push(ScreenSettings::default());
                    }
                    let slot = s
                        .screens
                        .get_mut(i)
                        .ok_or_else(|| anyhow!("there is no screen {i}"))?;
                    slot.place = place;
                    if monitor.is_some() {
                        slot.monitor = monitor;
                    }
                    if let Some(r) = rotation {
                        slot.rotation = r;
                    }
                }
            }
            if state.b() && cfg!(target_os = "linux") {
                let st = gnome::state();
                if st != gnome::ExtensionState::Active {
                    eprintln!("{}", st.describe());
                }
            }
            match save(&s)? {
                Some(v) => show(&v, j),
                None => {
                    println!("saved; the host is not running");
                    settle_picture(&s);
                }
            }
            Ok(())
        }
        HostCmd::Still { state } => {
            let mut s = HostSettings::load().0;
            s.still_picture = state.b();
            match save(&s)? {
                Some(v) => show(&v, j),
                None => {
                    println!("saved; the host is not running");
                    settle_picture(&s);
                }
            }
            Ok(())
        }
        HostCmd::Lock { mode, awake } => {
            let mut s = HostSettings::load().0;
            s.lock_screen = match mode {
                LockArg::Off => LockScreen::Off,
                LockArg::Live => LockScreen::Live,
                LockArg::Screensaver => LockScreen::Screensaver,
            };
            if let Some(m) = awake {
                if m > MAX_AWAKE_MINUTES {
                    bail!("the most is {MAX_AWAKE_MINUTES} minutes");
                }
                s.lock_awake_minutes = m;
            }
            match save(&s)? {
                Some(v) => show(&v, j),
                None => println!("saved; the host is not running"),
            }
            Ok(())
        }
        HostCmd::Gnome { action } => {
            match action {
                GnomeAction::Install => {
                    gnome::install().map_err(|e| anyhow!(e))?;
                    if let Ok(cmd) = autostart::host_command() {
                        autostart::record_host_program(&cmd).map_err(|e| anyhow!(e))?;
                    }
                }
                GnomeAction::Remove => gnome::remove().map_err(|e| anyhow!(e))?,
                GnomeAction::Status => {}
            }
            println!("{}", gnome::state().describe());
            Ok(())
        }
        HostCmd::Saver { action, rotation } => {
            if let Some(name) = rotation {
                let mut s = HostSettings::load().0;
                s.saver_rotation = Some(name);
                s.save().map_err(|e| anyhow!(e))?;
            }
            if !saver::AVAILABLE {
                bail!(
                    "this system has no screen saver list; on GNOME, use `jetcarp host lock screensaver`"
                );
            }
            match action {
                SaverAction::Install => {
                    let (path, warnings) = saver::install().map_err(|e| anyhow!(e))?;
                    for w in warnings {
                        eprintln!("warning: {w}");
                    }
                    if saver::BAKED {
                        println!(
                            "written to {}; choose JETCARP in {} (opening). Install again after changing its rotation",
                            path.display(),
                            jetcarp_display::macsaver::settings_place()
                        );
                    } else {
                        println!(
                            "copied to {}; Screen Saver Settings is opening",
                            path.display()
                        );
                    }
                }
                SaverAction::Remove => {
                    let note = saver::remove().map_err(|e| anyhow!(e))?;
                    println!("removed");
                    if let Some(n) = note {
                        println!("{n}");
                    }
                }
                SaverAction::Status => println!("{}", saver::describe()),
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::status_lines;
    use serde_json::json;

    #[test]
    fn status_says_where_each_screen_is_the_caption_and_why_it_is_paused() {
        let stage = |label: &str, building: bool, cells: u64| {
            json!({"rotation": "A classic a day", "label": label, "number": 2, "count": 7,
                "generation": 0, "cells": cells, "paused": true, "building": building})
        };
        let v = json!({
            "pid": 7, "uptime_s": 9.0, "memory_mb": 300, "gpu": "RTX",
            "screens": [
                {"monitor": 0, "place": "wallpaper", "stage": stage("Life", false, 1000)},
                {"monitor": 1, "place": "wallpaper", "stage": stage("langtons-ant", true, 0)},
                {"monitor": null, "place": "window", "stage": stage("Life", false, 10)},
            ],
            "monitors": ["DISPLAY1", "DISPLAY5"],
            "wallpaper": true, "extension": true, "lock_screen": "off",
            "caption": {"show": true, "corner": "BottomLeft", "size": 20.0,
                "generation": false, "seconds": 0.0},
            "paused_why": ["on battery"],
            "log": [],
        });
        let lines = status_lines(&v);
        assert!(
            lines[1].ends_with(
                "generation 0 · 1000 cells · wallpaper on monitor 0 (DISPLAY1) · paused"
            ),
            "{lines:?}"
        );
        assert!(
            lines[2].contains("· starting · wallpaper on monitor 1 (DISPLAY5)"),
            "{lines:?}"
        );
        assert!(lines[3].ends_with("· window · paused"), "{lines:?}");
        let has = |l: &str| lines.iter().any(|x| x == l);
        assert!(has("caption: on, bottom left, 20 pt"), "{lines:?}");
        assert!(has("paused: on battery"), "{lines:?}");
        let mut off = v.clone();
        off["caption"]["show"] = json!(false);
        off["paused_why"] = json!([]);
        let lines = status_lines(&off);
        assert!(lines.iter().any(|l| l == "caption: off"));
        assert!(!lines.iter().any(|l| l.starts_with("paused:")));
    }
}

//! `jetcarp app`: control the running JETCARP app through its local endpoint (the app's AI control,
//! `jetcarp --control`): its displays and screensaver (`app_display`), running, and any other
//! tool (`jetcarp app call <tool> <json>`).

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use serde_json::{Value, json};

#[derive(Args, Debug)]
pub struct AppArgs {
    #[command(subcommand)]
    pub cmd: AppCmd,
    /// App port (default: the one the app recorded when AI control was switched on).
    #[arg(long, global = true)]
    pub port: Option<u16>,
    /// App token (default: the recorded one).
    #[arg(long, global = true)]
    pub token: Option<String>,
    /// Print the app's reply as JSON.
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand, Debug)]
pub enum AppCmd {
    /// Where the grid shows, the monitors, the screensaver, keep-awake.
    Status,
    /// Screensaver mode on or off (with --profile: show that profile).
    Screensaver {
        state: OnOff,
        /// Show this screensaver profile (`jetcarp app profiles` lists them).
        #[arg(long)]
        profile: Option<String>,
    },
    /// The screensaver profiles, with the one showing marked.
    Profiles,
    /// Displays: mode, full screen, outputs, span or mirror, cell size, chrome, keep-awake.
    Display(DisplayArgs),
    /// Run generations.
    Play,
    /// Pause.
    Pause,
    /// Generations per second (0: as fast as possible).
    Speed { gens_per_sec: f64 },
    /// The next item of the screensaver's rotation.
    Next,
    /// The previous item of the screensaver's rotation.
    Previous,
    /// Call any MCP tool on the app's world: `jetcarp app call world_status` or
    /// `jetcarp app call world_step '{"generations": 10}'`.
    Call {
        tool: String,
        /// Arguments as a JSON object (default `{}`).
        args: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum OnOff {
    On,
    Off,
}

impl OnOff {
    fn b(self) -> bool {
        self == OnOff::On
    }
}

#[derive(Args, Debug, Default)]
pub struct DisplayArgs {
    /// window, fullscreen, or outputs.
    #[arg(long)]
    pub mode: Option<String>,
    /// The main window full screen (on) or windowed (off).
    #[arg(long)]
    pub fullscreen: Option<OnOff>,
    /// Monitor for full screen (index from `jetcarp app status`).
    #[arg(long)]
    pub fullscreen_monitor: Option<usize>,
    /// Output monitors by index, comma separated (sets mode outputs).
    #[arg(long, value_delimiter = ',')]
    pub monitors: Vec<usize>,
    /// Outputs as plain windows: WxH@X,Y separated by ';' (trying a span on one monitor).
    #[arg(long)]
    pub windows: Option<String>,
    /// One canvas across the outputs.
    #[arg(long, conflicts_with = "mirror")]
    pub span: bool,
    /// The same view on each output.
    #[arg(long)]
    pub mirror: bool,
    /// Canvas units: pixels or logical.
    #[arg(long)]
    pub units: Option<String>,
    /// Draw no grid in the main window while outputs are on.
    #[arg(long)]
    pub hide_preview: Option<OnOff>,
    /// Cell size in physical pixels (1 to 64).
    #[arg(long)]
    pub cell_px: Option<f64>,
    /// Rebuild the tiling to fill the canvas at the cell size.
    #[arg(long)]
    pub refill: bool,
    /// Zoom so a cell edge is the cell size in pixels.
    #[arg(long)]
    pub pixel_zoom: bool,
    #[arg(long)]
    pub keep_awake: Option<OnOff>,
    #[arg(long)]
    pub menu_autohide: Option<OnOff>,
    /// Hide (on) or show (off) every pane.
    #[arg(long)]
    pub panes_hidden: Option<OnOff>,
    #[arg(long)]
    pub status_bar: Option<OnOff>,
    /// GPU simulation.
    #[arg(long)]
    pub gpu: Option<OnOff>,
    /// Show each monitor's index on it for three seconds.
    #[arg(long)]
    pub identify: bool,
    /// Screensaver settings as JSON (see the app_display tool's `saver`), e.g.
    /// '{"source": "rotation", "rotation": "Wolfram series"}'.
    #[arg(long)]
    pub saver: Option<String>,
}

/// The tool call a command makes: (tool, arguments).
pub fn request(cmd: &AppCmd) -> Result<(String, Value)> {
    let d = |v: Value| Ok(("app_display".to_string(), v));
    match cmd {
        AppCmd::Status => d(json!({})),
        AppCmd::Screensaver { state, profile } => {
            let mut v = json!({"screensaver": state.b()});
            if let Some(p) = profile {
                v["profile"] = json!(p);
            }
            d(v)
        }
        AppCmd::Profiles => d(json!({})),
        AppCmd::Play => d(json!({"playing": true})),
        AppCmd::Pause => d(json!({"playing": false})),
        AppCmd::Speed { gens_per_sec } => d(json!({"speed": gens_per_sec})),
        AppCmd::Next => d(json!({"skip": "next"})),
        AppCmd::Previous => d(json!({"skip": "previous"})),
        AppCmd::Display(a) => d(display_args(a)?),
        AppCmd::Call { tool, args } => {
            let v: Value = match args {
                None => json!({}),
                Some(s) => serde_json::from_str(s)
                    .map_err(|e| anyhow::anyhow!("arguments must be a JSON object: {e}"))?,
            };
            if !v.is_object() {
                bail!("arguments must be a JSON object");
            }
            Ok((tool.clone(), v))
        }
    }
}

fn display_args(a: &DisplayArgs) -> Result<Value> {
    let mut v = serde_json::Map::new();
    let mut put = |k: &str, x: Value| {
        v.insert(k.to_string(), x);
    };
    if let Some(m) = &a.mode {
        put("mode", json!(m));
    }
    if let Some(f) = a.fullscreen {
        put("fullscreen", json!(f.b()));
    }
    if let Some(m) = a.fullscreen_monitor {
        put("fullscreen_monitor", json!(m));
    }
    if !a.monitors.is_empty() {
        put("monitors", json!(a.monitors));
    }
    if let Some(w) = &a.windows {
        let list: Vec<&str> = w
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        put("windows", json!(list));
    }
    if a.span {
        put("arrangement", json!("span"));
    }
    if a.mirror {
        put("arrangement", json!("mirror"));
    }
    if let Some(u) = &a.units {
        put("units", json!(u));
    }
    for (k, x) in [
        ("hide_preview", a.hide_preview),
        ("keep_awake", a.keep_awake),
        ("menu_autohide", a.menu_autohide),
        ("panes_hidden", a.panes_hidden),
        ("status_bar", a.status_bar),
        ("gpu", a.gpu),
    ] {
        if let Some(x) = x {
            put(k, json!(x.b()));
        }
    }
    if let Some(c) = a.cell_px {
        put("cell_px", json!(c));
    }
    if a.refill {
        put("refill", json!(true));
    }
    if a.pixel_zoom {
        put("pixel_zoom", json!(true));
    }
    if a.identify {
        put("identify", json!(true));
    }
    if let Some(s) = &a.saver {
        let x: Value = serde_json::from_str(s)
            .map_err(|e| anyhow::anyhow!("--saver must be a JSON object: {e}"))?;
        put("saver", x);
    }
    Ok(Value::Object(v))
}

/// Call `tool` on the app; returns the tool's result object.
pub fn call_app(
    info: Option<jetcarp_mcp::link::EndpointInfo>,
    tool: &str,
    args: &Value,
) -> Result<Value> {
    let mut c = jetcarp_mcp::link::AppClient::connect(info).map_err(anyhow::Error::msg)?;
    let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": tool, "arguments": args}});
    let r = c.call(&msg).map_err(anyhow::Error::msg)?;
    if let Some(e) = r.get("error") {
        bail!("{}", e["message"].as_str().unwrap_or("error from the app"));
    }
    Ok(r["result"].clone())
}

/// The text of a tool result.
pub fn result_text(r: &Value) -> String {
    r["content"]
        .as_array()
        .map(|c| {
            c.iter()
                .filter_map(|x| x["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// A short report of `app_display`'s state.
pub fn summary(s: &Value) -> String {
    let mut out = vec![];
    let onoff = |b: &Value| {
        if b.as_bool() == Some(true) {
            "on"
        } else {
            "off"
        }
    };
    let mut where_ = s["mode"].as_str().unwrap_or("?").to_string();
    if s["mode"] == "outputs" {
        let outs: Vec<String> = s["outputs"]
            .as_array()
            .map(|o| {
                o.iter()
                    .map(|o| match o["window"].as_array() {
                        Some(w) => format!("window {}x{}", w[0], w[1]),
                        None => format!("monitor {}", o["monitor"]),
                    })
                    .collect()
            })
            .unwrap_or_default();
        where_ = format!(
            "outputs ({}, {})",
            s["arrangement"].as_str().unwrap_or("?"),
            outs.join(", ")
        );
    }
    let num = |v: &Value| v.as_f64().unwrap_or(0.0);
    out.push(format!(
        "display: {where_}; canvas {:.0} x {:.0} px; cells {} px",
        num(&s["canvas_px"][0]),
        num(&s["canvas_px"][1]),
        num(&s["cell_px"])
    ));
    out.push(format!(
        "running: {}; speed {} gen/s{}; GPU simulation {}",
        if s["playing"] == true {
            "yes"
        } else {
            "paused"
        },
        s["speed"],
        if s["max_speed"] == true { " (max)" } else { "" },
        onoff(&s["gpu"])
    ));
    let ss = &s["screensaver"];
    let mut saver = format!(
        "screensaver: {}; source {}",
        onoff(&ss["active"]),
        ss["settings"]["source"].as_str().unwrap_or("?")
    );
    let profile = ss["profile"]
        .as_str()
        .or(ss["settings"]["profile"].as_str().filter(|_| {
            ss["settings"]["source"]
                .as_str()
                .is_some_and(|s| s.eq_ignore_ascii_case("profile"))
        }));
    if let Some(p) = profile {
        saver.push_str(&format!(" ({p})"));
    }
    if let Some(items) = ss["items"].as_array()
        && !items.is_empty()
    {
        saver.push_str(&format!(
            "; item {} of {} ({}), next in {:.0} s",
            ss["index"].as_u64().unwrap_or(0) + 1,
            items.len(),
            items[ss["index"].as_u64().unwrap_or(0) as usize]
                .as_str()
                .unwrap_or("?"),
            ss["seconds_left"].as_f64().unwrap_or(0.0)
        ));
    }
    out.push(saver);
    let k = &s["keep_awake"];
    out.push(format!(
        "keep-awake: {}{}",
        if k["held"] == true {
            "held by "
        } else {
            "not held"
        },
        k["backend"].as_str().unwrap_or("")
    ));
    if let Some(m) = s["monitors"].as_array() {
        for m in m {
            out.push(format!(
                "monitor {}: {} {} x {} at ({}, {}) scale {}",
                m["index"],
                m["name"].as_str().unwrap_or("unnamed"),
                m["size_px"][0],
                m["size_px"][1],
                m["position_px"][0],
                m["position_px"][1],
                m["scale"]
            ));
        }
    }
    out.push(format!(
        "chrome: menu auto-hide {}, panes {}, status bar {}",
        onoff(&s["menu_autohide"]),
        if s["panes_hidden"] == true {
            "hidden"
        } else {
            "shown"
        },
        onoff(&s["status_bar"])
    ));
    out.join("\n")
}

/// The screensaver profiles from `app_display`'s state, one per line; `*` marks the one showing
/// (or chosen, when the screensaver is off).
pub fn profiles_list(s: &Value) -> String {
    let ss = &s["screensaver"];
    let current = ss["profile"]
        .as_str()
        .or(ss["settings"]["profile"].as_str());
    let mut out = vec![];
    for p in s["profiles"].as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or("?");
        let mark = if Some(name) == current { "*" } else { " " };
        let reseed = match &p["reseed"] {
            Value::String(t) => t.clone(),
            Value::Number(n) if n.as_u64() == Some(0) => "no reseeding".into(),
            Value::Number(n) => format!("{n} policies"),
            _ => "?".into(),
        };
        out.push(format!(
            "{mark} {name:24} {:18} {}; {} px; reseed: {reseed}",
            p["kind"].as_str().unwrap_or(""),
            p["shows"].as_str().unwrap_or(""),
            p["cell_px"]
        ));
    }
    if let Some(d) = s["profiles_dir"].as_str() {
        out.push(format!("files: {d}"));
    }
    out.join("\n")
}

pub fn run(a: AppArgs) -> Result<()> {
    let (tool, args) = request(&a.cmd)?;
    let info = match (a.port, a.token) {
        (None, None) => None,
        (port, token) => {
            let file = jetcarp_mcp::link::EndpointInfo::read().ok();
            Some(jetcarp_mcp::link::EndpointInfo {
                port: port
                    .or(file.as_ref().map(|f| f.port))
                    .unwrap_or(jetcarp_mcp::link::DEFAULT_PORT),
                token: token
                    .or(file.map(|f| f.token))
                    .ok_or_else(|| anyhow::anyhow!("give --token too"))?,
            })
        }
    };
    let r = call_app(info, &tool, &args)?;
    let text = result_text(&r);
    if r["isError"] == true {
        bail!("{text}");
    }
    if tool == "app_display" && !a.json {
        match serde_json::from_str::<Value>(&text) {
            Ok(v) if matches!(a.cmd, AppCmd::Profiles) => println!("{}", profiles_list(&v)),
            Ok(v) => println!("{}", summary(&v)),
            Err(_) => println!("{text}"),
        }
    } else {
        println!("{text}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct T {
        #[command(subcommand)]
        cmd: Top,
    }
    #[derive(Subcommand)]
    enum Top {
        App(AppArgs),
    }

    fn parse(args: &[&str]) -> (String, Value) {
        let mut v = vec!["jetcarp", "app"];
        v.extend_from_slice(args);
        let Top::App(a) = T::try_parse_from(v).expect("parses").cmd;
        request(&a.cmd).unwrap()
    }

    #[test]
    fn commands_make_app_display_calls() {
        assert_eq!(parse(&["status"]), ("app_display".into(), json!({})));
        assert_eq!(
            parse(&["screensaver", "on"]).1,
            json!({"screensaver": true})
        );
        assert_eq!(
            parse(&["screensaver", "on", "--profile", "Day & Night"]).1,
            json!({"screensaver": true, "profile": "Day & Night"})
        );
        assert_eq!(parse(&["profiles"]), ("app_display".into(), json!({})));
        assert_eq!(parse(&["pause"]).1, json!({"playing": false}));
        assert_eq!(parse(&["next"]).1, json!({"skip": "next"}));
        assert_eq!(parse(&["speed", "60"]).1, json!({"speed": 60.0}));
        assert_eq!(
            parse(&[
                "display",
                "--monitors",
                "0,1",
                "--span",
                "--cell-px",
                "1",
                "--refill",
                "--keep-awake",
                "on",
                "--panes-hidden",
                "on"
            ])
            .1,
            json!({"monitors": [0, 1], "arrangement": "span", "cell_px": 1.0, "refill": true,
                   "keep_awake": true, "panes_hidden": true})
        );
        assert_eq!(
            parse(&[
                "display",
                "--windows",
                "800x600@0,0; 800x600@800,0",
                "--mirror"
            ])
            .1,
            json!({"windows": ["800x600@0,0", "800x600@800,0"], "arrangement": "mirror"})
        );
        assert_eq!(
            parse(&["call", "world_step", "{\"generations\": 3}"]),
            ("world_step".into(), json!({"generations": 3}))
        );
        let mut v = vec!["jetcarp", "app", "display", "--span", "--mirror"];
        assert!(
            T::try_parse_from(v.drain(..)).is_err(),
            "span and mirror conflict"
        );
    }

    #[test]
    fn summary_reads_the_state() {
        let s = json!({"mode": "outputs", "outputs": [{"monitor": 0}, {"window": [800, 600]}],
            "arrangement": "span", "canvas_px": [1600, 600], "cell_px": 1.0, "playing": true,
            "speed": 30.0, "gpu": true, "screensaver": {"active": true, "settings": {"source": "rotation"},
            "items": ["Quick Start life", "scene a.json"], "index": 1, "seconds_left": 42.0},
            "keep_awake": {"held": true, "backend": "portal"}, "monitors": [], "menu_autohide": true,
            "panes_hidden": true, "status_bar": false});
        let t = summary(&s);
        assert!(
            t.contains("outputs (span, monitor 0, window 800x600)"),
            "{t}"
        );
        assert!(
            t.contains("item 2 of 2 (scene a.json), next in 42 s"),
            "{t}"
        );
        assert!(t.contains("held by portal"), "{t}");
        let s = json!({"screensaver": {"active": true, "profile": "Hodgepodge",
            "settings": {"source": "Profile", "profile": "Life"}},
            "profiles": [
                {"name": "Life", "kind": "built-in", "shows": "B3/S23 on Square", "cell_px": 3.0, "reseed": 2},
                {"name": "Hodgepodge", "kind": "built-in, edited", "shows": "Hodgepodge on Square", "cell_px": 3.0, "reseed": 0},
                {"name": "Mine", "kind": "yours", "shows": "scene file a.json", "cell_px": 2.0, "reseed": "scene's own"}],
            "profiles_dir": "/c/jetcarp/screensavers"});
        let t = profiles_list(&s);
        assert!(t.contains("* Hodgepodge"), "{t}");
        assert!(t.contains("  Life"), "{t}");
        assert!(t.contains("reseed: no reseeding"), "{t}");
        assert!(t.contains("reseed: scene's own"), "{t}");
        assert!(t.ends_with("files: /c/jetcarp/screensavers"), "{t}");
        assert!(summary(&s).contains("screensaver: on; source Profile (Hodgepodge)"));
    }

    /// The command round trip over a real link, answered by a host with a display.
    #[test]
    fn round_trip_through_the_link() {
        use jetcarp_api::world::{Host, Session};
        use jetcarp_mcp::{Server, link};
        use std::sync::Arc;
        struct Fake {
            s: Session,
            saver: bool,
        }
        impl Host for Fake {
            fn world(&self) -> &jetcarp_core::World {
                &self.s.world
            }
            fn exec(
                &mut self,
                c: jetcarp_core::Command,
            ) -> Result<jetcarp_core::world::Applied, String> {
                self.s.exec(c)
            }
            fn replace(&mut self, s: jetcarp_core::Scene) -> Result<(), String> {
                self.s.replace(s)
            }
            fn has_display(&self) -> bool {
                true
            }
            fn display(&mut self, args: &Value) -> Result<Value, String> {
                if let Some(b) = args["screensaver"].as_bool() {
                    self.saver = b;
                }
                Ok(json!({"mode": "window", "screensaver": {"active": self.saver}}))
            }
        }
        let listener = link::Listener::start(0, Arc::new(|| {})).expect("listen");
        let info = link::EndpointInfo {
            port: listener.port,
            token: listener.token.clone(),
        };
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop2 = stop.clone();
        let ui = std::thread::spawn(move || {
            let mut server = Server::new();
            let mut host = Fake {
                s: Session::new(),
                saver: false,
            };
            while !stop2.load(std::sync::atomic::Ordering::SeqCst) {
                for p in listener.poll() {
                    let msg = p.message.clone();
                    p.reply(server.handle(&mut host, &msg));
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });
        let (tool, args) = parse(&["screensaver", "on"]);
        let r = call_app(Some(info.clone()), &tool, &args).unwrap();
        let v: Value = serde_json::from_str(&result_text(&r)).unwrap();
        assert_eq!(v["screensaver"]["active"], true);
        let r = call_app(Some(info), "world_status", &json!({})).unwrap();
        assert_eq!(r["isError"], false);
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        ui.join().unwrap();
    }
}

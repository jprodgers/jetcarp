//! The `app_display` tool (`jetcarp_mcp::app_tools`): the display, screensaver, and keep-awake
//! state as JSON, and changes to them from the CLI (`jetcarp app ...`) and MCP.

use super::display::{OutputMode, OutputSpec, SaverSource};
use super::*;
use crate::span::{Arrangement, Units};
use jetcarp_display::rotation::Rotation;
use serde_json::{Value, json};

const KEYS: &[&str] = &[
    "screensaver",
    "playing",
    "speed",
    "gpu",
    "mode",
    "fullscreen",
    "fullscreen_monitor",
    "monitors",
    "windows",
    "arrangement",
    "units",
    "hide_preview",
    "menu_autohide",
    "panes_hidden",
    "status_bar",
    "cell_px",
    "refill",
    "pixel_zoom",
    "keep_awake",
    "skip",
    "identify",
    "saver",
    "profile",
    "rotation",
    "save_rotation",
    "delete_rotation",
];

const SAVER_KEYS: &[&str] = &[
    "source",
    "profile",
    "scene",
    "quickstart",
    "rotation",
    "cell_px",
    "refill",
    "speed",
    "gpu",
    "keep_awake",
    "exit_any_input",
    "control",
    "hide_windows",
    "cursor_hide_s",
    "caption",
];

fn check(args: &Value, allowed: &[&str], what: &str) -> Result<(), String> {
    let Some(o) = args.as_object() else {
        return Err(format!("{what} must be an object"));
    };
    for k in o.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(format!(
                "unknown {what} argument `{k}`. Allowed: {}",
                allowed.join(", ")
            ));
        }
    }
    Ok(())
}

fn get_bool(v: &Value, k: &str) -> Result<Option<bool>, String> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(x) => Err(format!("`{k}` must be true or false, got {x}")),
    }
}

fn get_f64(v: &Value, k: &str) -> Result<Option<f64>, String> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(x) => x
            .as_f64()
            .map(Some)
            .ok_or_else(|| format!("`{k}` must be a number, got {x}")),
    }
}

fn get_str<'a>(v: &'a Value, k: &str) -> Result<Option<&'a str>, String> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(x) => Err(format!("`{k}` must be a string, got {x}")),
    }
}

/// `WxH@X,Y` (or `WxH`, placed right of the previous one).
pub(super) fn parse_window(s: &str, next_x: i32) -> Result<OutputSpec, String> {
    let (size, at) = s.split_once('@').unwrap_or((s, ""));
    let (w, h) = size
        .split_once('x')
        .ok_or_else(|| format!("window {s:?}: give WxH@X,Y"))?;
    let w: u32 = w
        .trim()
        .parse()
        .map_err(|_| format!("window {s:?}: width"))?;
    let h: u32 = h
        .trim()
        .parse()
        .map_err(|_| format!("window {s:?}: height"))?;
    let pos = if at.is_empty() {
        [next_x, 0]
    } else {
        let (x, y) = at
            .split_once(',')
            .ok_or_else(|| format!("window {s:?}: position X,Y"))?;
        [
            x.trim().parse().map_err(|_| format!("window {s:?}: X"))?,
            y.trim().parse().map_err(|_| format!("window {s:?}: Y"))?,
        ]
    };
    if !(16..=16384).contains(&w) || !(16..=16384).contains(&h) {
        return Err(format!("window {s:?}: sizes are 16 to 16384 pixels"));
    }
    Ok(OutputSpec {
        monitor: 0,
        pos: Some(pos),
        window: Some([w, h]),
    })
}

/// Windows as `WxH@X,Y` separated by `;` or given as a list.
pub(super) fn parse_windows<'a>(
    items: impl IntoIterator<Item = &'a str>,
) -> Result<Vec<OutputSpec>, String> {
    let mut out: Vec<OutputSpec> = vec![];
    for s in items {
        let next_x = out.last().map_or(0, |o| {
            o.pos.unwrap_or([0, 0])[0] + o.window.unwrap_or([0, 0])[0] as i32
        });
        out.push(parse_window(s.trim(), next_x)?);
    }
    Ok(out)
}

impl JetcarpApp {
    /// The display state.
    pub(super) fn display_state(&self) -> Value {
        let d = &self.shell.display;
        let now = self.ctx.as_ref().map_or(0.0, |c| c.input(|i| i.time));
        let saver = match &self.display.saver {
            Some(s) => {
                let items: Vec<String> = s.items().iter().map(|i| i.label.clone()).collect();
                let profile = s
                    .current()
                    .filter(|c| c.stored)
                    .map(|c| c.profile.name.clone());
                json!({"active": true, "items": items.len(), "index": s.index(),
                       "showing": s.current().map(|c| c.label.clone()),
                       "seconds_left": s.seconds_left(now),
                       "profile": profile, "settings": d.screensaver})
            }
            None => json!({"active": false, "settings": d.screensaver}),
        };
        let profiles: Vec<Value> = self
            .display
            .profiles
            .list()
            .iter()
            .map(|l| {
                json!({"name": l.profile.name, "kind": l.tag(), "description": l.profile.description,
                       "shows": l.profile.show.describe(), "cell_px": l.profile.cell_px,
                       "speed": l.profile.speed,
                       "reseed": match &l.profile.reseed { None => json!("scene's own"), Some(r) => json!(r.len()) },
                       "file": l.file})
            })
            .collect();
        let rotations: Vec<Value> = self
            .display
            .rotations
            .list()
            .iter()
            .map(|l| {
                json!({"name": l.rotation.name, "builtin": l.builtin,
                       "description": l.rotation.description, "order": l.rotation.order,
                       "items": l.rotation.items.len(), "script": !l.rotation.script.is_empty(),
                       "file": l.file})
            })
            .collect();
        let monitors: Vec<Value> = self
            .monitors_deduped()
            .iter()
            .map(|m| {
                json!({"index": m.index, "name": m.name, "size_px": m.size_px,
                       "position_px": m.position_px, "scale": m.scale_factor,
                       "refresh_hz": m.refresh_millihertz.map(|r| r as f64 / 1000.0),
                       "current": m.current})
            })
            .collect();
        json!({
            "mode": d.mode.id(),
            "fullscreen": self.display.fs_applied.is_some(),
            "fullscreen_monitor": d.fullscreen_monitor,
            "outputs": d.outputs,
            "outputs_open": self.display.outputs.len(),
            "arrangement": match d.arrangement { Arrangement::Span => "span", Arrangement::Mirror => "mirror" },
            "units": match d.units { Units::Pixels => "pixels", Units::Logical => "logical" },
            "hide_preview": d.hide_preview,
            "canvas_px": self.canvas_px(),
            "cell_px": d.cell_px,
            "zoom_px_per_cell": self.zoom * self.cell_unit(),
            "playing": self.playing,
            "speed": self.speed,
            "max_speed": self.max_speed,
            "gpu": self.sim.on,
            "menu_autohide": self.shell.menu_autohide,
            "panes_hidden": self.shell.panes_hidden,
            "status_bar": self.shell.status_bar,
            "keep_awake": {
                "on": d.keep_awake,
                "held": self.display.keep.active(),
                "backend": self.display.keep.backend().map(|b| b.id()),
                "error": self.display.keep.error,
            },
            "screensaver": saver,
            "profiles": profiles,
            "profiles_dir": self.display.profiles.dir(),
            "rotations": rotations,
            "monitors": monitors,
            "scene": self.scene_name,
        })
    }

    /// `app_display`: apply the arguments, then return the state.
    pub(super) fn display_api(&mut self, args: &Value) -> Result<Value, String> {
        check(args, KEYS, "app_display")?;
        let ctx = self.ctx.clone().ok_or("the app has no window yet")?;
        self.profile_store();
        self.rotation_store();
        if let Some(v) = args.get("save_rotation").filter(|v| !v.is_null()) {
            let r: Rotation =
                serde_json::from_value(v.clone()).map_err(|e| format!("save_rotation: {e}"))?;
            self.rotation_store().save(&r)?;
        }
        if let Some(n) = get_str(args, "delete_rotation")? {
            self.rotation_store().delete(n)?;
        }
        let rotation = match get_str(args, "rotation")? {
            Some(n) => Some(self.rotation_store().get(n).ok_or_else(|| {
                format!(
                    "no rotation {n:?}. Rotations: {}",
                    self.display.rotations.names().join(", ")
                )
            })?),
            None => None,
        };
        // A profile: shown by the screensaver from now on (started with `screensaver`, or at
        // once if it runs).
        let profile = match get_str(args, "profile")? {
            Some(n) => Some(self.profile_store().get(n).ok_or_else(|| {
                format!(
                    "no screensaver profile {n:?}. Profiles: {}",
                    self.display.profiles.names().join(", ")
                )
            })?),
            None => None,
        };
        // Settings first (validated before anything changes where possible).
        let mode = get_str(args, "mode")?
            .map(|m| OutputMode::from_id(m).ok_or(format!("unknown mode {m:?}")))
            .transpose()?;
        let arrangement = match get_str(args, "arrangement")? {
            Some("span") => Some(Arrangement::Span),
            Some("mirror") => Some(Arrangement::Mirror),
            Some(x) => return Err(format!("unknown arrangement {x:?}")),
            None => None,
        };
        let units = match get_str(args, "units")? {
            Some("pixels") => Some(Units::Pixels),
            Some("logical") => Some(Units::Logical),
            Some(x) => return Err(format!("unknown units {x:?}")),
            None => None,
        };
        let mut outputs: Option<Vec<OutputSpec>> = None;
        if let Some(m) = args.get("monitors").filter(|m| !m.is_null()) {
            let list = m.as_array().ok_or("`monitors` must be a list of indexes")?;
            let known: Vec<usize> = self.shell.monitors.list().iter().map(|m| m.index).collect();
            let mut v = vec![];
            for x in list {
                let i = x.as_u64().ok_or("`monitors` must be a list of indexes")? as usize;
                if !known.is_empty() && !known.contains(&i) {
                    return Err(format!("no monitor {i} (monitors: {known:?})"));
                }
                // Keep a manual position given before.
                let pos = self
                    .shell
                    .display
                    .outputs
                    .iter()
                    .find(|o| o.monitor == i && o.window.is_none())
                    .and_then(|o| o.pos);
                v.push(OutputSpec {
                    monitor: i,
                    pos,
                    window: None,
                });
            }
            outputs = Some(v);
        }
        if let Some(w) = args.get("windows").filter(|m| !m.is_null()) {
            let list = w
                .as_array()
                .ok_or("`windows` must be a list of \"WxH@X,Y\"")?;
            let strs: Vec<&str> = list.iter().filter_map(|x| x.as_str()).collect();
            if strs.len() != list.len() {
                return Err("`windows` must be a list of \"WxH@X,Y\"".into());
            }
            let mut v = outputs.unwrap_or_default();
            v.extend(parse_windows(strs)?);
            outputs = Some(v);
        }
        if let Some(c) = get_f64(args, "cell_px")?
            && !(1.0..=64.0).contains(&c)
        {
            return Err("`cell_px` is 1 to 64".into());
        }
        if let Some(s) = args.get("saver").filter(|s| !s.is_null()) {
            self.apply_saver_args(s)?;
        }
        // Where the grid shows.
        if let Some(o) = outputs {
            if o.is_empty() && mode.is_none() {
                self.shell.display.mode = OutputMode::Window;
            }
            self.shell.display.outputs = o;
            if mode.is_none() && !self.shell.display.outputs.is_empty() {
                self.shell.display.mode = OutputMode::Outputs;
            }
        }
        if let Some(m) = mode {
            self.shell.display.mode = m;
        }
        if let Some(f) = get_bool(args, "fullscreen")? {
            self.shell.display.mode = if f {
                OutputMode::FullScreen
            } else {
                OutputMode::Window
            };
        }
        if let Some(m) = args.get("fullscreen_monitor") {
            self.shell.display.fullscreen_monitor = m.as_u64().map(|m| m as usize);
        }
        if let Some(a) = arrangement {
            self.shell.display.arrangement = a;
        }
        if let Some(u) = units {
            self.shell.display.units = u;
        }
        if let Some(b) = get_bool(args, "hide_preview")? {
            self.shell.display.hide_preview = b;
        }
        if let Some(b) = get_bool(args, "menu_autohide")? {
            self.shell.menu_autohide = b;
        }
        if let Some(b) = get_bool(args, "panes_hidden")? {
            self.shell.panes_hidden = b;
        }
        if let Some(b) = get_bool(args, "status_bar")? {
            self.shell.status_bar = b;
        }
        if let Some(b) = get_bool(args, "keep_awake")? {
            self.shell.display.keep_awake = b;
        }
        if let Some(c) = get_f64(args, "cell_px")? {
            self.shell.display.cell_px = c as f32;
        }
        if get_bool(args, "refill")? == Some(true) {
            self.fill_canvas();
        }
        if get_bool(args, "pixel_zoom")? == Some(true) {
            self.pixel_zoom(self.shell.display.cell_px as f64);
        }
        if get_bool(args, "identify")? == Some(true) {
            self.identify_monitors(&ctx);
        }
        // Running.
        if let Some(s) = get_f64(args, "speed")? {
            if s <= 0.0 {
                self.max_speed = true;
            } else {
                self.max_speed = false;
                self.speed = s.clamp(0.1, 100_000.0) as f32;
            }
        }
        if let Some(g) = get_bool(args, "gpu")? {
            if g && !self.sim.available() {
                return Err(format!(
                    "GPU simulation is not available: {}",
                    self.sim.hint()
                ));
            }
            self.sim.on = g;
        }
        // The screensaver last, so it starts on the outputs and settings given with it.
        if let Some(p) = &profile {
            let s = &mut self.shell.display.screensaver;
            s.source = SaverSource::Profile;
            s.profile = p.name.clone();
        }
        if let Some(r) = &rotation {
            let s = &mut self.shell.display.screensaver;
            s.source = SaverSource::Rotation;
            s.rotation = r.name.clone();
        }
        let chosen = profile.is_some() || rotation.is_some();
        let saver_given = args.get("saver").is_some_and(|s| !s.is_null());
        match get_bool(args, "screensaver")? {
            Some(true) if self.saver_active() && (chosen || saver_given) => {
                self.saver_restart(&ctx)
            }
            Some(true) => self.saver_enter(&ctx),
            Some(false) => self.saver_leave(&ctx),
            None if self.saver_active() && chosen => self.saver_restart(&ctx),
            None => {}
        }
        if let Some(b) = get_bool(args, "playing")? {
            self.playing = b;
            if b {
                self.screen = Screen::Sim;
            }
        }
        match get_str(args, "skip")? {
            Some("next") => self.saver_skip(1)?,
            Some("previous") => self.saver_skip(-1)?,
            Some(x) => return Err(format!("skip is next or previous, not {x:?}")),
            None => {}
        }
        self.sync_keep_awake();
        ctx.request_repaint();
        Ok(self.display_state())
    }

    fn apply_saver_args(&mut self, s: &Value) -> Result<(), String> {
        check(s, SAVER_KEYS, "saver")?;
        let mut c = self.shell.display.screensaver.clone();
        if let Some(x) = get_str(s, "source")? {
            c.source = SaverSource::from_id(x).ok_or(format!(
                "unknown saver source {x:?} (profile, rotation, current, scene, quickstart)"
            ))?;
        }
        if let Some(x) = get_str(s, "profile")? {
            c.profile = x.to_string();
        }
        if let Some(x) = get_str(s, "scene")? {
            c.scene = x.to_string();
        }
        if let Some(x) = get_str(s, "quickstart")? {
            c.quickstart = x.to_string();
        }
        if let Some(x) = get_str(s, "rotation")? {
            c.rotation = x.to_string();
        }
        for (k, f) in [
            ("refill", &mut c.refill),
            ("gpu", &mut c.gpu),
            ("keep_awake", &mut c.keep_awake),
            ("exit_any_input", &mut c.exit_any_input),
            ("control", &mut c.control),
            ("hide_windows", &mut c.hide_windows),
        ] {
            if let Some(b) = get_bool(s, k)? {
                *f = b;
            }
        }
        if let Some(x) = get_f64(s, "cell_px")? {
            if !(1.0..=64.0).contains(&x) {
                return Err("saver.cell_px is 1 to 64".into());
            }
            c.cell_px = x as f32;
        }
        if let Some(x) = get_f64(s, "speed")? {
            c.speed = x.max(0.0) as f32;
        }
        if let Some(x) = get_f64(s, "cursor_hide_s")? {
            c.cursor_hide_s = x.clamp(0.0, 3600.0) as f32;
        }
        match s.get("caption") {
            None | Some(Value::Null) => {}
            Some(Value::Bool(b)) => c.caption.show = *b,
            Some(o) => {
                let mut merged = serde_json::to_value(c.caption).map_err(|e| e.to_string())?;
                let (Value::Object(m), Value::Object(new)) = (&mut merged, o) else {
                    return Err("`caption` must be true, false, or an object".into());
                };
                for (k, v) in new {
                    if !m.contains_key(k) {
                        return Err(format!(
                            "unknown caption setting `{k}`. Allowed: show, corner, size, generation, seconds"
                        ));
                    }
                    m.insert(k.clone(), v.clone());
                }
                c.caption = serde_json::from_value(merged).map_err(|e| format!("caption: {e}"))?;
            }
        }
        self.shell.display.screensaver = c;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_parse() {
        let v = parse_windows(["800x600@0,0", "640x480", "100x100@-5,7"]).unwrap();
        assert_eq!(v[0].window, Some([800, 600]));
        assert_eq!(v[0].pos, Some([0, 0]));
        assert_eq!(v[1].pos, Some([800, 0]), "placed right of the one before");
        assert_eq!(v[2].pos, Some([-5, 7]));
        assert!(parse_windows(["800"]).is_err());
        assert!(parse_windows(["8x8"]).is_err());
    }
}

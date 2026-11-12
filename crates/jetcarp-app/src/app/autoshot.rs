//! Headless screenshot checks: the `JETCARP_SCREENSHOT` hooks.

use super::*;

/// Debug aid: `JETCARP_SCREENSHOT=out.png [JETCARP_SCENE=name|path|start|blank] [JETCARP_FRAMES=n]` opens a
/// scene, runs n frames, saves a screenshot of the window, and exits. Lets headless checks see the UI.
/// `JETCARP_SCRIPTS=sweep|world|script|help|<example>.rhai` opens the Scripts window;
/// `JETCARP_RESULTS=<sweep folder>` the results browser (`JETCARP_RESULTS_OPEN=k` opens row k).
///
/// The app shell: `JETCARP_MENU_AUTOHIDE=1` auto-hides the menu bar (`=shown` also reveals it),
/// `JETCARP_STATUS_BAR=0` hides the status bar, `JETCARP_PANES=hidden` hides every pane (as Tab does),
/// `JETCARP_MONITORS=1` prints the monitor list, `JETCARP_MENU=<title>` holds that menu open, `JETCARP_FLOAT=<pane>[,<pane>...]` floats panes, `JETCARP_POPOUT=<pane or tool key>[,...]` pops
/// panes or tool windows out to their own OS windows, `JETCARP_HIDE=<pane>[,...]` hides panes, and
/// `JETCARP_SCREENSHOT_VIEWPORT=<pane or tool key>` also saves that popped-out window, next to the
/// main screenshot as `<name>-<key>.png`. `JETCARP_GRID_POPOUT=1` mirrors the grid view in a second
/// OS window (the paint-callback experiment for wave 2's outputs).
pub(super) struct Autoshot {
    path: std::path::PathBuf,
    frames_left: u32,
    requested: bool,
    /// Screenshots still to arrive.
    pending: Arc<std::sync::atomic::AtomicUsize>,
    /// Popped-out windows to save too: viewport id and file.
    extra: Vec<(egui::ViewportId, std::path::PathBuf)>,
    /// Frames since the screenshots were requested (gives up on a window that never answers).
    waited: u32,
}

fn env_list(name: &str) -> Vec<String> {
    std::env::var(name)
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

impl JetcarpApp {
    pub(super) fn with_autoshot(mut self, ctx: &egui::Context) -> Self {
        let Ok(path) = std::env::var("JETCARP_SCREENSHOT") else {
            return self;
        };
        let frames = std::env::var("JETCARP_FRAMES")
            .ok()
            .and_then(|f| f.parse().ok())
            .unwrap_or(90);
        match std::env::var("JETCARP_SCENE").as_deref() {
            Ok("start") | Err(_) => {}
            Ok("empty" | "blank") => self.empty_world(),
            Ok(name) => {
                // A bundled scene, a Quick Start id (`rule:B3/S23`), or a file path.
                let scene = self.menu.scene_of(name).or_else(|| {
                    std::fs::read_to_string(name)
                        .ok()
                        .and_then(|j| Scene::from_json(&j).ok())
                });
                if let Some(s) = scene {
                    self.load_scene_bg(&s);
                }
            }
        }
        // `JETCARP_PAUSED=1`: do not start the scene running (frame times of a static view).
        if std::env::var("JETCARP_PAUSED").is_ok() {
            self.playing = false;
            self.autoplay = false;
        }
        if std::env::var("JETCARP_PALETTES").is_ok() {
            self.pal_open = true;
            self.pick_palette("Prism");
        }
        // `JETCARP_TOOL_PALETTE=<name>` (in `tools.rs`): every tool window in that palette.
        self.tool_palette_env();
        // `JETCARP_SPEED=<gen/s>` (or `max`): run speed, so screenshots get far enough.
        if let Ok(v) = std::env::var("JETCARP_SPEED") {
            match v.parse::<f32>() {
                Ok(s) if s > 0.0 => self.speed = s.min(10_000.0),
                _ if v == "max" => self.max_speed = true,
                _ => eprintln!("JETCARP_SPEED: expected generations per second or max"),
            }
        }
        // `JETCARP_RESEED=<preset>` (in `reseed_ui.rs`) installs a reseed preset once the scene has
        // loaded. `JETCARP_WALLPAPER=1` opens the wallpaper dialog; `=export` also exports once.
        if let Ok(v) = std::env::var("JETCARP_WALLPAPER") {
            self.screen = Screen::Sim;
            self.wallpaper.open = true;
            self.wallpaper.auto_export = v == "export";
        }
        if let Ok(what) = std::env::var("JETCARP_CELL_EDITOR") {
            self.cell_editor.open_env(&what, &self.world.topo.spec);
        }
        if std::env::var("JETCARP_EDITOR").is_ok() {
            self.editor.load_spec(&self.world.topo.spec);
            self.editor.open = true;
        }
        // `JETCARP_SCRIPTS=sweep|world|script|help|<example.rhai>`: open the Scripts window.
        if let Ok(what) = std::env::var("JETCARP_SCRIPTS") {
            use crate::scripts_ui::Tab;
            self.screen = Screen::Sim;
            self.scripts.open = true;
            match what.as_str() {
                "world" => self.scripts.sweep_from_world(&self.world),
                "script" => self.scripts.tab = Tab::Script,
                "help" => self.scripts.tab = Tab::Help,
                name if name.ends_with(".rhai") => self.scripts.load_example(name),
                _ => {}
            }
            // `JETCARP_SWEEP=W:0-255` (series:from-to) sets the form's rules;
            // `JETCARP_SCRIPTS_RUN=1` presses Run once the view has its size.
            if let Ok(s) = std::env::var("JETCARP_SWEEP") {
                use crate::sweep_form::Series;
                let (name, range) = s.split_once(':').unwrap_or((s.as_str(), ""));
                if let Some((k, _)) = Series::ALL
                    .iter()
                    .find(|(k, _)| format!("{k:?}").eq_ignore_ascii_case(name))
                {
                    self.scripts.form.set_series(*k);
                }
                if let Some((a, b)) = range.split_once('-')
                    && let (Ok(a), Ok(b)) = (a.parse(), b.parse())
                {
                    (self.scripts.form.from, self.scripts.form.to) = (a, b);
                }
            }
            if std::env::var("JETCARP_SCRIPTS_RUN").is_ok() {
                self.scripts.autorun = Some(5);
            }
        }
        // `JETCARP_RESULTS=<sweep folder>`: open the results browser on it.
        // `JETCARP_LIBRARY_WINDOW=1` (or a pattern id/name to select) opens the Library window.
        if let Ok(what) = std::env::var("JETCARP_LIBRARY_WINDOW") {
            self.screen = Screen::Sim;
            self.library.open = true;
            self.library.reload();
            if what != "1" {
                self.library.select_key(&what);
            }
        }
        // `JETCARP_DASHBOARD=1|<job>` opens the Hunts dashboard (selecting that job);
        // `JETCARP_DASHBOARD_HUNT=<rules>@<tilings>@<soups|forever>` starts a hunt there.
        if let Ok(what) = std::env::var("JETCARP_DASHBOARD") {
            self.dash.open_env(&what);
        }
        // `JETCARP_QS_SEARCH=text` searches the menu.
        if let Ok(t) = std::env::var("JETCARP_QS_SEARCH") {
            self.menu.view.filter.text = t;
        }
        // `JETCARP_QS_CATEGORY=<category>`: start with that category selected.
        if let Ok(c) = std::env::var("JETCARP_QS_CATEGORY") {
            self.menu.view.filter.behaviour = c;
        }
        // `JETCARP_QS_CARD_W=<points>`: card width on the grid.
        if let Ok(w) = std::env::var("JETCARP_QS_CARD_W")
            && let Ok(w) = w.parse()
        {
            self.menu.view.card_w = w;
        }
        // `JETCARP_CATALOG_TOOLS=collide|remove|evolve[:run]` opens the Catalog tools window;
        // `JETCARP_TOOLS_A` / `JETCARP_TOOLS_B` set the patterns to collide.
        if let Ok(t) = std::env::var("JETCARP_CATALOG_TOOLS") {
            self.screen = Screen::Sim;
            self.catalog_tools.open_env(&t);
            if let Ok(a) = std::env::var("JETCARP_TOOLS_A") {
                self.catalog_tools.a = a.clone();
                self.catalog_tools.remove_id = a;
            }
            if let Ok(b) = std::env::var("JETCARP_TOOLS_B") {
                self.catalog_tools.b = b;
            }
        }
        // `JETCARP_ANTS=<tilings>:<min>-<max>` starts an ant search in the Ants window (`list`: open).
        if let Ok(a) = std::env::var("JETCARP_ANTS") {
            self.screen = Screen::Sim;
            self.ants.open_env(&a);
        }
        // `JETCARP_PILE=<tiling>:<grains>` opens the Sandpile window and starts that pile.
        if let Ok(p) = std::env::var("JETCARP_PILE") {
            self.screen = Screen::Sim;
            self.pile.open_env(&p);
        }
        // `JETCARP_PASTE=<pattern>@<cell>[,<cell>...]`: paste a library pattern as clicks would.
        if let Ok(p) = std::env::var("JETCARP_PASTE")
            && let Some((key, cells)) = p.split_once('@')
        {
            let lib = jetcarp_store::Library::open_default();
            let src = jetcarp_core::pattern::PatternSource {
                library: Some(key.to_string()),
                ..Default::default()
            };
            match (
                lib.get(key),
                jetcarp_core::pattern::paste::resolve(&src, 0, Some(&lib)),
            ) {
                (Ok(r), Ok(source)) => {
                    self.paste = Some(crate::library_ui::PasteRequest {
                        source,
                        label: r.label(),
                        rule: r.rule.clone(),
                        orientation: 0,
                    });
                    for c in cells.split(',').filter_map(|c| c.trim().parse().ok()) {
                        self.paste_at(c);
                    }
                }
                (Err(e), _) | (_, Err(e)) => self.message = Some((e, true)),
            }
        }
        if let Ok(f) = std::env::var("JETCARP_RESULTS") {
            self.screen = Screen::Sim;
            self.results.open_folder(std::path::Path::new(&f));
            if let Ok(k) = std::env::var("JETCARP_RESULTS_OPEN")
                && let Ok(k) = k.parse()
            {
                self.results.open_row(k);
            }
        }
        match std::env::var("JETCARP_MENU_AUTOHIDE").as_deref() {
            Ok("shown") => {
                self.shell.menu_autohide = true;
                self.shell.force_reveal = true;
            }
            Ok(v) if v != "0" => self.shell.menu_autohide = true,
            _ => {}
        }
        self.shell.open_menu = std::env::var("JETCARP_MENU").ok();
        if std::env::var("JETCARP_STATUS_BAR").is_ok_and(|v| v == "0") {
            self.shell.status_bar = false;
        }
        if std::env::var("JETCARP_PANES").is_ok_and(|v| v == "hidden") {
            self.shell.panes_hidden = true;
        }
        for id in env_list("JETCARP_FLOAT") {
            self.shell.layout.set_place(&id, panes::Place::Floating);
        }
        for id in env_list("JETCARP_HIDE") {
            self.shell.layout.set_place(&id, panes::Place::Hidden);
        }
        for key in env_list("JETCARP_POPOUT") {
            if panes::pane_def(&key).is_some() {
                self.shell.layout.set_place(&key, panes::Place::Popped);
            } else if let Some(t) = menu::ToolWin::from_key(&key) {
                crate::popout::set_popped(ctx, &key, true);
                if !self.tool_open(t) {
                    self.screen = Screen::Sim;
                    self.set_tool_open(t, true);
                }
            } else {
                eprintln!("JETCARP_POPOUT: no pane or tool window {key:?}");
            }
        }
        // Displays: `JETCARP_OUTPUTS=0,1` (monitors) or `JETCARP_OUTPUTS=800x600@0,0;800x600@800,0`
        // (plain windows as outputs), `JETCARP_ARRANGEMENT=span|mirror`, `JETCARP_FULLSCREEN=1`,
        // `JETCARP_CELL_PX=n`, `JETCARP_FILL=1` (fill the canvas once it has settled),
        // `JETCARP_PIXEL_ZOOM=1`, `JETCARP_SAVER=1` (screensaver mode), `JETCARP_HIDE_PREVIEW=1`,
        // `JETCARP_PANE=<id>` (show that pane's tab).
        if let Ok(o) = std::env::var("JETCARP_OUTPUTS") {
            let d = &mut self.shell.display;
            d.outputs = if o.contains('x') {
                match display_api::parse_windows(o.split(';')) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("JETCARP_OUTPUTS: {e}");
                        vec![]
                    }
                }
            } else {
                o.split(',')
                    .filter_map(|m| m.trim().parse().ok())
                    .map(|m| display::OutputSpec {
                        monitor: m,
                        pos: None,
                        window: None,
                    })
                    .collect()
            };
            d.mode = display::OutputMode::Outputs;
        }
        if let Ok(a) = std::env::var("JETCARP_ARRANGEMENT") {
            self.shell.display.arrangement = if a == "mirror" {
                crate::span::Arrangement::Mirror
            } else {
                crate::span::Arrangement::Span
            };
        }
        if std::env::var("JETCARP_HIDE_PREVIEW").is_ok() {
            self.shell.display.hide_preview = true;
        }
        if std::env::var("JETCARP_FULLSCREEN").is_ok() {
            self.shell.display.mode = display::OutputMode::FullScreen;
        }
        if let Ok(c) = std::env::var("JETCARP_CELL_PX")
            && let Ok(c) = c.parse::<f32>()
        {
            self.shell.display.cell_px = c.clamp(1.0, 64.0);
            self.shell.display.screensaver.cell_px = c.clamp(1.0, 64.0);
        }
        self.display.fill_pending = std::env::var("JETCARP_FILL").is_ok();
        self.display.pixel_zoom_pending = std::env::var("JETCARP_PIXEL_ZOOM").is_ok();
        // `JETCARP_SPEED=n|max`: generations per second (frame time checks).
        match std::env::var("JETCARP_SPEED").as_deref() {
            Ok("max") => self.max_speed = true,
            Ok(v) => {
                if let Ok(v) = v.parse::<f32>() {
                    self.speed = v;
                }
            }
            Err(_) => {}
        }
        if let Ok(p) = std::env::var("JETCARP_PANE") {
            self.shell.layout.show_pane(&p);
        }
        // `JETCARP_SAVER_PROFILE=<name>`: the screensaver shows that profile; with `JETCARP_SAVER`
        // it starts in it, else the profile is loaded into the window (Try in this window).
        if let Ok(name) = std::env::var("JETCARP_SAVER_PROFILE") {
            match self.profile_store().get(&name) {
                Some(p) if std::env::var("JETCARP_SAVER").is_ok() => {
                    let s = &mut self.shell.display.screensaver;
                    s.source = display::SaverSource::Profile;
                    s.profile = p.name;
                }
                Some(p) => self.profile_try(&p),
                None => eprintln!("JETCARP_SAVER_PROFILE: no profile {name:?}"),
            }
        }
        // `JETCARP_SAVER_ROTATION=<name>`: the screensaver runs that rotation.
        if let Ok(name) = std::env::var("JETCARP_SAVER_ROTATION") {
            let s = &mut self.shell.display.screensaver;
            s.source = display::SaverSource::Rotation;
            s.rotation = name;
            // `JETCARP_ROTATION_EDIT=1`: with the rotation's editor open.
            if std::env::var("JETCARP_ROTATION_EDIT").is_ok() {
                self.rotation_edit_chosen();
            }
        }
        if std::env::var("JETCARP_SAVER").is_ok() {
            self.saver_enter(ctx);
        }
        self.shell.grid_mirror = match std::env::var("JETCARP_GRID_POPOUT").as_deref() {
            Ok("deferred") => shell::Mirror::Deferred,
            Ok(v) if v != "0" => shell::Mirror::Immediate,
            _ => shell::Mirror::Off,
        };
        let path: std::path::PathBuf = path.into();
        let extra = env_list("JETCARP_SCREENSHOT_VIEWPORT")
            .into_iter()
            .map(|key| {
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("shot");
                let file = path.with_file_name(format!("{stem}-{key}.png"));
                (crate::popout::viewport_id(&key), file)
            })
            .collect();
        self.autoshot = Some(Autoshot {
            path,
            frames_left: frames,
            requested: false,
            pending: Default::default(),
            extra,
            waited: 0,
        });
        self
    }

    pub(super) fn autoshot_tick(&mut self, ctx: &egui::Context) {
        let Some(a) = &mut self.autoshot else { return };
        ctx.request_repaint();
        if a.requested {
            a.waited += 1;
        }
        let left = a.pending.load(std::sync::atomic::Ordering::SeqCst);
        if a.requested && left > 0 && a.waited > 300 {
            eprintln!("screenshot: {left} window(s) never answered; giving up");
            a.pending.store(0, std::sync::atomic::Ordering::SeqCst);
        }
        if a.requested && a.pending.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            if let Some(f) = &self.frametime {
                println!("{}", f.report());
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.building.is_some() || self.display.fill_pending || self.display.pixel_zoom_pending {
            return;
        }
        if a.frames_left > 0 {
            a.frames_left -= 1;
            return;
        }
        if !a.requested {
            a.requested = true;
            // `JETCARP_MONITORS=1`: print the monitor list (wave 2's outputs choose from it).
            if std::env::var_os("JETCARP_MONITORS").is_some() {
                for m in self.shell.monitors.list() {
                    println!(
                        "monitor {}: {}{}",
                        m.index,
                        m.describe(),
                        if m.current { " (this window)" } else { "" }
                    );
                }
            }
            let mut shots = vec![(egui::ViewportId::ROOT, a.path.clone())];
            shots.extend(a.extra.iter().cloned());
            a.pending
                .store(shots.len(), std::sync::atomic::Ordering::SeqCst);
            for (vp, path) in shots {
                let pending = a.pending.clone();
                ctx.send_viewport_cmd_to(
                    vp,
                    egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                        let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                        let out = render::Image {
                            width: img.size[0] as u32,
                            height: img.size[1] as u32,
                            rgba,
                        };
                        if let Err(e) = render::write_png(&path, &out, &[]) {
                            eprintln!("screenshot: {e}");
                        }
                        println!("screenshot saved to {}", path.display());
                        pending.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                    })),
                );
            }
        }
    }
}

//! The whole app drawn headlessly, for the book's pictures of it (tests only). `JetcarpApp` runs
//! as eframe would run it, on [`Offscreen`]'s device: its grid is drawn by its own paint
//! callback, and widgets are clicked by their text (AccessKit's tree says where they are). The
//! `#[ignore]`d tests write PNGs to `JETCARP_OFFSCREEN_OUT`; every folder must be a scratch one
//! (`JETCARP_CONFIG_DIR`, `JETCARP_DATA_DIR`, `JETCARP_LIBRARY`, `JETCARP_OUT`,
//! `JETCARP_RUNTIME_DIR`). Run them one at a time:
//! `cargo test -p jetcarp-app --release shots -- --ignored --test-threads 1`.
//!
//! The Screensaver pane prints the settings folder, so the book's pictures were made with the test
//! binary run from a scratch folder and `JETCARP_CONFIG_DIR='~/.config/jetcarp'` (relative: a folder
//! named `~` in the scratch folder), which reads as the usual place rather than a scratch path. The
//! book has them as lossless WebP (`cwebp -lossless -z 9`).

use super::*;
use crate::offscreen::Offscreen;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The app on an offscreen device, and where its pictures go.
struct Harness {
    o: Offscreen,
    app: JetcarpApp,
    frame: eframe::Frame,
    dir: PathBuf,
}

impl Harness {
    /// The app in a window of `w` × `h` points, as at its first launch but with the setup
    /// window closed, its photosensitive seizure warning answered. `None` without
    /// `JETCARP_OFFSCREEN_OUT` or a GPU.
    fn new(w: u32, h: u32) -> Option<Harness> {
        Harness::open(w, h, true)
    }

    /// [`new`](Self::new), with the photosensitive seizure warning answered in the scratch
    /// settings folder (`answered`), or not (it then shows first, #96).
    fn open(w: u32, h: u32, answered: bool) -> Option<Harness> {
        let dir = PathBuf::from(std::env::var_os("JETCARP_OFFSCREEN_OUT")?);
        std::fs::create_dir_all(&dir).ok()?;
        for v in [
            "JETCARP_CONFIG_DIR",
            "JETCARP_DATA_DIR",
            "JETCARP_LIBRARY",
            "JETCARP_OUT",
            "JETCARP_RUNTIME_DIR",
        ] {
            assert!(std::env::var_os(v).is_some(), "set {v} to a scratch folder");
        }
        use jetcarp_core::photosensitivity as ps;
        let settings = jetcarp_core::paths::config_dir().expect("JETCARP_CONFIG_DIR is set");
        if !answered {
            let _ = std::fs::remove_file(ps::file_in(&settings));
        } else if ps::read_in(&settings).is_none() {
            ps::record_in(&settings, ps::Answer::NoDisorder, "test").unwrap();
        }
        let Some(o) = Offscreen::new(w, h) else {
            return None;
        };
        o.ctx.enable_accesskit();
        let mut cc = eframe::CreationContext::_new_kittest(o.ctx.clone());
        cc.wgpu_render_state = Some(o.render_state());
        let mut app = JetcarpApp::new(&cc, crate::cli::LaunchArgs::default());
        // As a first launch looks, whatever an earlier test left in the settings file.
        app.setup.open = false;
        app.shell.layout = panes::Layout::default();
        for t in menu::ToolWin::ALL {
            app.set_tool_open(t, false);
        }
        Some(Harness {
            o,
            app,
            frame: eframe::Frame::_new_kittest(),
            dir,
        })
    }

    /// One frame, `dt` seconds after the last, with these input events: `logic` then `ui`, as
    /// eframe runs them.
    fn frame_with(&mut self, dt: f64, events: Vec<egui::Event>) -> Vec<u8> {
        let (app, frame) = (&mut self.app, &mut self.frame);
        self.o.frame_with(dt, events, |ctx| {
            ctx.root_ui(|ui| {
                eframe::App::logic(app, ui.ctx(), frame);
                eframe::App::ui(app, ui, frame)
            })
        })
    }

    /// Frames for `secs` of real time (each `dt` of the app's time), or until `done`.
    fn run(&mut self, secs: f64, dt: f64, mut done: impl FnMut(&JetcarpApp) -> bool) -> bool {
        let t = Instant::now();
        while t.elapsed().as_secs_f64() < secs {
            self.frame_with(dt, vec![]);
            if done(&self.app) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// A few quiet frames, the last one saved as `<name>.png`.
    fn shot(&mut self, name: &str) -> Vec<u8> {
        let mut px = vec![];
        for _ in 0..4 {
            px = self.frame_with(0.02, vec![]);
        }
        let p = self.dir.join(format!("{name}.png"));
        self.o.save(&px, &p).unwrap();
        eprintln!("wrote {}", p.display());
        px
    }

    /// [`shot`](Self::shot) of a part of the window, in points.
    fn shot_part(&mut self, name: &str, part: egui::Rect) {
        let mut px = vec![];
        for _ in 0..4 {
            px = self.frame_with(0.02, vec![]);
        }
        let p = self.dir.join(format!("{name}.png"));
        self.o.save_part(&px, part, &p).unwrap();
        eprintln!("wrote {}", p.display());
    }

    /// The pointer moved to `at`, for a few frames (opens submenus, shows hover text).
    fn hover(&mut self, at: egui::Pos2) {
        for _ in 0..3 {
            self.frame_with(0.05, vec![egui::Event::PointerMoved(at)]);
        }
    }

    /// A click of `button` at `at`.
    fn click_at(&mut self, at: egui::Pos2, button: egui::PointerButton) {
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame_with(0.05, vec![egui::Event::PointerMoved(at)]);
        self.frame_with(0.05, vec![press(true)]);
        self.frame_with(0.05, vec![press(false)]);
        self.frame_with(0.05, vec![]);
    }

    /// The left button pressed at `from`, the pointer moved to `to` in steps, and let go there (a
    /// window dragged by its title bar).
    fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame_with(0.05, vec![egui::Event::PointerMoved(from)]);
        self.frame_with(0.05, vec![button(from, true)]);
        for i in 1..=8 {
            let p = from + (to - from) * (i as f32 / 8.0);
            self.frame_with(0.05, vec![egui::Event::PointerMoved(p)]);
        }
        self.frame_with(0.05, vec![button(to, false)]);
        self.frame_with(0.05, vec![]);
    }

    /// Where the widget with this text is (see [`Offscreen::find`]); panics if it is not drawn.
    fn find(&self, text: &str) -> egui::Rect {
        self.o.find(text).unwrap_or_else(|| {
            let mut names: Vec<&str> = self.o.widgets.iter().map(|w| w.0.as_str()).collect();
            names.sort();
            names.dedup();
            panic!("no widget {text:?} on screen; there are {names:?}")
        })
    }

    /// A left click on the widget with this text.
    fn click(&mut self, text: &str) {
        let r = self.find(text);
        self.click_at(r.center(), egui::PointerButton::Primary);
    }

    /// A click on the nearest widget right of the one with this text, on the same line (a text
    /// box after its label or a box before it).
    fn click_right_of(&mut self, text: &str) {
        let r = self.find(text);
        let next = self
            .o
            .widgets
            .iter()
            .map(|w| w.1)
            .filter(|w| w.min.x >= r.max.x && w.min.y < r.max.y && w.max.y > r.min.y)
            .min_by(|a, b| a.min.x.total_cmp(&b.min.x))
            .unwrap_or_else(|| panic!("nothing right of {text:?}"));
        self.click_at(next.center(), egui::PointerButton::Primary);
    }

    /// The mouse wheel turned over `at`, moving the content up by `dy` points.
    fn scroll(&mut self, at: egui::Pos2, dy: f32) {
        self.frame_with(0.05, vec![egui::Event::PointerMoved(at)]);
        self.frame_with(
            0.05,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -dy),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        for _ in 0..6 {
            self.frame_with(0.05, vec![]);
        }
    }

    /// Scroll the content under `at` until the widget with this text is above `bottom`.
    fn scroll_to(&mut self, text: &str, at: egui::Pos2, bottom: f32) {
        for _ in 0..40 {
            if self
                .o
                .find(text)
                .is_some_and(|r| r.max.y < bottom && r.min.y > 30.0)
            {
                return;
            }
            self.scroll(at, 120.0);
        }
        panic!("{text:?} never scrolled into view");
    }

    /// The pointer on the widget with this text (a submenu opens).
    fn hover_on(&mut self, text: &str) {
        let r = self.find(text);
        self.hover(r.center());
    }

    /// Wait for a scene being built to be in place.
    fn built(&mut self) {
        let ok = self.run(120.0, 0.05, |a| {
            a.building.is_none() && a.screen == Screen::Sim
        });
        assert!(ok, "the world was never built");
    }

    /// Frames until the fps shown has settled near 60 (the app's time moves 1/60 s a frame).
    fn steady(&mut self) {
        for _ in 0..40 {
            self.frame_with(1.0 / 60.0, vec![]);
        }
    }

    /// The left column of panes takes this share of the window's width (the default is 0.24),
    /// and its upper tab group this share of its height (0.45).
    fn column(&mut self, width: f32, upper: f32) {
        use egui_tiles::{Container, Tile};
        let tiles = &mut self.app.shell.layout.tree.tiles;
        let Some(root) = self.app.shell.layout.tree.root else {
            return;
        };
        let kids = match tiles.get(root) {
            Some(Tile::Container(Container::Linear(l))) => l.children.clone(),
            _ => return,
        };
        if let Some(Tile::Container(Container::Linear(l))) = tiles.get_mut(root) {
            l.shares.set_share(kids[0], width);
            l.shares.set_share(kids[1], 1.0 - width);
        }
        let col = match tiles.get(kids[0]) {
            Some(Tile::Container(Container::Linear(l))) => l.children.clone(),
            _ => return,
        };
        if col.len() == 2
            && let Some(Tile::Container(Container::Linear(l))) = tiles.get_mut(kids[0])
        {
            l.shares.set_share(col[0], upper);
            l.shares.set_share(col[1], 1.0 - upper);
        }
    }

    /// The menu bar's menu `title` held open ("": closed again), the pointer out of its way.
    fn hold_menu(&mut self, title: &str) {
        let [w, h] = self.o.size();
        self.hover(egui::pos2(w as f32 - 2.0, h as f32 - 2.0));
        self.app.shell.open_menu = (!title.is_empty()).then(|| title.to_string());
        for _ in 0..3 {
            self.frame_with(0.05, vec![]);
        }
    }

    /// Esc pressed and let go (closes menus and popups).
    fn escape(&mut self) {
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame_with(0.05, vec![key(true)]);
        self.frame_with(0.05, vec![key(false)]);
        self.frame_with(0.05, vec![]);
    }

    /// An app action, as the menu or a shortcut does it.
    fn action(&mut self, a: menu::Action) {
        let ctx = self.o.ctx.clone();
        self.app.run_action(&ctx, a);
        self.frame_with(0.05, vec![]);
    }

    /// The Quick Start screen loaded, and its cards given time to start.
    fn quick_start_loaded(&mut self) {
        let ok = self.run(120.0, 0.05, |a| a.menu.test_loaded());
        assert!(ok, "the Quick Start never loaded");
        self.run(8.0, 0.05, |_| false);
    }

    /// Open the Quick Start entry with this handle (`life`, `qs-…`), as a click on its card does.
    fn open_entry(&mut self, handle: &str) {
        let i = self
            .app
            .menu
            .items
            .iter()
            .position(|it| it.handle == handle)
            .unwrap_or_else(|| panic!("no Quick Start entry {handle}"));
        let s = self.app.menu.full_scene(i).unwrap();
        self.app.load_scene_bg(&s);
        self.built();
    }

    /// The rectangle holding all of these, grown by `margin` points.
    fn around(rects: &[egui::Rect], margin: f32) -> egui::Rect {
        let mut r = rects[0];
        for x in &rects[1..] {
            r = r.union(*x);
        }
        r.expand(margin)
    }
}

/// The handle of the first Quick Start entry whose title starts with `title`.
fn handle_titled(m: &start::Menu, title: &str) -> String {
    m.items
        .iter()
        .find(|it| it.title.starts_with(title))
        .unwrap_or_else(|| panic!("no Quick Start entry titled {title:?}"))
        .handle
        .clone()
}

/// The book's tour of the app (`getting-started/tour.md`): the Quick Start screen, a card's
/// menu, a world with its panes, and the Tools and Window menus. The book's pictures are of a
/// library set up by `jetcarp setup --profile explorer --yes --quickstart-minutes 1` (the starter
/// pack and a one-minute build), with `JETCARP_QS_SEED=7`.
#[test]
#[ignore]
fn shots_tour() {
    let Some(mut h) = Harness::new(1400, 900) else {
        return;
    };
    h.quick_start_loaded();
    h.steady();
    h.shot("tour-quick-start");
    // A card's menu, by a right click on the first card.
    let (_, card) = h.app.menu.test_first_card().expect("a card on screen");
    h.click_at(card.center(), egui::PointerButton::Secondary);
    let places = h.app.menu.test_menu_places();
    assert_eq!(places.len(), 6, "five stars and Hide");
    let mut all = vec![card];
    all.extend(places);
    h.shot_part("tour-card-menu", Harness::around(&all, 12.0));
    h.escape();
    // A world: an entry of the starter pack, its panes docked as at first.
    let handle = handle_titled(&h.app.menu, "Oscillating Greenberg-Hastings");
    h.open_entry(&handle);
    h.run(4.0, 0.05, |_| false);
    h.steady();
    h.shot("tour-world");
    // The Tools menu.
    h.hold_menu("Tools");
    let first = h.find("Scripts and sweeps…");
    let last = h.find("AI control…");
    let bar = h.find("Tools");
    h.shot_part("tour-tools-menu", Harness::around(&[first, last, bar], 8.0));
    h.hold_menu("");
    h.escape();
    // The Window menu: every pane and tool window, and where each is.
    h.hold_menu("Window");
    let bar = h.find("Window");
    let last = h.find("Displays");
    let wide = h.find("Scripts and sweeps  (closed)");
    h.shot_part("tour-window-menu", Harness::around(&[bar, last, wide], 8.0));
    h.hold_menu("");
    h.escape();
}

/// The book's photosensitive seizure warning (`getting-started/photosensitivity.md`, #96): the
/// app's first screen with no answer saved, and the advice that follows the answer that you have,
/// or may have, a seizure disorder. Until it is answered nothing else shows and nothing runs;
/// *Continue at my own risk* saves that answer and the launch goes on.
#[test]
#[ignore]
fn shots_photosensitivity_warning() {
    use jetcarp_core::photosensitivity as ps;
    let Some(mut h) = Harness::open(960, 540, false) else {
        return;
    };
    assert!(h.app.warning.showing && h.app.pending_launch.is_some());
    h.shot("photosensitivity-1-warning");
    assert!(h.o.find("Quick Start").is_none(), "only the warning shows");
    h.click(ps::AT_RISK);
    h.shot("photosensitivity-2-at-risk");
    h.click(ps::CONTINUE);
    assert!(!h.app.warning.showing && h.app.pending_launch.is_none());
    assert_eq!(ps::read().map(|r| r.answer), Some(ps::Answer::AtRisk));
    // Answered as the other pictures are, for the tests after this one.
    ps::record(ps::Answer::NoDisorder, "test").unwrap();
}

/// Not a picture: the AI control window opens on the Quick Start, with no world open, and its
/// checkbox opens the endpoint (issue #1: the window was drawn only with a world open, so the menu
/// item ticked a window that never showed).
#[test]
#[ignore]
fn ai_control_window_on_the_quick_start() {
    let Some(mut h) = Harness::new(1200, 800) else {
        return;
    };
    assert!(h.app.screen == Screen::Start);
    h.action(menu::Action::Tool(menu::ToolWin::AiControl));
    h.run(1.0, 0.05, |_| false);
    h.click("Allow AI control (127.0.0.1 only)");
    assert!(h.app.ai_port().is_some(), "the endpoint opened");
    h.find("Address");
    h.click("Allow AI control (127.0.0.1 only)");
    assert!(h.app.ai_port().is_none(), "and closed again");
    assert!(h.app.screen == Screen::Start);
}

/// The AI control window with AI control allowed (`tutorials/mcp.md`, #114): how to connect an
/// assistant (`jetcarp mcp --app`, with its Copy button), the address and token, and the note that
/// the address is the MCP server's private link to the app, not an endpoint for HTTP clients. Over
/// the Quick Start, so it needs no library of its own.
#[test]
#[ignore]
fn shots_ai_control() {
    let Some(mut h) = Harness::new(1200, 800) else {
        return;
    };
    h.action(menu::Action::Tool(menu::ToolWin::AiControl));
    h.run(1.0, 0.05, |_| false);
    h.click("Allow AI control (127.0.0.1 only)");
    assert!(h.app.ai_port().is_some(), "the endpoint opened");
    // Down from the menu bar by its title bar ("AI control" is the whole window), so the picture
    // shows the window alone, and the pointer out of it, so nothing in it is highlighted.
    let win = h.find("AI control");
    let bar = egui::pos2(win.center().x, win.min.y + 10.0);
    h.drag(bar, bar + egui::vec2(0.0, 140.0));
    h.hover(egui::pos2(2.0, 798.0));
    let win = h.find("AI control");
    assert!(win.min.y > 100.0, "the window was dragged down: {win:?}");
    h.shot_part("mcp-ai-control", Harness::around(&[win], 2.0));
    h.click("Allow AI control (127.0.0.1 only)");
    assert!(h.app.ai_port().is_none(), "and closed again");
}

/// Tutorial 1 (`tutorials/first-wallpaper.md`): a Quick Start entry opened, its palette list
/// open in the CAs pane; then the Screensaver pane, with the world saved as a profile; then the
/// Desktop background pane (its rotation is the host settings', *My wallpaper* in the book's
/// picture).
#[test]
#[ignore]
fn shots_wallpaper() {
    let Some(mut h) = Harness::new(1400, 900) else {
        return;
    };
    h.quick_start_loaded();
    h.open_entry("penrose-rhomb-brain");
    h.run(3.0, 0.05, |_| false);
    // The CAs pane's palette list.
    let pal = h.app.world.cas[0].palette.clone();
    eprintln!("palette: {pal}");
    h.click(&pal);
    h.hover_on("Aurora");
    h.steady();
    h.shot("wallpaper-palette-list");
    h.click("Aurora");
    h.run(2.0, 0.05, |_| false);
    // The Screensaver pane, in a wider column of panes.
    h.action(menu::Action::Pane("screensaver"));
    h.column(0.36, 0.7);
    h.frame_with(0.05, vec![]);
    // Saved as a profile (once: a later run finds it saved), and chosen under Show.
    let name = h.app.scene_name.clone();
    if !h.app.profile_store().names().contains(&name) {
        let pane = h.find("Start the screensaver").center();
        let bottom = h.find("CAs").min.y;
        h.scroll_to("Save current world as profile", pane, bottom);
        h.click("Save current world as profile");
        h.scroll(pane, -4000.0);
        h.run(0.5, 0.05, |_| false);
    }
    // Show → Profile; its list scrolls, and the new profile is at the end. (A second try if the
    // first click missed while the list was still moving.)
    for _ in 0..2 {
        h.click_right_of("Profile");
        let item = h.find("HighLife").center();
        h.scroll(item, 2000.0);
        h.click(&name);
        if h.app.shell.display.screensaver.profile == name {
            break;
        }
        h.escape();
    }
    assert_eq!(h.app.shell.display.screensaver.profile, name);
    // Back to the pane's top.
    h.scroll(egui::pos2(200.0, 300.0), -4000.0);
    h.find("Start the screensaver");
    h.steady();
    h.shot("wallpaper-screensaver-pane");
    // The Desktop background pane, from the link at the Screensaver pane's foot.
    h.app
        .display
        .host
        .test_extension(jetcarp_display::gnome::ExtensionState::NotInstalled);
    h.action(menu::Action::Pane("desktop"));
    // Tall enough for the whole pane, down to the Performance group's pacing (and, while Fit to
    // this computer is in the release, its button: #112).
    h.column(0.36, 0.94);
    h.frame_with(0.05, vec![]);
    h.find(if jetcarp_core::fit::FIT {
        "Fit to this computer…"
    } else {
        "Show every generation"
    });
    h.steady();
    h.shot("wallpaper-desktop-pane");
}

/// The Desktop background pane after *Fit to this computer* (`guides/wallpapers.md`): each scene's
/// cards, with their pictures. A real sweep saved in the scratch settings is shown when there is
/// one (`jetcarp host tune` run with the same `JETCARP_CONFIG_DIR` first); else two scenes' cards
/// are made up here, their pictures drawn on the processor.
///
/// Fit is switched off in 0.4.2 (#112), so there are no cards to picture and the book has no such
/// picture: this does nothing until Fit returns in 0.4.3 (#113).
#[test]
#[ignore]
fn shots_desktop_pane_cards() {
    use jetcarp_display::host_settings::{
        GpuTiming, HostSettings, Place, Rung, Size, TunedScene, Tuning, crops_dir,
    };
    if !jetcarp_core::fit::FIT {
        eprintln!("Fit to this computer is switched off (#112): no cards to picture");
        return;
    }
    // Tall enough for the Performance group and three scenes' cards below it.
    let Some(mut h) = Harness::new(1600, 1100) else {
        return;
    };
    let (mut s, _) = HostSettings::load();
    let swept = s.screens[0]
        .tuning
        .as_ref()
        .is_some_and(|t| t.scenes.iter().any(|x| !x.rungs.is_empty()));
    if !swept {
        s.screens[0].place = Place::Wallpaper;
        // The cards show only the scenes of the rotation shown (#73): these two are its first.
        s.screens[0].rotation = "Until it settles".into();
        let dir = crops_dir().unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        let mut scenes = vec![];
        for (k, (name, rule, own)) in [
            ("Quick Start rule:B3/S23", "B3/S23", 3.0f32),
            ("Quick Start rule:B36/S23", "B36/S23", 4.0),
        ]
        .into_iter()
        .enumerate()
        {
            let mut rungs = vec![];
            for (i, px) in [1.0f32, 2.0, 3.0, 4.0, 6.0, 8.0].into_iter().enumerate() {
                let (w, h) = (
                    (192.0 / px).ceil() as u32 + 2,
                    (120.0 / px).ceil() as u32 + 2,
                );
                let mut sc = Scene::new(TilingSpec::rect(
                    "square",
                    w.max(8),
                    h.max(8),
                    Boundary::Wrap,
                ));
                sc.rng_state = 7 + k as u64;
                sc.cas = vec![CaSpec::new(rule).seed(SeedSpec::Random { density: 0.3 })];
                let mut world = World::from_scene(&sc).unwrap();
                let _ = world.apply(Command::Step { gens: 30 });
                let b = world.topo.bounds;
                let img = jetcarp_core::render::render_view(
                    &world,
                    192,
                    120,
                    [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0],
                    px as f64,
                    &jetcarp_core::render::ViewOptions::default(),
                );
                let file = format!("s0-made-up-{k}-{i}.png");
                std::fs::write(
                    dir.join(&file),
                    jetcarp_core::media::encode::png(&img).unwrap(),
                )
                .unwrap();
                let cells = (2560.0 * 1440.0 / (px * px)) as u64;
                let load = cells as f32 / 1e6;
                rungs.push(Rung {
                    size: Size::px(px),
                    cells,
                    cpu_percent: 0.1 + 2.0 * load,
                    gpu_percent: Some(0.8 + 3.0 * load),
                    gpu_unmeasured: false,
                    p95_ms: 1.5 + 3.0 * load,
                    fps: 30.0,
                    on_gpu: cells >= 16_000,
                    fits: px >= 2.0 || k == 0,
                    over: (px < 2.0 && k == 1)
                        .then(|| "processor 8.4% (over 8.0%: the 10% limit, less 20%)".into()),
                    crop: Some(file),
                    own: px == own,
                });
            }
            let rec = if k == 0 { 1.0 } else { 2.0 };
            scenes.push(TunedScene {
                scene: name.into(),
                tiling: "square".into(),
                design_px: own,
                cell_px: if k == 0 { 2.0 } else { rec },
                chosen: k == 0,
                recommended: Size::px(rec),
                rungs,
                gpu_timing: GpuTiming::Kernel,
                ..Default::default()
            });
        }
        s.screens[0].tuning = Some(Tuning {
            rotation: s.screens[0].rotation.clone(),
            canvas: [2560, 1440],
            scale: 1.0,
            fps: 30.0,
            scenes,
            ..Default::default()
        });
        s.save().unwrap();
    }
    h.app.empty_world();
    h.app
        .display
        .host
        .test_extension(jetcarp_display::gnome::ExtensionState::Active);
    h.action(menu::Action::Pane("desktop"));
    h.column(0.62, 0.94);
    h.frame_with(0.05, vec![]);
    let pane = h.find("Show on the desktop").center();
    let bottom = h.o.size()[1] as f32 - 30.0;
    h.scroll_to("Fit to this computer…", pane, bottom);
    // Up to the Performance group's top, the cards below it.
    for _ in 0..40 {
        if h.o.find("Performance").is_some_and(|r| r.min.y < 60.0) {
            break;
        }
        h.scroll(pane, 60.0);
    }
    // The pointer off the pane, so no hover text covers it.
    let [w, ht] = h.o.size();
    h.hover(egui::pos2(w as f32 - 2.0, ht as f32 - 2.0));
    h.find("Recommended");
    h.steady();
    h.shot("desktop-pane-cards");
}

/// Issue #41: rotations from the Desktop background pane. With a scene open in the app (the one the
/// Windows test had, JACA rainbow fronts on Cairo pentagons), the pane before and after *Show just
/// this scene*: the scene saved as a profile and as a rotation of just that profile, the screen
/// running it, the screensaver's choice untouched; pressed again, nothing more is made. Then the
/// editor opened from the pane, with Save and Close above it and `kites` typed into its Add Quick
/// Start box (#74), and a new rotation saved there, chosen for the screen alone. Pictures:
/// `desktop-just-*.png`; `desktop-just-editor.png` is the book's
/// `guides/rotation-editor-desktop-pane.webp` (`guides/rotations.md`).
#[test]
#[ignore]
fn shots_desktop_show_just_this_scene() {
    use jetcarp_display::host_settings::HostSettings;
    use jetcarp_display::rotation::RotationStore;
    let Some(mut h) = Harness::new(1400, 900) else {
        return;
    };
    let q = jetcarp_core::quickstart::jaca_presets()
        .into_iter()
        .find(|q| q.scene.name == "JACA rainbow fronts (Cairo)")
        .expect("the JACA preset");
    h.app.load_scene_bg(&q.scene);
    h.built();
    h.run(2.0, 0.05, |_| false);
    let name = h.app.scene_name.clone();
    assert_eq!(name, "JACA rainbow fronts (Cairo)");
    let saver = h.app.shell.display.screensaver.clone();
    h.app
        .display
        .host
        .test_extension(jetcarp_display::gnome::ExtensionState::NotInstalled);
    h.action(menu::Action::Pane("desktop"));
    h.column(0.4, 0.94);
    h.frame_with(0.05, vec![]);
    let pane = egui::Rect::from_min_max(
        egui::pos2(0.0, 0.0),
        egui::pos2(0.4 * h.o.size()[0] as f32 + 8.0, h.o.size()[1] as f32),
    );
    h.find("Show just this scene");
    h.steady();
    h.shot_part("desktop-just-before", pane);
    // Its explanation, as the pointer rests on it.
    h.hover_on("Show just this scene");
    h.run(1.5, 0.05, |_| false);
    h.shot_part("desktop-just-hover", pane);

    // One press: a profile and a rotation of just it, chosen for the (only) screen.
    h.click("Show just this scene");
    h.run(0.5, 0.05, |_| false);
    let rotations = RotationStore::dir_for_settings(h.app.shell.store.path()).unwrap();
    let file = rotations.join("jaca-rainbow-fronts-cairo.json");
    assert!(file.exists(), "{} was written", file.display());
    let r = h.app.rotation_store().get(&name).expect("the rotation");
    assert_eq!(r.just_profile(), Some(name.as_str()));
    assert!(h.app.profile_store().listed(&name).unwrap().file.is_some());
    assert_eq!(HostSettings::load().0.screens[0].rotation, name, "on disk");
    assert_eq!(
        h.app.shell.display.screensaver, saver,
        "the screensaver's own"
    );
    h.steady();
    h.shot_part("desktop-just-after", pane);
    h.shot("desktop-just-after-window");

    // Again: the same profile and rotation, updated, not "2".
    let files = std::fs::read_dir(&rotations).unwrap().count();
    h.click("Show just this scene");
    h.run(0.5, 0.05, |_| false);
    assert_eq!(std::fs::read_dir(&rotations).unwrap().count(), files);
    let twos = format!("{name} 2");
    assert!(!h.app.rotation_store().names().contains(&twos));
    assert!(!h.app.profile_store().names().contains(&twos));
    assert_eq!(h.app.shell.display.screensaver, saver);

    // Edit, beside it: the editor opens under the screen, Save and Close at its top.
    let row = h.find("Show just this scene");
    let on_row = |h: &Harness, text: &str| {
        h.o.widgets
            .iter()
            .filter(|w| w.0 == text && w.1.min.y < row.max.y && w.1.max.y > row.min.y)
            .map(|w| w.1)
            .next()
            .unwrap_or_else(|| panic!("no {text:?} beside Show just this scene"))
    };
    let edit = on_row(&h, "Edit");
    h.click_at(edit.center(), egui::PointerButton::Primary);
    assert_eq!(h.app.display.host_rotation_ui.host_screen(), Some(0));
    // The top one of each pair of buttons.
    let top = |h: &Harness, text: &str| {
        h.o.widgets
            .iter()
            .filter(|w| w.0 == text)
            .map(|w| w.1)
            .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
            .unwrap_or_else(|| panic!("no {text:?}"))
    };
    assert!(
        top(&h, "Save").max.y < top(&h, "Name").min.y,
        "Save above the name"
    );
    // The Add Quick Start box searched (#74): the entries matching what is typed, to click.
    h.click_right_of("Add series");
    h.frame_with(0.05, vec![egui::Event::Text("kites".into())]);
    h.run(0.5, 0.05, |_| false);
    h.find("Brian's Brain on Penrose kites and darts (P2)");
    // The pointer off the pane, so no hover text covers the list.
    let [w, ht] = h.o.size();
    h.hover(egui::pos2(w as f32 - 2.0, ht as f32 - 2.0));
    h.steady();
    h.shot_part("desktop-just-editor", pane);
    let close = top(&h, "Close");
    h.click_at(close.center(), egui::PointerButton::Primary);
    assert_eq!(h.app.display.host_rotation_ui.host_screen(), None);

    // New, then Save at the top: chosen for the screen, the screensaver's choice as it was.
    let new = on_row(&h, "New");
    h.click_at(new.center(), egui::PointerButton::Primary);
    assert_eq!(h.app.display.host_rotation_ui.host_screen(), Some(0));
    let save = top(&h, "Save");
    h.click_at(save.center(), egui::PointerButton::Primary);
    h.run(0.5, 0.05, |_| false);
    let chosen = HostSettings::load().0.screens[0].rotation.clone();
    assert!(chosen.starts_with("My rotation"), "chose {chosen:?}");
    assert!(
        h.app
            .rotation_store()
            .listed(&chosen)
            .unwrap()
            .file
            .is_some()
    );
    assert_eq!(
        h.app.shell.display.screensaver, saver,
        "still the screensaver's own"
    );
    h.steady();
    h.shot_part("desktop-just-new-saved", pane);

    // The Screensaver pane's editor, Save at its top too: saving there chooses the rotation for
    // the screensaver and leaves the screen's as it is.
    h.action(menu::Action::Pane("screensaver"));
    h.app.shell.display.screensaver.source = super::display::SaverSource::Rotation;
    h.app.shell.display.screensaver.rotation = name.clone();
    h.frame_with(0.05, vec![]);
    let below_menus = |h: &Harness, text: &str| {
        h.o.widgets
            .iter()
            .filter(|w| w.0 == text && w.1.min.y > 30.0)
            .map(|w| w.1)
            .min_by(|a, b| a.min.y.total_cmp(&b.min.y))
            .unwrap_or_else(|| panic!("no {text:?}"))
    };
    let edit = below_menus(&h, "Edit");
    h.click_at(edit.center(), egui::PointerButton::Primary);
    assert!(
        top(&h, "Save").max.y < top(&h, "Name").min.y,
        "Save above the name"
    );
    h.find("Save and start");
    h.steady();
    h.shot_part("saver-rotation-editor", pane);
    let save = top(&h, "Save");
    h.click_at(save.center(), egui::PointerButton::Primary);
    h.run(0.5, 0.05, |_| false);
    let s = &h.app.shell.display.screensaver;
    assert_eq!(
        (s.rotation.as_str(), s.source),
        (name.as_str(), super::display::SaverSource::Rotation)
    );
    assert_eq!(
        HostSettings::load().0.screens[0].rotation,
        chosen,
        "the screen's as it was"
    );
}

/// Tutorial 2 (`tutorials/life-on-a-new-tiling.md`): a new empty world, the 2-uniform tiling
/// k2-007 from the tiling catalog, Life from *CA → Add from catalog*, run; the Objects pane's
/// census with the ships highlighted. Prints the world's hash (the tutorial's command line
/// reaches the same).
#[test]
#[ignore]
fn shots_new_tiling() {
    let Some(mut h) = Harness::new(1400, 900) else {
        return;
    };
    h.quick_start_loaded();
    h.action(menu::Action::NewEmpty);
    h.built();
    // The tiling catalog, in the Tiling pane.
    h.action(menu::Action::Pane("tiling"));
    h.column(0.30, 0.62);
    h.frame_with(0.05, vec![]);
    h.click("Tiling catalog");
    h.click_right_of("any");
    h.frame_with(0.05, vec![egui::Event::Text("k2-007".into())]);
    h.run(0.5, 0.05, |_| false);
    h.click("k2-007  ");
    h.built();
    // About 10,000 cells, so the world is the same on any screen.
    h.click("Fill the view");
    h.click("About 10,000");
    h.built();
    h.run(0.5, 0.05, |_| false);
    eprintln!("tiling: {}", h.app.world.topo.describe());
    // Zoomed in, so the tiles show.
    h.app.zoom_by(5.0);
    h.shot("life-tiling-catalog");
    h.app.fit_pending = true;
    // CA → Add from catalog → Life-like → Life.
    h.hold_menu("CA");
    h.hover_on("Add from catalog");
    let n = catalog().iter().filter(|e| e.family == "Life-like").count();
    h.hover_on(&format!("Life-like ({n})"));
    // The list scrolls: Life a few lines from its top.
    let first = h.find("2x2  B36/S125");
    let life = h.find("Life  B3/S23");
    h.scroll(
        first.center(),
        life.min.y - first.min.y - 6.0 * life.height(),
    );
    h.hover_on("Life  B3/S23");
    let life = h.find("Life  B3/S23");
    let right =
        h.o.widgets
            .iter()
            .filter(|w| (w.1.min.x - life.min.x).abs() < 1.0)
            .map(|w| w.1.max.x)
            .fold(life.max.x, f32::max);
    let menu = Harness::around(
        &[
            h.find("CA"),
            h.find("Add from catalog"),
            h.find("Layered (2)"),
        ],
        10.0,
    );
    let part = egui::Rect::from_min_max(menu.min, egui::pos2(right + 40.0, menu.max.y));
    h.shot_part("life-add-from-catalog", part);
    h.click("Life  B3/S23");
    h.app.shell.open_menu = None;
    h.frame_with(0.05, vec![]);
    eprintln!(
        "added: gen {} hash {} cas {:?}",
        h.app.world.generation,
        h.app.world.hash_hex(),
        h.app
            .world
            .cas
            .iter()
            .map(|c| c.notation())
            .collect::<Vec<_>>()
    );
    eprintln!("scene: {}", h.app.world.to_scene().to_json());
    let gens: u64 = std::env::var("JETCARP_SHOTS_GENS")
        .ok()
        .and_then(|g| g.parse().ok())
        .unwrap_or(500);
    h.app.playing = false;
    h.app.exec(Command::Step { gens });
    eprintln!(
        "gen {} hash {}",
        h.app.world.generation,
        h.app.world.hash_hex()
    );
    h.action(menu::Action::Pane("objects"));
    h.click("Find objects");
    h.run(1.0, 0.05, |_| false);
    // The ships' row, highlighting them in the grid.
    let ships =
        h.o.widgets
            .iter()
            .find(|w| w.0.contains(" ship "))
            .map(|w| w.1)
            .expect("a ship in the census");
    h.click_at(ships.center(), egui::PointerButton::Primary);
    h.steady();
    h.shot("life-objects");
    // Edge neighbours, from the same start (Edit → Reset), as far.
    h.click("Edge");
    h.action(menu::Action::Reset);
    h.app.playing = false;
    h.app.exec(Command::Step { gens });
    eprintln!(
        "edge: gen {} hash {}",
        h.app.world.generation,
        h.app.world.hash_hex()
    );
    h.click("Find objects");
    h.steady();
    h.shot("life-objects-edge");
}

/// Tutorial 4 (`tutorials/building-quick-start.md`): the Quick Start screen showing only the
/// atlas a build published into this library (`jetcarp quickstart build --minutes 3`, in the
/// book's picture).
#[test]
#[ignore]
fn shots_quick_start_atlas() {
    let Some(mut h) = Harness::new(1400, 900) else {
        return;
    };
    h.quick_start_loaded();
    h.app.menu.view.filter.source = Some(jetcarp_store::QsSource::Atlas);
    h.run(8.0, 0.05, |_| false);
    h.steady();
    h.shot("quickstart-atlas");
}

/// Issue #88: Run → Jump's Generations field takes a click and typing, the menu staying open (a
/// click inside it closed the menu, so only the default 1,000 could be jumped). No picture.
#[test]
#[ignore]
fn jump_field_takes_typing() {
    let Some(mut h) = Harness::new(1200, 800) else {
        return;
    };
    h.app.empty_world();
    for _ in 0..3 {
        h.frame_with(0.05, vec![]);
    }
    h.click("Run");
    h.hover_on("Jump");
    h.find("Generations");
    h.click_right_of("Generations");
    // Select what is there, then type a new number over it.
    let ctrl_a = |pressed| egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    };
    h.frame_with(0.05, vec![ctrl_a(true), ctrl_a(false)]);
    h.frame_with(0.05, vec![egui::Event::Text("2^20".into())]);
    for _ in 0..3 {
        h.frame_with(0.05, vec![]);
    }
    assert!(
        h.o.find("Jump ahead (HashLife)").is_some(),
        "the Jump menu closed when its field was clicked"
    );
    assert_eq!(h.app.jump_gens, "2^20");
}

/// Issue #90: a rule typed in a CA's Rule box, the box still focused (no Enter, no click
/// elsewhere), is applied by Save's first step, so the scene saved has the rule the box shows.
#[test]
#[ignore]
fn a_rule_being_typed_applies_before_save() {
    let Some(mut h) = Harness::new(1400, 900) else {
        return;
    };
    h.app.empty_world();
    h.app.exec(Command::AddCa {
        ca: CaSpec::new("B3/S23").seed(SeedSpec::Random { density: 0.3 }),
    });
    for _ in 0..4 {
        h.frame_with(0.05, vec![]);
    }
    h.click_right_of("Rule");
    let ctrl_a = |pressed| egui::Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    };
    h.frame_with(0.05, vec![ctrl_a(true), ctrl_a(false)]);
    h.frame_with(0.05, vec![egui::Event::Text("B36/S23".into())]);
    h.frame_with(0.05, vec![]);
    assert_eq!(
        h.app.world.cas[0].notation(),
        "B3/S23",
        "applied before Save"
    );
    let app = &mut h.app;
    h.o.frame_with(0.05, vec![], |ctx| app.commit_rule_edit(ctx));
    assert_eq!(h.app.world.cas[0].notation(), "B36/S23");
}

/// A Margolus Tron field: every 2 by 2 block of the empty background is complemented each
/// generation, so the whole field flips black and white at the scene's speed (#97's test scene).
const TRON_FIELD: &str = r#"{"name": "Tron flip", "tiling": {"family": "square", "size": {"rect": {"w": 120, "h": 90}}, "boundary": "wrap"}, "rng_state": 1, "cas": [{"rule": "Margolus Tron", "seed": "empty"}]}"#;

/// Not a picture: the app's view through the strobe guard (#148). A field flipping every
/// generation, run at 60 generations a second in a window drawn in real time at about 60 frames a
/// second: the guard softens it, then slows it, and what the window shows (measured frame by frame
/// on the processor, the whole window) never flashes more than three times a second.
/// `JETCARP_STROBE_RECORD=DIR` also writes every frame there as a PNG, for IRIS.
#[test]
#[ignore]
fn strobe_guard_keeps_the_app_view_under_the_limit() {
    use jetcarp_core::strobe::Viewing;
    use jetcarp_core::strobe::flash::FlashMeter;
    use jetcarp_core::strobe::frame::reduce;
    let Some(mut h) = Harness::new(640, 480) else {
        return;
    };
    assert!(h.app.guard_on_here(), "the guard is on by default");
    let scene: Scene = serde_json::from_str(TRON_FIELD).unwrap();
    let world = World::from_scene(&scene).unwrap();
    h.app.show_world(world, "Tron flip".into(), String::new());
    h.app.speed = 60.0;
    h.app.max_speed = false;
    h.app.playing = true;
    let record = std::env::var_os("JETCARP_STROBE_RECORD").map(PathBuf::from);
    if let Some(d) = &record {
        std::fs::create_dir_all(d).unwrap();
    }
    let [w, hh] = h.o.size();
    let mut meter = FlashMeter::new(Viewing::screen(1.0), None);
    let (mut worst, mut slowed) = (0, false);
    let start = Instant::now();
    let mut k = 0u32;
    while start.elapsed().as_secs_f64() < 12.0 {
        let t = start.elapsed().as_secs_f64();
        let px = h.frame_with(1.0 / 60.0, vec![]);
        let r = meter.update(&reduce(&px, w, hh), t);
        worst = worst.max(r.most());
        if let Some(d) = &record {
            h.o.save(&px, &d.join(format!("app-{k:05}.png"))).unwrap();
        }
        k += 1;
        slowed |= h.app.strobe.cap.is_some();
        // About 60 frames a second of real time.
        let next = Duration::from_secs_f64(k as f64 / 60.0);
        if let Some(wait) = next.checked_sub(start.elapsed()) {
            std::thread::sleep(wait);
        }
    }
    eprintln!(
        "{k} frames; most transitions in a second on screen: {worst}; report: {:?}",
        h.app.strobe.report
    );
    assert!(worst <= jetcarp_core::strobe::MAX_TRANSITIONS, "{worst}");
    assert!(slowed, "the guard slowed the field");
    assert!(
        h.app.guarded_speed().0 <= 3.0,
        "{:?}",
        h.app.guarded_speed()
    );
}

/// Settings → Safety (`guides/strobe-guard.md`, #148): its two switches, and the confirmation that
/// turns one off. *Turn it off* does nothing until its box is ticked; the desktop's switch stays on
/// when the app's goes off; for someone at risk there is no switch at all.
#[test]
#[ignore]
fn shots_safety() {
    use jetcarp_core::photosensitivity::{self as ps, GuardPlace};
    let Some(mut h) = Harness::new(900, 640) else {
        return;
    };
    let settings = jetcarp_core::paths::config_dir().expect("a scratch settings folder");
    ps::record_in(&settings, ps::Answer::NoDisorder, "test").unwrap();
    h.app.strobe.reread();
    h.action(menu::Action::Tool(menu::ToolWin::Safety));
    h.run(0.5, 0.05, |_| false);
    h.shot("safety-1-window");
    h.click("Turn off the strobe guard…");
    h.shot("safety-2-confirm");
    h.click(ps::GUARD_TURN_OFF);
    assert!(
        ps::guard_on_in(&settings, GuardPlace::App),
        "not without the box ticked"
    );
    h.click(ps::GUARD_OFF_CHECK);
    h.click(ps::GUARD_TURN_OFF);
    assert!(!ps::guard_on_in(&settings, GuardPlace::App), "turned off");
    assert!(
        ps::guard_on_in(&settings, GuardPlace::Desktop),
        "the desktop's stays on"
    );
    assert!(!h.app.guard_on_here());
    h.click("Turn the strobe guard back on");
    assert!(ps::guard_on_in(&settings, GuardPlace::App));
    // The desktop's: its confirmation has the sentence about other people in the room.
    h.click("Turn off the strobe guard for the desktop…");
    h.find(ps::GUARD_OFF_ROOM);
    h.shot("safety-3-confirm-desktop");
    h.click(ps::GUARD_KEEP);
    assert!(ps::guard_on_in(&settings, GuardPlace::Desktop), "kept on");
    assert!(
        h.o.find(ps::GUARD_OFF_ROOM).is_none(),
        "and the confirmation closed"
    );
    // Someone at risk: always on, no switch.
    ps::record_in(&settings, ps::Answer::AtRisk, "test").unwrap();
    h.app.strobe.reread();
    h.run(0.3, 0.05, |_| false);
    assert!(h.o.find("Turn off the strobe guard…").is_none());
    assert!(
        h.o.find("Turn off the strobe guard for the desktop…")
            .is_none()
    );
    h.find(ps::GUARD_ALWAYS);
    ps::record_in(&settings, ps::Answer::NoDisorder, "test").unwrap();
    h.app.strobe.reread();
}

/// Tools → This computer… (`guides/this-computer.md`, #213), on an example 32 GB machine (never
/// the one running the test), not measured yet: its limits, and one of your own. Then a world
/// over the machine's limit: asked about, not built on *Cancel*, built on *Build anyway*.
#[test]
#[ignore]
fn shots_this_computer() {
    use jetcarp_core::machine::{self, GpuMemory, Hardware, Id, Limits, Own};
    let Some(mut h) = Harness::new(1000, 760) else {
        return;
    };
    let settings = jetcarp_core::paths::config_dir().expect("a scratch settings folder");
    let _ = std::fs::remove_file(settings.join("limits.json"));
    let _ = std::fs::remove_file(settings.join("machine.json"));
    h.action(menu::Action::Tool(menu::ToolWin::Machine));
    h.run(0.5, 0.05, |_| false);
    // An example machine, so the picture shows no real one.
    let example = Hardware {
        os: "linux".into(),
        arch: "x86_64".into(),
        cpu: "An 8-core processor".into(),
        threads: 16,
        cores: Some(8),
        memory_total: 32 << 30,
        memory_available: Some(21 << 30),
        gpus: vec![GpuMemory {
            vendor: 0x1002,
            device: 0x73df,
            dedicated: Some(12 << 30),
            shared: Some(16 << 30),
            unified: false,
            source: "sysfs".into(),
            name: String::new(),
        }],
    };
    let set_example = |h: &mut Harness, own: &Own| {
        let l = Limits::compute(&example, own);
        let mut info = h
            .app
            .machine
            .info
            .clone()
            .expect("the window read the machine");
        info.hardware = example.clone();
        info.adapters = vec![jetcarp_api::ctx::GpuAdapter {
            name: "An example GPU".into(),
            backend: "Vulkan".into(),
            kind: "discrete".into(),
            ..Default::default()
        }];
        info.limits.entries = l.entries;
        info.limits.memory_total = l.memory_total;
        h.app.machine.show_info(info);
    };
    let own = Limits::compute(&example, &Own::default())
        .set(&Own::default(), Id::ReservedSystem, (12u64 << 30) as f64)
        .unwrap();
    set_example(&mut h, &own);
    h.run(0.3, 0.05, |_| false);
    h.find("Measure this computer");
    h.find("Reserved for the System");
    h.find("Back to the suggestion");
    h.shot("this-computer-1-window");
    // A world over the limit: the reference machine's 4 GB here, lowered to 100 MB for the test.
    h.app.set_tool_open(menu::ToolWin::Machine, false);
    h.quick_start_loaded();
    h.open_entry("penrose-rhomb-brain");
    let l = Limits::reference();
    let small = l
        .set(&Own::default(), Id::World, (100u64 << 20) as f64)
        .unwrap();
    small.save(&settings).unwrap();
    machine::reload();
    let mut big = jetcarp_core::TilingSpec::new("square");
    big.size = Some(jetcarp_core::SizeSpec::Rect { w: 4000, h: 4000 });
    let before = h.app.world.topo.len();
    h.app.set_tiling_bg(big.clone());
    h.run(0.3, 0.05, |_| false);
    assert!(h.app.machine.ask.is_some(), "asked first");
    h.find("Build anyway");
    h.shot("this-computer-2-over-limit");
    h.click("Cancel");
    assert!(h.app.machine.ask.is_none());
    assert_eq!(h.app.world.topo.len(), before, "not built on Cancel");
    h.app.set_tiling_bg(big);
    h.run(0.3, 0.05, |_| false);
    h.click("Build anyway");
    h.built();
    assert_eq!(h.app.world.topo.len(), 16_000_000, "built when asked");
    Own::default().save(&settings).unwrap();
    machine::reload();
}

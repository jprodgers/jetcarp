//! The whole app drawn headlessly, for the book's pictures of it (tests only). `JetcarpApp` runs
//! as eframe would run it, on [`Offscreen`]'s device: its grid is drawn by its own paint
//! callback, and widgets are clicked by their text (AccessKit's tree says where they are). The
//! `#[ignore]`d tests write PNGs to `JETCARP_OFFSCREEN_OUT`; every folder must be a scratch one
//! (`JETCARP_CONFIG_DIR`, `JETCARP_DATA_DIR`, `JETCARP_LIBRARY`, `JETCARP_OUT`,
//! `JETCARP_RUNTIME_DIR`). Run them one at a time:
//! `cargo test -p jetcarp-app --release shots -- --ignored --test-threads 1`.

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
    /// window closed. `None` without `JETCARP_OFFSCREEN_OUT` or a GPU.
    fn new(w: u32, h: u32) -> Option<Harness> {
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
        let Some(o) = Offscreen::new(w, h) else {
            eprintln!("no GPU adapter");
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

    /// One frame, `dt` seconds after the last, with these input events.
    fn frame_with(&mut self, dt: f64, events: Vec<egui::Event>) -> Vec<u8> {
        let (app, frame) = (&mut self.app, &mut self.frame);
        self.o.frame_with(dt, events, |ctx| {
            ctx.root_ui(|ui| eframe::App::ui(app, ui, frame))
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

/// Tutorial 1 (`tutorials/first-wallpaper.md`): a Quick Start entry opened, its palette list
/// open in the CAs pane; then the Screensaver pane, with the world saved as a profile and the
/// Background host section open (its rotation is the host settings', *My wallpaper* in the book's
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
    h.app
        .display
        .host
        .test_extension(jetcarp_display::gnome::ExtensionState::NotInstalled);
    h.click("Background host");
    h.steady();
    h.shot("wallpaper-screensaver-pane");
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

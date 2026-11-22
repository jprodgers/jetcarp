//! The app from the keyboard alone and through a screen reader's eyes (#219), tested on the whole
//! app with no window and no GPU: key presses go in as egui events, and what comes out is read
//! from AccessKit's tree, the same tree Narrator, NVDA, VoiceOver and Orca are given.
//!
//! The app reads and writes its settings, library and output folders, so each test runs in a
//! process of its own with every folder a scratch one ([`in_scratch`]): the test the suite runs
//! starts its `#[ignore]`d twin (`child_…`) with those folders set, and fails when it fails. The
//! twin does nothing when started any other way. The twins test the interface, not the GPU, so
//! they run with it switched off, and each ends its process at once when it has passed ([`done`]),
//! as the app does at exit: a graphics driver's thread still running while the process is torn
//! down can crash it, which would fail a test that had passed.

use super::*;
use eframe::egui::accesskit;
use egui::{Event, Key, Modifiers};
use std::collections::{HashMap, HashSet};

/// Set in a twin's process by [`in_scratch`].
const CHILD: &str = "JETCARP_KEYBOARD_TEST_CHILD";

/// Run the `#[ignore]`d test `name` (`app::keyboard_tests::child_…`) in a process of its own,
/// every folder JETCARP uses a scratch one; panic with its output when it fails.
pub(super) fn in_scratch(name: &str) {
    let short = name.rsplit("::").next().unwrap_or(name);
    let root = jetcarp_core::scratch::ScratchDir::new(&format!("kb-{short}"));
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args([
        name,
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ]);
    for (var, sub) in [
        ("JETCARP_CONFIG_DIR", "config"),
        ("JETCARP_DATA_DIR", "data"),
        ("JETCARP_LIBRARY", "data/library.sqlite"),
        ("JETCARP_OUT", "out"),
        ("JETCARP_RUNTIME_DIR", "run"),
    ] {
        cmd.env(var, root.join(sub));
    }
    cmd.env(CHILD, "1");
    cmd.env("JETCARP_GPU", "off");
    for v in [
        "JETCARP_OFFSCREEN_OUT",
        "JETCARP_SCREENSHOT",
        "JETCARP_MENU",
        "JETCARP_REQUIRE_GPU",
    ] {
        cmd.env_remove(v);
    }
    let out = cmd.output().expect("start the test's own process");
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success() && text.contains(PASSED),
        "{name} failed in its own process ({}):\n{text}",
        out.status
    );
}

/// What a twin prints when it has passed.
const PASSED: &str = "keyboard test passed";

/// The end of a twin that passed: say so, and end the process at once.
pub(super) fn done() -> ! {
    println!("{PASSED}");
    jetcarp_display::exit::end_process(0)
}

/// In a twin: whether it was started by [`in_scratch`] (else it returns at once, touching no
/// folder of the person running it).
pub(super) fn child() -> bool {
    std::env::var_os(CHILD).is_some()
}

/// A node of AccessKit's tree, as the last frame gave it.
#[derive(Clone, Debug)]
pub(super) struct Node {
    pub role: accesskit::Role,
    pub label: Option<String>,
    pub value: Option<String>,
    pub numeric: Option<f64>,
    pub toggled: Option<accesskit::Toggled>,
    pub placeholder: Option<String>,
    pub labelled_by: Vec<accesskit::NodeId>,
    pub live: accesskit::Live,
    pub focusable: bool,
    pub clickable: bool,
    pub bounds: Option<egui::Rect>,
    pub active_descendant: Option<accesskit::NodeId>,
}

impl Node {
    pub(super) fn of(n: &accesskit::Node) -> Node {
        Node {
            role: n.role(),
            label: n.label().map(String::from),
            value: n.value().map(String::from),
            numeric: n.numeric_value(),
            toggled: n.toggled(),
            placeholder: n.placeholder().map(String::from),
            labelled_by: n.labelled_by().to_vec(),
            live: n.live().unwrap_or(accesskit::Live::Off),
            focusable: n.supports_action(accesskit::Action::Focus),
            clickable: n.supports_action(accesskit::Action::Click),
            bounds: n.bounds().map(|b| {
                egui::Rect::from_min_max(
                    egui::pos2(b.x0 as f32, b.y0 as f32),
                    egui::pos2(b.x1 as f32, b.y1 as f32),
                )
            }),
            active_descendant: n.active_descendant(),
        }
    }
}

/// The whole app, headless.
pub(super) struct Kb {
    pub ctx: egui::Context,
    pub app: JetcarpApp,
    frame: eframe::Frame,
    time: f64,
    size: egui::Vec2,
    /// The last frame's AccessKit tree, by node, and the node with the keyboard.
    pub nodes: HashMap<accesskit::NodeId, Node>,
    pub focus: Option<accesskit::NodeId>,
    /// The last frame's shapes (the focus ring is looked for among them).
    pub shapes: Vec<egui::epaint::ClippedShape>,
}

impl Kb {
    /// The app as at a first launch, its warning answered and setup closed, in a window of
    /// 1280 × 800 points.
    pub fn new() -> Kb {
        Kb::sized(1280.0, 800.0)
    }

    pub fn sized(w: f32, h: f32) -> Kb {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = JetcarpApp::new(&cc, crate::cli::LaunchArgs::default());
        app.shell.store = settings::Store::none();
        app.warning.showing = false;
        app.pending_launch = None;
        app.setup.open = false;
        app.shell.layout = panes::Layout::default();
        app.a11y.settings = Default::default();
        for t in menu::ToolWin::ALL {
            app.set_tool_open(t, false);
        }
        let mut k = Kb {
            ctx,
            app,
            frame: eframe::Frame::_new_kittest(),
            time: 0.0,
            size: egui::vec2(w, h),
            nodes: HashMap::new(),
            focus: None,
            shapes: vec![],
        };
        k.frames(3);
        k
    }

    /// The app with a world open: Life on squares, paused, its panes docked as at first.
    pub fn with_life() -> Kb {
        let mut k = Kb::new();
        let scene = Scene::from_json(
            r#"{"name": "Keyboard test", "tiling": {"family": "square", "size": {"rect": {"w": 40, "h": 30}}, "boundary": "wrap"}, "rng_state": 3, "cas": [{"rule": "B3/S23", "seed": "empty"}]}"#,
        )
        .unwrap();
        k.app.load_scene(&scene);
        k.app.playing = false;
        k.app.message = None;
        k.frames(4);
        k
    }

    /// One frame with these events.
    pub fn frame_with(&mut self, events: Vec<Event>) {
        self.time += 1.0 / 30.0;
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        let (app, frame) = (&mut self.app, &mut self.frame);
        let mut out = self.ctx.run_pass(input, |ctx| {
            ctx.root_ui(|ui| {
                eframe::App::logic(app, ui.ctx(), frame);
                eframe::App::ui(app, ui, frame);
            });
        });
        if let Some(u) = out.platform_output.accesskit_update.take() {
            self.nodes = u.nodes.iter().map(|(id, n)| (*id, Node::of(n))).collect();
            self.focus = Some(u.focus);
        }
        self.shapes = std::mem::take(&mut out.shapes);
        out.textures_delta.clear();
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame_with(vec![]);
        }
    }

    /// A key pressed with these modifiers, and let go.
    pub fn key_with(&mut self, key: Key, modifiers: Modifiers) {
        let ev = |pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        self.frame_with(vec![ev(true)]);
        self.frame_with(vec![ev(false)]);
    }

    pub fn key(&mut self, key: Key) {
        self.key_with(key, Modifiers::NONE);
    }

    pub fn typed(&mut self, text: &str) {
        self.frame_with(vec![Event::Text(text.into())]);
    }

    /// The widget with the keyboard now.
    pub fn focused(&self) -> Option<egui::Id> {
        self.ctx.memory(|m| m.focused())
    }

    /// The node a screen reader is on: the focused widget's, or its active descendant (as
    /// AccessKit's platform adapters take it).
    pub fn focused_node(&self) -> Option<&Node> {
        let n = self.focus.and_then(|f| self.nodes.get(&f))?;
        Some(
            n.active_descendant
                .and_then(|d| self.nodes.get(&d))
                .unwrap_or(n),
        )
    }

    /// What a screen reader calls a node: its own label, the labels it is labelled by, or a text
    /// field's placeholder.
    pub fn name(&self, n: &Node) -> String {
        name_in(&self.nodes, n)
    }

    /// The widgets that take the keyboard this frame, in the order they were drawn, with their
    /// layer and rectangle: those the Tab key should reach.
    pub fn focusable(&self) -> Vec<(egui::Id, egui::LayerId, egui::Rect)> {
        let ctx = &self.ctx;
        let mut seen = HashSet::new();
        let mut v = vec![];
        let drawn: HashSet<egui::LayerId> =
            ctx.viewport(|vp| vp.prev_pass.widgets.layer_ids().collect());
        // Back to front.
        let layers: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().collect());
        for layer in layers.into_iter().filter(|l| drawn.contains(l)) {
            if !ctx.memory(|m| m.allows_interaction(layer)) {
                continue;
            }
            let ws: Vec<_> = ctx.viewport(|vp| {
                vp.prev_pass
                    .widgets
                    .get_layer(layer)
                    .filter(|w| w.sense.is_focusable() && w.enabled && w.visible)
                    .map(|w| (w.id, w.rect))
                    .collect()
            });
            for (id, rect) in ws {
                if seen.insert(id) {
                    v.push((id, layer, rect));
                }
            }
        }
        v
    }
}

/// What a screen reader calls node `n` of tree `nodes`.
pub(super) fn name_in(nodes: &HashMap<accesskit::NodeId, Node>, n: &Node) -> String {
    if let Some(l) = n.label.as_ref().filter(|l| !l.trim().is_empty()) {
        return l.clone();
    }
    let by: Vec<String> = n
        .labelled_by
        .iter()
        .filter_map(|id| nodes.get(id))
        .filter_map(|m| m.label.clone().or(m.value.clone()))
        .collect();
    if !by.is_empty() {
        return by.join(" ");
    }
    n.placeholder.clone().unwrap_or_default()
}

/// The last frame's tree, by node.
pub(super) fn tree_of(
    nodes: &[(accesskit::NodeId, accesskit::Node)],
) -> HashMap<accesskit::NodeId, Node> {
    nodes.iter().map(|(id, n)| (*id, Node::of(n))).collect()
}

/// What a screen reader would find wrong with the tree: interactive nodes with no name, a name
/// that says nothing (a symbol alone), no role, or a value that does not read.
pub(super) fn audit(k: &Kb) -> Vec<String> {
    audit_tree(&k.nodes)
}

/// [`audit`] of a tree.
pub(super) fn audit_tree(tree: &HashMap<accesskit::NodeId, Node>) -> Vec<String> {
    use accesskit::Role;
    let mut problems = vec![];
    let says = |s: &str| s.chars().any(|c| c.is_alphanumeric());
    let mut nodes: Vec<&Node> = tree.values().collect();
    nodes.sort_by(|a, b| {
        let p = |n: &Node| n.bounds.map(|r| (r.min.y, r.min.x)).unwrap_or((0.0, 0.0));
        p(a).partial_cmp(&p(b)).unwrap()
    });
    for n in nodes {
        let interactive = n.focusable || n.clickable || egui::accessibility::is_input(n.role);
        if !interactive {
            continue;
        }
        let at = n
            .bounds
            .map(|r| format!("at ({:.0}, {:.0})", r.min.x, r.min.y))
            .unwrap_or_default();
        if matches!(n.role, Role::Unknown | Role::GenericContainer) {
            problems.push(format!("no role: a focusable node {at}"));
            continue;
        }
        if !egui::accessibility::is_input(n.role) {
            continue;
        }
        let name = name_in(tree, n);
        // `JETCARP_A11Y_DUMP=1`: every control as a screen reader would name it, for reading by eye.
        if std::env::var_os("JETCARP_A11Y_DUMP").is_some() {
            eprintln!(
                "  {:?} {name:?}{} {at}",
                n.role,
                n.value
                    .as_ref()
                    .map(|v| format!(" = {v:?}"))
                    .unwrap_or_default()
            );
        }
        if name.trim().is_empty() {
            problems.push(format!(
                "no name: a {:?} {at}{}",
                n.role,
                n.value
                    .as_ref()
                    .map(|v| format!(" showing {v:?}"))
                    .unwrap_or_default()
            ));
        } else if !says(&name) {
            problems.push(format!(
                "a name that says nothing: {:?} {name:?} {at}",
                n.role
            ));
        }
        match n.role {
            Role::Slider | Role::SpinButton if n.numeric.is_none() && n.value.is_none() => {
                problems.push(format!("no value: {:?} {name:?} {at}", n.role))
            }
            Role::CheckBox | Role::Switch | Role::RadioButton if n.toggled.is_none() => {
                problems.push(format!("no state: {:?} {name:?} {at}", n.role))
            }
            Role::ComboBox | Role::TextInput | Role::MultilineTextInput | Role::SearchInput
                if n.value.is_none() =>
            {
                problems.push(format!("no value: {:?} {name:?} {at}", n.role))
            }
            _ => {}
        }
    }
    problems
}

/// Tab from where the keyboard is now, `max` times or until focus comes back to where it started;
/// the widgets reached, in order.
fn tab_cycle(k: &mut Kb, max: usize, back: bool) -> Vec<egui::Id> {
    let mods = if back {
        Modifiers::SHIFT
    } else {
        Modifiers::NONE
    };
    let mut seen = vec![];
    for _ in 0..max {
        k.key_with(Key::Tab, mods);
        k.frame_with(vec![]);
        let Some(f) = k.focused() else {
            continue;
        };
        if seen.first() == Some(&f) {
            break;
        }
        seen.push(f);
    }
    seen
}

// ------------------------------------------------------------------------------------------
// The tests the suite runs, each starting its twin

#[test]
fn tab_reaches_every_control_of_the_main_window() {
    in_scratch("app::keyboard_tests::child_tab_reaches_every_control_of_the_main_window");
}

/// With a world open, Tab goes through every control the window draws, the menu bar's, the
/// panes', the grid and the status bar's, and comes round again; Shift+Tab goes back the same way.
/// Tab used to hide the panes instead.
#[test]
#[ignore]
fn child_tab_reaches_every_control_of_the_main_window() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    k.app.message = Some(("A note in the status line.".into(), false));
    k.frames(2);
    assert!(!k.app.shell.panes_hidden);
    // Every control but the few widgets only the mouse uses: the lines between docked panes, the
    // tab bars' dragged backgrounds.
    let chrome = k.app.kb_chrome(&k.ctx);
    assert!(
        (2..=6).contains(&chrome.len()),
        "mouse-only widgets: {}",
        chrome.len()
    );
    let want: Vec<egui::Id> = k
        .focusable()
        .into_iter()
        .map(|w| w.0)
        .filter(|id| !chrome.contains(id))
        .collect();
    assert!(want.len() > 40, "only {} focusable widgets", want.len());
    let got = tab_cycle(&mut k, 600, false);
    assert!(!k.app.shell.panes_hidden, "Tab hid the panes");
    assert!(
        got.iter().all(|id| !chrome.contains(id)),
        "Tab stopped on a mouse-only widget"
    );
    let missed: Vec<_> = want.iter().filter(|w| !got.contains(w)).collect();
    assert!(
        missed.is_empty(),
        "Tab never reached {} of {} widgets: {:?}",
        missed.len(),
        want.len(),
        missed
            .iter()
            .filter_map(|id| k.nodes.get(&id.accesskit_id()).map(|n| k.name(n)))
            .collect::<Vec<_>>()
    );
    let back = tab_cycle(&mut k, 600, true);
    assert_eq!(back.len(), got.len(), "Shift+Tab reached as many");
    done();
}

impl Kb {
    /// The focusable widget whose node a screen reader names `name` (the first drawn).
    pub fn widget_named(&self, name: &str) -> Option<egui::Id> {
        self.focusable().into_iter().map(|w| w.0).find(|id| {
            self.nodes
                .get(&id.accesskit_id())
                .is_some_and(|n| self.name(n) == name)
        })
    }

    /// Give the keyboard to the widget named `name`, as Tab would.
    pub fn focus_named(&mut self, name: &str) {
        let id = self
            .widget_named(name)
            .unwrap_or_else(|| panic!("no focusable {name:?}: {:?}", self.names()));
        self.ctx.memory_mut(|m| m.request_focus(id));
        self.frames(2);
    }

    /// Every focusable widget's name, for messages.
    pub fn names(&self) -> Vec<String> {
        self.focusable()
            .into_iter()
            .filter_map(|w| self.nodes.get(&w.0.accesskit_id()).map(|n| self.name(n)))
            .collect()
    }

    /// The name of the widget with the keyboard.
    pub fn focused_name(&self) -> String {
        self.focused()
            .and_then(|id| self.nodes.get(&id.accesskit_id()))
            .map(|n| self.name(n))
            .unwrap_or_default()
    }

    /// Where the focused widget was drawn last frame, and the part of it not clipped away.
    pub fn focused_rects(&self) -> Option<(egui::Rect, egui::Rect)> {
        let id = self.focused()?;
        self.ctx.viewport(|vp| {
            vp.prev_pass
                .widgets
                .get(id)
                .map(|w| (w.rect, w.interact_rect))
        })
    }

    /// A ring of at least `width` points drawn round `rect` (the focus ring) in the last frame.
    pub fn ring_round(&self, rect: egui::Rect, width: f32) -> bool {
        use egui::epaint::Shape;
        fn any(s: &Shape, rect: egui::Rect, width: f32) -> bool {
            match s {
                Shape::Rect(r) => {
                    r.stroke.width >= width
                        && r.fill == egui::Color32::TRANSPARENT
                        && r.rect
                            .expand(r.stroke.width * 2.0 + 1.0)
                            .contains_rect(rect)
                        && rect
                            .expand(r.stroke.width * 2.0 + 6.0)
                            .contains_rect(r.rect)
                }
                Shape::Vec(v) => v.iter().any(|s| any(s, rect, width)),
                _ => false,
            }
        }
        self.shapes.iter().any(|c| any(&c.shape, rect, width))
    }

    /// The nodes in the tree that are live regions (the status line's announcements).
    pub fn live(&self) -> Vec<&Node> {
        self.nodes
            .values()
            .filter(|n| n.live != accesskit::Live::Off)
            .collect()
    }

    /// The focusable widget with the largest area: the grid.
    pub fn grid_widget(&self) -> egui::Id {
        self.focusable()
            .into_iter()
            .max_by(|a, b| a.2.area().total_cmp(&b.2.area()))
            .map(|w| w.0)
            .unwrap()
    }

    /// The first text field drawn.
    pub fn first_text_field(&self) -> egui::Id {
        self.focusable()
            .into_iter()
            .map(|w| w.0)
            .find(|id| {
                self.nodes
                    .get(&id.accesskit_id())
                    .is_some_and(|n| n.role == accesskit::Role::TextInput)
            })
            .expect("a text field")
    }
}

#[test]
fn tab_reaches_every_control_of_each_tool_window_and_the_quick_start() {
    in_scratch(
        "app::keyboard_tests::child_tab_reaches_every_control_of_each_tool_window_and_the_quick_start",
    );
}

/// Each tool window opened alone over a world, the setup window and Export animation: from the
/// window's first control, Tab goes through every control it draws (its title bar's buttons
/// included, the title bar itself passed over) before it leaves the window. And on the Quick
/// Start, Tab reaches every control drawn: the buttons, the sidebar's filters, the search box and
/// the cards.
#[test]
#[ignore]
fn child_tab_reaches_every_control_of_each_tool_window_and_the_quick_start() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    let mut failures = vec![];
    let mut walk = |k: &mut Kb, what: &str| {
        k.frames(3);
        // The window is the layer drawn on top that is not the background.
        let ws = k.focusable();
        let Some(layer) = ws
            .iter()
            .map(|w| w.1)
            .rfind(|l| *l != egui::LayerId::background())
        else {
            failures.push(format!("{what}: no window drawn"));
            return;
        };
        let chrome = k.app.kb_chrome(&k.ctx);
        let want: Vec<egui::Id> = ws
            .iter()
            .filter(|w| w.1 == layer && !chrome.contains(&w.0))
            .map(|w| w.0)
            .collect();
        k.ctx.memory_mut(|m| m.request_focus(want[0]));
        k.frames(2);
        let mut got = vec![want[0]];
        for _ in 0..want.len() + 5 {
            k.key(Key::Tab);
            k.frames(1);
            let mut f = k.focused();
            if f.is_some() && f == got.last().copied() && k.ctx.text_edit_focused() {
                // A box that keeps Tab for typing (a code editor): Esc leaves it, and Tab goes on
                // from it.
                k.key(Key::Escape);
                k.key(Key::Tab);
                k.frames(1);
                f = k.focused();
            }
            match f {
                Some(f) if want.contains(&f) && !got.contains(&f) => got.push(f),
                _ => break,
            }
        }
        let missed: Vec<String> = want
            .iter()
            .filter(|w| !got.contains(w))
            .map(|id| {
                k.nodes
                    .get(&id.accesskit_id())
                    .map(|n| k.name(n))
                    .unwrap_or_default()
            })
            .collect();
        eprintln!("{what}: {} of {} controls reached", got.len(), want.len());
        if !missed.is_empty() {
            failures.push(format!("{what}: Tab never reached {missed:?}"));
        }
    };
    for t in menu::ToolWin::ALL {
        k.app.set_tool_open(t, true);
        walk(&mut k, t.label());
        k.app.set_tool_open(t, false);
        k.frames(1);
    }
    k.app.anim.open = true;
    walk(&mut k, "Export animation");
    k.app.anim.open = false;
    k.app.setup.show_again();
    walk(&mut k, "Setup");
    k.app.setup.open = false;
    k.frames(1);
    // The Quick Start: every control drawn at first, the cards in view among them.
    k.app.run_action(&k.ctx.clone(), menu::Action::QuickStart);
    k.app.menu.view.live = false;
    k.frames(10);
    let want: Vec<egui::Id> = k.focusable().into_iter().map(|w| w.0).collect();
    let got = tab_cycle(&mut k, 900, false);
    let missed = want.iter().filter(|w| !got.contains(w)).count();
    eprintln!(
        "Quick Start: {} controls at first, {} reached",
        want.len(),
        got.len()
    );
    if missed > 0 {
        failures.push(format!("Quick Start: Tab never reached {missed} controls"));
    }
    assert!(failures.is_empty(), "{failures:#?}");
    done();
}

#[test]
fn escape_closes_and_enter_confirms_in_dialogs_and_tool_windows() {
    in_scratch(
        "app::keyboard_tests::child_escape_closes_and_enter_confirms_in_dialogs_and_tool_windows",
    );
}

/// Export animation opens with its Export button holding the keyboard, so Enter exports and Esc
/// closes it; Esc on a control in a tool window closes the window, and Esc in a text field only
/// leaves the field.
#[test]
#[ignore]
fn child_escape_closes_and_enter_confirms_in_dialogs_and_tool_windows() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    // Export animation: Esc closes it.
    k.app.anim.open = true;
    k.frames(3);
    assert_eq!(
        k.focused_name(),
        "Export…",
        "the Export button has the keyboard"
    );
    k.key(Key::Escape);
    assert!(!k.app.anim.open, "Esc closed Export animation");
    // Enter presses Export…, which asks where to save (no dialog is shown in tests).
    k.app.anim.open = true;
    k.frames(3);
    assert!(!crate::file_dialog::asked("anim.export"));
    k.key(Key::Enter);
    k.frames(2);
    assert!(
        crate::file_dialog::asked("anim.export"),
        "Enter pressed Export…"
    );
    // A tool window: Esc on one of its controls closes it.
    k.app.set_tool_open(menu::ToolWin::Accessibility, true);
    k.frames(3);
    k.focus_named("Reduce motion");
    k.key(Key::Escape);
    k.frames(1);
    assert!(
        !k.app.tool_open(menu::ToolWin::Accessibility),
        "Esc closed the Accessibility window"
    );
    // Esc in a text field leaves the field, and the pane stays.
    k.app.run_action(&k.ctx.clone(), menu::Action::Pane("add"));
    k.frames(2);
    let field = k.first_text_field();
    k.ctx.memory_mut(|m| m.request_focus(field));
    k.frames(2);
    k.typed("B36");
    k.key(Key::Escape);
    assert_eq!(k.focused(), None, "Esc left the field");
    assert_eq!(k.app.shell.layout.place("add"), panes::Place::Docked);
    done();
}

#[test]
fn tabs_cards_and_the_setup_window_from_the_keyboard() {
    in_scratch("app::keyboard_tests::child_tabs_cards_and_the_setup_window_from_the_keyboard");
}

/// A pane's tab shows its pane with Enter; a Quick Start card's menu opens with Shift+F10 and
/// closes with Esc; the setup window opens with its Next button holding the keyboard, so Enter goes
/// on to the next page, and Esc closes it, as Later does.
#[test]
#[ignore]
fn child_tabs_cards_and_the_setup_window_from_the_keyboard() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    // The upper group shows the Scene pane; its Tiling tab (not the menu bar's Tiling), then
    // Enter.
    assert!(k.widget_named("Tiling family").is_none());
    let tab = k
        .focusable()
        .into_iter()
        .find(|w| {
            w.2.min.y > 20.0
                && k.nodes
                    .get(&w.0.accesskit_id())
                    .is_some_and(|n| k.name(n) == "Tiling")
        })
        .map(|w| w.0)
        .expect("the Tiling tab");
    k.ctx.memory_mut(|m| m.request_focus(tab));
    k.frames(2);
    k.key(Key::Enter);
    k.frames(2);
    assert!(
        k.widget_named("Tiling family").is_some(),
        "Enter on the Tiling tab showed the Tiling pane"
    );
    // The setup window (its status read on a thread first): Next has the keyboard; Enter goes
    // on; Esc closes it.
    k.app.setup.show_again();
    let t = std::time::Instant::now();
    while k.focused_name() != "Next" && t.elapsed().as_secs() < 20 {
        k.frames(1);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(k.focused_name(), "Next");
    k.key(Key::Enter);
    k.frames(3);
    assert!(
        k.widget_named("Later").is_some() && k.widget_named("Back").is_some(),
        "the second page: {:?}",
        k.names()
    );
    k.focus_named("Back");
    k.key(Key::Escape);
    k.frames(1);
    assert!(!k.app.setup.open, "Esc closed the setup window");
    // A Quick Start card's menu.
    k.app.run_action(&k.ctx.clone(), menu::Action::QuickStart);
    k.app.menu.view.live = false;
    // Until a card is drawn: ten frames are enough here, not on a slow CI runner.
    let t = std::time::Instant::now();
    let mut first = None;
    while first.is_none() && t.elapsed().as_secs() < 20 {
        k.frames(1);
        first = k.app.menu.test_first_card();
        if first.is_none() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    let (handle, _) = first.expect("a card drawn");
    let title = k
        .app
        .menu
        .items
        .iter()
        .find(|it| it.handle == handle)
        .unwrap()
        .title
        .clone();
    let card = k
        .focusable()
        .into_iter()
        .map(|w| w.0)
        .find(|id| {
            k.nodes
                .get(&id.accesskit_id())
                .is_some_and(|n| k.name(n).starts_with(&format!("{title}:")))
        })
        .expect("the card's button, named by its title");
    k.ctx.memory_mut(|m| m.request_focus(card));
    k.frames(2);
    k.key_with(Key::F10, Modifiers::SHIFT);
    k.frames(2);
    assert!(
        egui::Popup::is_any_open(&k.ctx),
        "Shift+F10 opened its menu"
    );
    assert!(k.widget_named("Hide from Quick Start").is_some());
    assert!(!k.app.shell.menu_autohide, "and did nothing else");
    k.key(Key::Escape);
    k.frames(1);
    assert!(!egui::Popup::is_any_open(&k.ctx), "Esc closed it");
    done();
}

#[test]
fn f10_opens_the_menus_and_the_arrows_and_enter_choose() {
    in_scratch("app::keyboard_tests::child_f10_opens_the_menus_and_the_arrows_and_enter_choose");
}

/// F10 puts the keyboard on the menu bar's File menu, open; → moves along the bar, opening each
/// menu in turn, ↓ goes into a menu, Enter chooses, and Esc closes the menu.
#[test]
#[ignore]
fn child_f10_opens_the_menus_and_the_arrows_and_enter_choose() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    let outlines = k.app.shell.cell_outlines;
    let hide = k.app.shell.menu_autohide;
    k.key(Key::F10);
    k.frames(1);
    assert_eq!(k.app.shell.menu_autohide, hide, "F10 left auto-hide alone");
    assert_eq!(k.focused_name(), "File");
    assert!(egui::Popup::is_any_open(&k.ctx), "the File menu is open");
    // → → to View, Enter opens it, ↓ to its fourth item, Cell outlines, Enter.
    k.key(Key::ArrowRight);
    k.key(Key::ArrowRight);
    assert_eq!(k.focused_name(), "View");
    assert!(egui::Popup::is_any_open(&k.ctx), "the View menu is open");
    for _ in 0..4 {
        k.key(Key::ArrowDown);
    }
    assert_eq!(k.focused_name(), "Cell outlines");
    k.key(Key::Enter);
    k.frames(1);
    assert_ne!(
        k.app.shell.cell_outlines, outlines,
        "Enter chose Cell outlines"
    );
    k.key(Key::Escape);
    k.frames(1);
    assert!(!egui::Popup::is_any_open(&k.ctx), "Esc closed the menu");
    done();
}

#[test]
fn f6_moves_between_the_menu_bar_panes_grid_status_line_and_windows() {
    in_scratch(
        "app::keyboard_tests::child_f6_moves_between_the_menu_bar_panes_grid_status_line_and_windows",
    );
}

/// F6 goes from the menu bar to each docked pane, the grid, the status line and each tool window
/// in turn, and round again; Shift+F6 goes back.
#[test]
#[ignore]
fn child_f6_moves_between_the_menu_bar_panes_grid_status_line_and_windows() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    k.app.message = Some(("A note in the status line.".into(), false));
    k.app.set_tool_open(menu::ToolWin::Accessibility, true);
    k.frames(3);
    let mut stops = vec![];
    for _ in 0..12 {
        k.key(Key::F6);
        k.frames(1);
        let f = k.focused().expect("F6 gave the keyboard to something");
        if stops.first() == Some(&f) {
            break;
        }
        stops.push(f);
    }
    let names: Vec<String> = stops
        .iter()
        .map(|id| {
            k.nodes
                .get(&id.accesskit_id())
                .map(|n| k.name(n))
                .unwrap_or_default()
        })
        .collect();
    eprintln!("F6 stops: {names:?}");
    assert_eq!(names.first().map(String::as_str), Some("File"), "{names:?}");
    let grid = k.grid_widget();
    assert!(stops.contains(&grid), "the grid is a stop: {names:?}");
    assert!(
        names.iter().any(|n| n == "Dismiss"),
        "the status line: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "Follow the system"),
        "the Accessibility window: {names:?}"
    );
    // Two tab groups of panes, the menu bar, the grid, the status line and the window.
    assert!(stops.len() >= 6, "{names:?}");
    // Shift+F6 goes back through the same stops.
    let mut back = vec![];
    for _ in 0..stops.len() {
        k.key_with(Key::F6, Modifiers::SHIFT);
        k.frames(1);
        back.push(k.focused().unwrap());
    }
    // From the first stop: the last, then each before it, back to the first.
    back.reverse();
    assert_eq!(back, stops, "Shift+F6 went back the same way");
    done();
}

#[test]
fn the_focused_control_is_scrolled_into_view_and_ringed() {
    in_scratch("app::keyboard_tests::child_the_focused_control_is_scrolled_into_view_and_ringed");
}

/// In a window as wide as the app opens but short, Tab through the long Desktop background pane:
/// each control it reaches is scrolled into view and has a ring round it, in the standard theme
/// and the high-contrast one.
#[test]
#[ignore]
fn child_the_focused_control_is_scrolled_into_view_and_ringed() {
    if !child() {
        return;
    }
    let mut k = Kb::sized(1280.0, 520.0);
    let scene = Scene::from_json(
        r#"{"tiling": {"family": "square", "size": {"rect": {"w": 20, "h": 20}}, "boundary": "wrap"}, "cas": [{"rule": "B3/S23", "seed": "empty"}]}"#,
    )
    .unwrap();
    k.app.load_scene(&scene);
    k.app.playing = false;
    k.app.message = None;
    k.app
        .run_action(&k.ctx.clone(), menu::Action::Pane("desktop"));
    k.frames(3);
    for high in [false, true] {
        k.app.a11y.settings.high_contrast = if high {
            crate::a11y::Follow::On
        } else {
            crate::a11y::Follow::Off
        };
        k.frames(2);
        // The pane's first control, then Tab through it.
        let pane = k
            .focusable()
            .into_iter()
            .filter(|w| w.2.min.x < 300.0 && w.2.min.y > 60.0)
            .map(|w| w.0)
            .next()
            .expect("a control in the upper pane group");
        k.ctx.memory_mut(|m| m.request_focus(pane));
        k.frames(2);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, k.size);
        let mut reached = 0;
        for _ in 0..40 {
            k.key(Key::Tab);
            k.frames(2);
            let Some((rect, shown)) = k.focused_rects() else {
                continue;
            };
            if rect.min.x > 320.0 {
                break;
            }
            reached += 1;
            // In view from top to bottom (a control wider than the pane is cut at its side,
            // which no scrolling mends).
            assert!(
                rect.min.y >= 0.0
                    && rect.max.y <= screen.max.y
                    && (shown.height() - rect.height()).abs() < 1.0
                    && shown.width() > 0.0,
                "{:?} at {rect:?} is out of view (shown {shown:?})",
                k.focused_name()
            );
            assert!(
                k.ring_round(rect, if high { 3.0 } else { 2.0 }),
                "no focus ring round {:?} at {rect:?}",
                k.focused_name()
            );
        }
        assert!(reached > 20, "only {reached} controls reached");
    }
    done();
}

#[test]
fn bare_keys_work_while_a_button_has_the_keyboard_but_not_while_typing() {
    in_scratch(
        "app::keyboard_tests::child_bare_keys_work_while_a_button_has_the_keyboard_but_not_while_typing",
    );
}

/// R, F, + and − act while a button or check box has the keyboard (Space, Enter, the arrows and
/// Tab are the button's); in a text field they are typing. F9 hides the panes and shows them.
#[test]
#[ignore]
fn child_bare_keys_work_while_a_button_has_the_keyboard_but_not_while_typing() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    k.app.exec(Command::Step { gens: 3 });
    k.frames(1);
    k.focus_named("File");
    let zoom = k.app.zoom;
    k.key(Key::Plus);
    assert!(
        k.app.zoom > zoom,
        "+ zoomed in while a menu button had the keyboard"
    );
    k.key(Key::R);
    assert_eq!(k.app.world.generation, 0, "R reset the world");
    assert_eq!(k.focused_name(), "File", "and the keyboard stayed");
    // In a text field the same keys are typing.
    k.app.run_action(&k.ctx.clone(), menu::Action::Pane("add"));
    k.frames(2);
    let field = k.first_text_field();
    k.ctx.memory_mut(|m| m.request_focus(field));
    k.frames(2);
    k.app.exec(Command::Step { gens: 2 });
    let zoom = k.app.zoom;
    k.frame_with(vec![
        Event::Key {
            key: Key::R,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
        Event::Text("r".into()),
    ]);
    k.frame_with(vec![Event::Text("+".into())]);
    k.frames(1);
    assert_eq!(k.app.world.generation, 2, "R was typed, not a reset");
    assert_eq!(k.app.zoom, zoom);
    // F9 hides the panes and brings them back; Tab never does.
    k.ctx.memory_mut(|m| m.surrender_focus(field));
    k.frames(1);
    k.key(Key::F9);
    assert!(k.app.shell.panes_hidden, "F9 hid the panes");
    k.key(Key::F9);
    assert!(!k.app.shell.panes_hidden);
    done();
}

#[test]
fn the_grid_s_cell_cursor_draws_erases_selects_and_moves() {
    in_scratch("app::keyboard_tests::child_the_grid_s_cell_cursor_draws_erases_selects_and_moves");
}

fn live_cells(k: &Kb) -> Vec<usize> {
    let f = &k.app.world.fields[0];
    (0..f.state.len()).filter(|&c| f.state[c] != 0).collect()
}

/// The grid takes the keyboard: the arrows move a cell cursor, which a screen reader is told
/// about; Space draws, Delete erases, Shift+arrows select, Space fills the selection, Ctrl+arrows
/// move what is selected, and Esc ends the selection, then leaves the grid.
#[test]
#[ignore]
fn child_the_grid_s_cell_cursor_draws_erases_selects_and_moves() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    let grid = k.grid_widget();
    k.ctx.memory_mut(|m| m.request_focus(grid));
    k.frames(2);
    let n = k.focused_node().expect("the grid's node").clone();
    let g = k.app.world.topo.lattice().unwrap().clone();
    assert!(
        matches!(n.role, accesskit::Role::GridCell),
        "the cursor's cell has the keyboard, for a screen reader: {n:?}"
    );
    let start = k.name(&n);
    assert!(start.starts_with("Cell "), "{start}");
    // Space draws, at the cell in the view's middle.
    k.key(Key::Space);
    let lit = live_cells(&k);
    assert_eq!(lit.len(), 1, "Space drew one cell");
    assert!(!k.app.playing, "Space drew, it did not run the world");
    let c0 = lit[0];
    let (u0, j0, _) = g.decompose(c0);
    // → moves one cell; the screen reader hears the new cell.
    k.key(Key::ArrowRight);
    let now = k.focused_node().map(|n| k.name(n)).unwrap_or_default();
    assert_ne!(now, start, "the cursor's new cell is announced");
    k.key(Key::Space);
    assert_eq!(live_cells(&k), vec![c0, g.index(u0 + 1, j0, 0)]);
    // Delete erases.
    k.key(Key::Delete);
    assert_eq!(live_cells(&k), vec![c0]);
    // Shift+↓ ↓ → selects a 2 by 3 block from here; Space fills it.
    k.key_with(Key::ArrowDown, Modifiers::SHIFT);
    k.key_with(Key::ArrowDown, Modifiers::SHIFT);
    k.key_with(Key::ArrowRight, Modifiers::SHIFT);
    k.key(Key::Space);
    let block: Vec<usize> = (0..3)
        .flat_map(|dj| (1..3).map(move |du| (du, dj)))
        .map(|(du, dj)| g.index(u0 + du, j0 + dj, 0))
        .collect();
    let mut want = block.clone();
    want.push(c0);
    want.sort();
    assert_eq!(live_cells(&k), want, "Space filled the selection");
    // Ctrl+← moves the selected block one cell left, onto c0's column.
    k.key_with(Key::ArrowLeft, Modifiers::COMMAND);
    let mut moved: Vec<usize> = block
        .iter()
        .map(|&c| {
            let (u, j, _) = g.decompose(c);
            g.index(u - 1, j, 0)
        })
        .collect();
    moved.sort();
    assert_eq!(live_cells(&k), moved, "Ctrl+← moved the block");
    // Delete erases the selection; Esc ends it, and a second Esc leaves the grid.
    k.key(Key::Delete);
    assert!(live_cells(&k).is_empty());
    k.key(Key::Escape);
    assert!(
        k.focused().is_some(),
        "the first Esc only ended the selection"
    );
    k.key(Key::Escape);
    assert_eq!(k.focused(), None, "the second left the grid");
    done();
}

#[test]
fn every_window_names_its_controls_for_a_screen_reader() {
    in_scratch("app::keyboard_tests::child_every_window_names_its_controls_for_a_screen_reader");
}

/// Every window, opened on its own over a world (and the Quick Start, the menus, the setup window
/// and the photosensitive seizure warning): each control has a role and a name that says
/// something, and each value reads.
#[test]
#[ignore]
fn child_every_window_names_its_controls_for_a_screen_reader() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    let mut all = vec![];
    let mut check = |k: &Kb, what: &str| {
        for p in audit(k) {
            all.push(format!("{what}: {p}"));
        }
    };
    // The main window, each pane shown in turn.
    for p in panes::PANES {
        k.app.run_action(&k.ctx.clone(), menu::Action::Pane(p.id));
        k.frames(3);
        check(&k, &format!("{} pane", p.title));
    }
    // Each menu, held open.
    for (title, _) in menu::MENUS {
        k.app.shell.open_menu = Some(title.to_string());
        k.frames(3);
        check(&k, &format!("{title} menu"));
    }
    k.app.shell.open_menu = None;
    k.key(Key::Escape);
    // Each tool window, alone.
    for t in menu::ToolWin::ALL {
        k.app.set_tool_open(t, true);
        k.frames(4);
        check(&k, t.label());
        k.app.set_tool_open(t, false);
        k.frames(1);
    }
    k.app.anim.open = true;
    k.frames(3);
    check(&k, "Export animation");
    k.app.anim.open = false;
    // The setup window and the Quick Start.
    k.app.setup.show_again();
    k.frames(4);
    check(&k, "Setup");
    k.app.setup.open = false;
    k.app.run_action(&k.ctx.clone(), menu::Action::QuickStart);
    k.frames(8);
    check(&k, "Quick Start");
    // The photosensitive seizure warning.
    k.app.warning.showing = true;
    k.frames(3);
    check(&k, "Photosensitive seizure warning");
    for p in &all {
        eprintln!("{p}");
    }
    assert!(
        all.is_empty(),
        "{} problems a screen reader would meet",
        all.len()
    );
    done();
}

#[test]
fn changes_are_announced_to_a_screen_reader() {
    in_scratch("app::keyboard_tests::child_changes_are_announced_to_a_screen_reader");
}

/// The status line is a live region: a scene paused by the strobe guard, a finished job and a
/// world that could not be opened are read out as they happen, the error at once.
#[test]
#[ignore]
fn child_changes_are_announced_to_a_screen_reader() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    assert!(k.live().is_empty(), "nothing to say yet");
    // The strobe guard pauses the scene.
    k.app.playing = true;
    {
        let mut out = k.app.strobe.out.lock().unwrap();
        let now = k.app.started.elapsed().as_secs_f64();
        out.views.insert(
            view::TILE_VIEW,
            crate::gpu::ViewOut {
                at: now,
                paused: Some("paused: it flashed".into()),
                notes: vec![
                    "paused this scene: it flashes faster than three times a second".into(),
                ],
                ..Default::default()
            },
        );
    }
    k.frames(2);
    assert!(!k.app.playing, "the guard paused it");
    let live = k.live();
    assert_eq!(live.len(), 1, "one live region");
    let said = k.name(live[0]);
    assert!(said.contains("Strobe guard: Paused this scene"), "{said}");
    assert_eq!(live[0].live, accesskit::Live::Polite);
    // A job that finished, from a tool window.
    crate::a11y::announce(&k.ctx, "Hunt 3 finished: 2 finds.");
    k.frames(2);
    assert_eq!(k.name(k.live()[0]), "Hunt 3 finished: 2 finds.");
    // A scene that could not be opened: said at once.
    let bad: Scene = serde_json::from_str(r#"{"tiling": {"family": "no-such-tiling"}}"#).unwrap();
    k.app.load_scene(&bad);
    k.frames(2);
    let live = k.live();
    assert_eq!(
        live[0].live,
        accesskit::Live::Assertive,
        "{:?}",
        k.name(live[0])
    );
    done();
}

#[test]
fn screen_reader_names_are_the_same_in_every_font() {
    in_scratch("app::keyboard_tests::child_screen_reader_names_are_the_same_in_every_font");
}

/// What a screen reader is told doesn't depend on the font: every pane, menu and tool window gives
/// the same roles and names in Atkinson Hyperlegible (the default) as in egui's original fonts.
#[test]
#[ignore]
fn child_screen_reader_names_are_the_same_in_every_font() {
    use crate::fonts::FontChoice;
    if !child() {
        return;
    }
    // Every (role, name) a screen reader meets in each part of the app, in this font.
    let names = |font: FontChoice| -> (Vec<(String, Vec<String>)>, String) {
        let mut k = Kb::with_life();
        k.app.a11y.settings.font = font.clone();
        k.app.a11y.settings.mono_font = font;
        k.frames(3);
        let mut seen = vec![];
        let mut take = |k: &Kb, what: &str| {
            let mut v: Vec<String> = k
                .nodes
                .values()
                .map(|n| format!("{:?} {:?}", n.role, k.name(n)))
                .collect();
            v.sort();
            seen.push((what.to_string(), v));
        };
        for p in panes::PANES {
            k.app.run_action(&k.ctx.clone(), menu::Action::Pane(p.id));
            k.frames(3);
            take(&k, p.title);
        }
        for (title, _) in menu::MENUS {
            k.app.shell.open_menu = Some(title.to_string());
            k.frames(3);
            take(&k, title);
        }
        k.app.shell.open_menu = None;
        k.key(Key::Escape);
        for t in menu::ToolWin::ALL {
            k.app.set_tool_open(t, true);
            k.frames(4);
            take(&k, t.label());
            k.app.set_tool_open(t, false);
            k.frames(1);
        }
        // (Not the Quick Start: its cards are chosen at random each time.)
        let first = k
            .ctx
            .fonts(|f| f.definitions().families[&egui::FontFamily::Proportional][0].clone());
        (seen, first)
    };
    let (atkinson, a_font) = names(FontChoice::Atkinson);
    let (original, o_font) = names(FontChoice::Egui);
    // The two runs drew in different fonts.
    assert_eq!(a_font, crate::fonts::NEXT);
    assert_eq!(o_font, "Ubuntu-Light");
    let mut differ = vec![];
    for ((what, a), (_, o)) in atkinson.iter().zip(&original) {
        let only_a: Vec<&String> = a.iter().filter(|n| !o.contains(n)).collect();
        let only_o: Vec<&String> = o.iter().filter(|n| !a.contains(n)).collect();
        if !only_a.is_empty() || !only_o.is_empty() {
            differ.push(format!(
                "{what}: only in Atkinson {only_a:?}; only in egui's {only_o:?}"
            ));
        }
    }
    assert!(differ.is_empty(), "{}", differ.join("\n"));
    assert!(atkinson.iter().all(|(_, v)| !v.is_empty()));
    done();
}

/// The status line keeps one line's height whatever it holds (#291): a message too long for the
/// window is cut short, not wrapped onto a second line, so nothing above the bar moves as its
/// contents come and go. (Brian's Brain on Penrose made it grow and shrink in a loop, moving the
/// whole window.)
#[test]
fn the_status_line_stays_one_line() {
    in_scratch("app::keyboard_tests::child_the_status_line_stays_one_line");
}

#[test]
#[ignore = "started by the_status_line_stays_one_line, in a scratch process"]
fn child_the_status_line_stays_one_line() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    k.size = egui::vec2(640.0, 480.0);
    k.frames(3);
    let long = "A message long enough to run past the edge of a narrow window, and on. ".repeat(4);
    let mut grids = vec![];
    for i in 0..12 {
        let text = if i % 2 == 0 {
            long.clone()
        } else {
            "Short".into()
        };
        k.app.message = Some((text, false));
        k.frames(2);
        grids.push(
            k.focusable()
                .into_iter()
                .map(|w| w.2)
                .max_by(|a, b| a.area().total_cmp(&b.area()))
                .unwrap(),
        );
    }
    assert!(
        grids.windows(2).all(|w| w[0] == w[1]),
        "the grid moved as the status line's message changed: {grids:?}"
    );
    done();
}

/// Steps obey the strobe guard as a run does (#293): with the guard's cap in place, steps held
/// down come no faster than the cap; while it holds a pause, no faster than its slowest pace; a
/// calm scene steps at any rate, and the steps' rate is the pace the guard is told. (Holding S on a
/// flashing scene stepped it at the key's repeat rate, past the cap and the pause, and now and then
/// a flash reached the screen.)
#[test]
fn steps_obey_the_strobe_guard() {
    in_scratch("app::keyboard_tests::child_steps_obey_the_strobe_guard");
}

#[test]
#[ignore = "started by steps_obey_the_strobe_guard, in a scratch process"]
fn child_steps_obey_the_strobe_guard() {
    if !child() {
        return;
    }
    let mut k = Kb::with_life();
    // What the grid's guard said this frame, stood in (the test has no GPU to measure with).
    fn guard_says(k: &mut Kb, cap: Option<f32>, paused: Option<&str>) {
        let now = k.app.started.elapsed().as_secs_f64();
        let mut out = k.app.strobe.out.lock().unwrap();
        let v = out.views.entry(super::view::TILE_VIEW).or_default();
        v.at = now;
        v.cap = cap;
        v.paused = paused.map(String::from);
    }
    // 30 presses of S, a frame each, as a key held down; the generations they made, and how long
    // they took.
    fn hold_s(k: &mut Kb, cap: Option<f32>, paused: Option<&str>) -> (u64, f64) {
        let (g0, t0) = (k.app.world.generation, std::time::Instant::now());
        for _ in 0..30 {
            guard_says(k, cap, paused);
            k.key(Key::S);
        }
        (k.app.world.generation - g0, t0.elapsed().as_secs_f64())
    }
    let (made, secs) = hold_s(&mut k, None, None);
    assert_eq!(made, 30, "a calm scene steps at any rate");
    assert!(
        k.app.strobe.reached.is_some_and(|r| r >= 1.0),
        "the steps' rate is the pace the guard is told: {:?}",
        k.app.strobe.reached
    );
    let _ = secs;
    let cap = 2.0;
    let (made, secs) = hold_s(&mut k, Some(cap), None);
    let most = 1 + (secs * cap as f64).floor() as u64;
    assert!(
        made <= most,
        "with a cap of {cap} a second, {made} steps in {secs:.2} s (at most {most})"
    );
    let pace = jetcarp_core::strobe::guard::MIN_PACE;
    let (made, secs) = hold_s(&mut k, None, Some("test"));
    let most = 1 + (secs * pace as f64).floor() as u64;
    assert!(
        made <= most,
        "while the guard holds a pause, {made} steps in {secs:.2} s (at most {most})"
    );
    done();
}

/// Tab reaches the script editor's code box (#295). A box that senses dragging (to select text),
/// with its scroll area's bars drawn inside it, was taken for a dragged background and passed over
/// by Tab and F6 alike. The tool-window walk above never met it: it opens *Scripts and sweeps…* on
/// its Sweep tab, and leaves out what it takes for mouse-only.
#[test]
fn tab_reaches_the_script_editor_s_code_box() {
    in_scratch("app::keyboard_tests::child_tab_reaches_the_script_editor_s_code_box");
}

#[test]
#[ignore = "started by tab_reaches_the_script_editor_s_code_box, in a scratch process"]
fn child_tab_reaches_the_script_editor_s_code_box() {
    if !child() {
        return;
    }
    // A script long enough to scroll, so the scroll bars are drawn.
    let long = crate::scripts_ui::EXAMPLES
        .iter()
        .map(|e| e.1)
        .max_by_key(|s| s.len())
        .unwrap();
    let mut failures = vec![];
    for (w, h) in [(1280.0, 800.0), (1000.0, 700.0), (800.0, 600.0)] {
        let mut k = Kb::with_life();
        k.size = egui::vec2(w, h);
        k.app
            .scripts
            .set_text_for_test(&format!("{long}\n{long}\n{long}"));
        k.app.scripts.tab = crate::scripts_ui::Tab::Script;
        k.app.set_tool_open(menu::ToolWin::Scripts, true);
        k.frames(4);
        let ws = k.focusable();
        let (code, layer) = ws
            .iter()
            .find(|w| {
                k.nodes
                    .get(&w.0.accesskit_id())
                    .is_some_and(|n| n.role == accesskit::Role::MultilineTextInput)
            })
            .map(|w| (w.0, w.1))
            .expect("the code box is drawn");
        let chrome = k.app.kb_chrome(&k.ctx);
        if chrome.contains(&code) {
            failures.push(format!(
                "{w}x{h}: the code box is taken for a mouse-only widget"
            ));
        }
        let first = ws
            .iter()
            .find(|w| w.1 == layer && !chrome.contains(&w.0))
            .unwrap()
            .0;
        k.ctx.memory_mut(|m| m.request_focus(first));
        k.frames(2);
        let mut reached = k.focused() == Some(code);
        for _ in 0..ws.len() + 5 {
            if reached {
                break;
            }
            k.key(Key::Tab);
            k.frames(1);
            reached = k.focused() == Some(code);
        }
        if !reached {
            failures.push(format!("{w}x{h}: Tab never reached the code box"));
            continue;
        }
        // The ring shows round the part of the box in view, not round the whole scrolled text.
        k.frames(2);
        let (_, visible) = k.focused_rects().unwrap();
        if !k.ring_round(visible, 2.0) {
            failures.push(format!(
                "{w}x{h}: no focus ring round the code box in view ({visible:?})"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    done();
}

#[test]
fn measuring_pauses_the_world_and_resumes_it_after() {
    in_scratch("app::keyboard_tests::child_measuring_pauses_the_world_and_resumes_it_after");
}

/// *Tools → This computer…* asks the person to close other heavy programs before measuring, and
/// measuring pauses the world running in the app, which runs again once it has ended (#301).
#[test]
#[ignore]
fn child_measuring_pauses_the_world_and_resumes_it_after() {
    if !child() {
        return;
    }
    // The test machine is busy with other tests: the load is taken as quiet here, so the window
    // measures at once rather than waiting for the processor to go quiet.
    jetcarp_core::machine::quiet::sample_with(|_| jetcarp_core::machine::quiet::Load {
        cpu: Some(0.0),
        gpu: None,
    });
    let mut k = Kb::with_life();
    k.app.set_tool_open(menu::ToolWin::Machine, true);
    k.app.playing = true;
    k.frames(4);
    let warned = |k: &Kb| {
        k.nodes.values().any(|n| {
            [&n.label, &n.value]
                .into_iter()
                .flatten()
                .any(|t| t.contains("Close other heavy programs first"))
        })
    };
    assert!(
        warned(&k),
        "no warning about heavy programs before measuring"
    );
    k.app.machine.start_quick();
    let t0 = std::time::Instant::now();
    let mut paused = false;
    while k.app.machine.running() {
        assert!(
            t0.elapsed() < std::time::Duration::from_secs(300),
            "the measuring did not end"
        );
        k.frames(1);
        paused |= !k.app.playing;
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(paused, "the world kept running while JETCARP measured");
    // The hold is looked at about twice a second.
    for _ in 0..40 {
        k.frames(1);
        if k.app.playing {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(
        k.app.playing,
        "the world did not run again after the measuring"
    );
    done();
}

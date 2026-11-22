//! The app from the keyboard alone (#219), and what a screen reader is told.
//!
//! - **Tab and Shift+Tab** move between controls, in the order they are drawn: the menu bar, the
//!   panes, the grid, the status line, then the floating windows. Widgets only the mouse uses (a
//!   window's title bar, the lines between docked panes) are passed over. A control the keyboard
//!   reaches is scrolled into view and ringed ([`crate::a11y::paint_focus_ring`]).
//! - **F6 and Shift+F6** move between the window's parts: the menu bar, each group of panes, the
//!   grid, the status line and each floating window, to the first control of each.
//! - **F10** opens the File menu with the keyboard on it; ← and → move along the menu bar.
//! - **The grid** takes the keyboard like any control. Its arrow keys move a cell cursor that
//!   draws, erases, selects and moves patterns (`cell_cursor.rs`), shown only while the grid has
//!   the keyboard; a screen reader hears the cursor's cell as the grid's active descendant.
//! - **The status line** is a live region: what it says is read out as it changes.
//!
//! The menus' shortcuts are in `menu.rs`'s table; the keys here are in [`MOVING`] and [`GRID`],
//! which the Keyboard shortcuts window and the book's keyboard page list with them.

use super::*;
use crate::a11y;
use crate::cell_cursor::{self, Dir, Shift};
use egui::{Key, Modifiers};

/// One key's line in the Keyboard shortcuts window and the book: the keys on Windows and Linux,
/// on macOS, and what they do.
pub(crate) struct KeyLine {
    pub keys: &'static str,
    pub mac: &'static str,
    pub what: &'static str,
}

/// Moving about the app.
pub(crate) const MOVING: &[KeyLine] = &[
    KeyLine {
        keys: "Tab, Shift+Tab",
        mac: "Tab, ⇧Tab",
        what: "The next control, the one before",
    },
    KeyLine {
        keys: "F6, Shift+F6",
        mac: "F6, ⇧F6",
        what: "The next part of the window: the menu bar, each group of panes, the grid, the \
               status line, each floating window; the part before",
    },
    KeyLine {
        keys: "F10",
        mac: "F10",
        what: "The menu bar: opens File. ← and → move along it, ↓ goes into a menu, ← comes back \
               out, Enter chooses, Esc closes",
    },
    KeyLine {
        keys: "Space, Enter",
        mac: "Space, Return",
        what: "Press the button, tick the box, choose the item, open the list that has the keyboard",
    },
    KeyLine {
        keys: "Arrow keys",
        mac: "Arrow keys",
        what: "Between controls; in a menu or list, between its items; on a slider, its value",
    },
    KeyLine {
        keys: "Esc",
        mac: "Esc",
        what: "Leave a text box; close a menu, a list, a dialog, or the tool window that has \
               the keyboard (from a text box, Esc twice); in a docked pane, back to the grid",
    },
    KeyLine {
        keys: "Ctrl+W",
        mac: "⌘W",
        what: "Close the tool window or dialog that has the keyboard (a popped-out one goes \
               back to the main window)",
    },
    KeyLine {
        keys: "Shift+F10",
        mac: "⇧F10",
        what: "A Quick Start card's menu: rate it, hide it",
    },
    KeyLine {
        keys: "Alt (held)",
        mac: "⌥ (held)",
        what: "Show the auto-hidden menu bar",
    },
];

/// The grid's cell cursor, while the grid has the keyboard.
pub(crate) const GRID: &[KeyLine] = &[
    KeyLine {
        keys: "Arrow keys",
        mac: "Arrow keys",
        what: "Move the cell cursor to the next cell that way",
    },
    KeyLine {
        keys: "Shift+arrow keys",
        mac: "⇧ and arrow keys",
        what: "Select: the cells between where you began and the cursor",
    },
    KeyLine {
        keys: "Space",
        mac: "Space",
        what: "Draw the cursor's cell, or every selected cell, in the Draw pane's state; with a \
               library pattern waiting, paste it there",
    },
    KeyLine {
        keys: "Delete, Backspace",
        mac: "⌦, ⌫",
        what: "Erase the cursor's cell or the selection",
    },
    KeyLine {
        keys: "Ctrl+arrow keys",
        mac: "⌘ and arrow keys",
        what: "Move the selected pattern (or the cursor's cell) one step that way",
    },
    KeyLine {
        keys: "Esc",
        mac: "Esc",
        what: "End the selection; again, leave the grid, so Space runs and pauses",
    },
];

/// What the keyboard needs between frames.
#[derive(Default)]
pub(super) struct KeyboardRt {
    pub cursor: cell_cursor::Cursor,
    /// The grid's widget this frame.
    pub grid_id: Option<egui::Id>,
    /// The widget with the keyboard at the end of last frame.
    last_focus: Option<egui::Id>,
    /// The widget with the keyboard at the start of last frame (Tab moves it during a frame, so
    /// the next frame scrolls to it).
    begin_focus: Option<egui::Id>,
    /// The last Tab went backwards (Shift+Tab).
    tab_back: bool,
    /// Tab landed on a mouse-only widget: move on past it.
    skip: Option<egui::FocusDirection>,
    /// When the pointer last pressed a button (focus it gives is not scrolled to).
    pointer_at: f64,
    /// The text box Esc just left: Tab carries on from it rather than from the window's start.
    escaped_from: Option<egui::Id>,
    /// F10 asked for the menu bar.
    pub menu_request: bool,
    /// The menu bar's buttons and their menus, as drawn last frame.
    pub menu_buttons: Vec<(egui::Id, egui::Id)>,
    /// The keyboard is on the menu bar or in a menu (an auto-hidden bar shows while it is).
    pub on_menus: bool,
}

impl JetcarpApp {
    /// At the start of a frame, before the shortcuts: announcements into the status line, F6, F10,
    /// the menu bar's arrows, and a control the keyboard reached scrolled into view.
    pub(super) fn keyboard_begin(&mut self, ctx: &egui::Context) {
        a11y::regions_begin(ctx);
        for t in a11y::take_announcements(ctx) {
            self.message = Some((t, false));
        }
        let now = ctx.input(|i| i.time);
        if ctx.input(|i| i.pointer.any_pressed() || i.pointer.any_released()) {
            self.kb.pointer_at = now;
        }
        if let Some(d) = self.kb.skip.take() {
            ctx.memory_mut(|m| m.move_focus(d));
        }
        let tab = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key: Key::Tab,
                    pressed: true,
                    modifiers,
                    ..
                } => Some(modifiers.shift),
                _ => None,
            })
        });
        // Esc leaves a text box with nothing holding the keyboard; the next Tab goes on from the
        // box, not from the menu bar, so a box that keeps Tab for itself (the script editor's) is
        // no trap.
        let left = a11y::last_focus(ctx);
        if left.text && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.kb.escaped_from = left.id;
        } else if let Some(back) = tab {
            self.kb.tab_back = back;
            if let Some(from) = self.kb.escaped_from.take()
                && ctx.memory(|m| m.focused().is_none())
            {
                ctx.memory_mut(|m| m.request_focus(from));
            }
        } else if self.kb.pointer_at == now || ctx.memory(|m| m.focused().is_some()) {
            self.kb.escaped_from = None;
        }
        self.escape_from_pane(ctx);
        // F6: the next part of the window; Shift+F6 the one before.
        for (m, back) in [(Modifiers::NONE, false), (Modifiers::SHIFT, true)] {
            if take_key(ctx, Key::F6, m) {
                self.next_region(ctx, back);
            }
        }
        // F10: the menu bar, its File menu open (Shift+F10 is a card's menu).
        if take_key(ctx, Key::F10, Modifiers::NONE) {
            self.kb.menu_request = true;
        }
        self.menu_bar_arrows(ctx);
        let focused = ctx.memory(|m| m.focused());
        if focused != self.kb.begin_focus
            && let Some(f) = focused
            && now - self.kb.pointer_at > 0.5
        {
            a11y::scroll_into_view(ctx, f);
        }
        self.kb.begin_focus = ctx.memory(|m| m.focused());
    }

    /// At the end of a frame: the focus ring, the accessibility tree's last tidy, the status
    /// line's live region, and where the keyboard is, for the next frame.
    pub(super) fn keyboard_end(&mut self, ctx: &egui::Context) {
        let chrome = a11y::find_chrome(ctx);
        let focused = ctx.memory(|m| m.focused());
        if let Some(f) = focused
            && chrome.contains(&f)
            && focused != self.kb.last_focus
        {
            // Tab reached a widget only the mouse uses: on to the next one.
            self.kb.skip = Some(if self.kb.tab_back {
                egui::FocusDirection::Previous
            } else {
                egui::FocusDirection::Next
            });
            ctx.request_repaint();
        }
        a11y::paint_focus_ring(ctx, &chrome);
        a11y::tidy_tree(ctx, &chrome);
        if let Some((m, err)) = &self.message {
            a11y::live_region(ctx, m, *err);
        }
        self.kb.on_menus = focused.is_some_and(|f| self.kb.menu_buttons.iter().any(|b| b.0 == f))
            || (egui::Popup::is_any_open(ctx) && self.kb.on_menus);
        self.kb.last_focus = focused;
        a11y::note_focus(ctx);
    }

    /// Esc on a control of a docked pane (not a text box, nor in a list): the keyboard back to the
    /// grid, as a tool window's Esc closes it (#308). Pasting, Esc stops the paste instead.
    fn escape_from_pane(&mut self, ctx: &egui::Context) {
        let f = a11y::last_focus(ctx);
        let (Some(id), Some(grid)) = (f.id, self.kb.grid_id) else {
            return;
        };
        if f.text
            || f.popup
            || id == grid
            || self.paste.is_some()
            || !ctx.input(|i| i.key_pressed(Key::Escape))
            || ctx.viewport(|v| v.prev_pass.widgets.get(grid).is_none())
        {
            return;
        }
        let regions = a11y::last_regions(ctx);
        let in_pane = a11y::region_of(ctx, &regions, id).is_some_and(|i| {
            let r = &regions[i];
            r.layer == egui::LayerId::background() && panes::PANES.iter().any(|p| p.title == r.name)
        });
        if in_pane {
            ctx.memory_mut(|m| m.request_focus(grid));
            ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        }
    }

    /// F6 (or Shift+F6 with `back`): the keyboard to the first control of the next part of the
    /// window.
    fn next_region(&mut self, ctx: &egui::Context, back: bool) {
        let regions = a11y::last_regions(ctx);
        let targets: Vec<(usize, egui::Id)> = regions
            .iter()
            .enumerate()
            .filter_map(|(i, r)| a11y::focusable_in(ctx, r).first().map(|id| (i, *id)))
            .collect();
        if targets.is_empty() {
            return;
        }
        let here = ctx
            .memory(|m| m.focused())
            .and_then(|f| a11y::region_of(ctx, &regions, f));
        let k = match (here, back) {
            (None, false) => 0,
            (None, true) => targets.len() - 1,
            (Some(h), false) => targets.iter().position(|t| t.0 > h).unwrap_or(0),
            (Some(h), true) => targets
                .iter()
                .rposition(|t| t.0 < h)
                .unwrap_or(targets.len() - 1),
        };
        let (i, id) = targets[k];
        ctx.memory_mut(|m| m.request_focus(id));
        if regions[i].rank == a11y::WINDOW {
            ctx.move_to_top(regions[i].layer);
        }
        a11y::scroll_into_view(ctx, id);
        // egui's own arrow and Tab moves this frame would take it elsewhere.
        ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));
    }

    /// ← and → on a menu bar's menu: the menu before or after, open if this one was.
    fn menu_bar_arrows(&mut self, ctx: &egui::Context) {
        let Some(f) = ctx.memory(|m| m.focused()) else {
            return;
        };
        let Some(at) = self.kb.menu_buttons.iter().position(|b| b.0 == f) else {
            return;
        };
        let n = self.kb.menu_buttons.len();
        let to = if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowRight)) {
            (at + 1) % n
        } else if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowLeft)) {
            (at + n - 1) % n
        } else {
            return;
        };
        let open = egui::Popup::is_any_open(ctx);
        let (button, popup) = self.kb.menu_buttons[to];
        ctx.memory_mut(|m| {
            m.move_focus(egui::FocusDirection::None);
            m.request_focus(button);
        });
        if open {
            egui::Popup::open_id(ctx, popup);
        }
    }

    // ---------------------------------------------------------------- the grid's cell cursor

    /// The grid's keys, while it has the keyboard: the cursor, drawing, erasing, selecting and
    /// moving patterns.
    pub(super) fn grid_keys(&mut self, ui: &egui::Ui, resp: &egui::Response, xf: &view::TileXf) {
        let ctx = ui.ctx().clone();
        let topo = self.world.topo.clone();
        self.kb.cursor.fit(&topo);
        self.kb.grid_id = Some(resp.id);
        if !resp.has_focus() {
            return;
        }
        // The arrows and Esc are the grid's while it has the keyboard, not egui's.
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                    tab: false,
                },
            )
        });
        if self.kb.cursor.cell.is_none() {
            self.kb.cursor.cell = topo.locate(xf.center).or(Some(topo.center_cell()));
            self.kb.cursor.line = None;
        }
        let keys: Vec<(Key, Modifiers)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        for (key, m) in keys {
            let dir = match key {
                Key::ArrowLeft => Some(Dir::Left),
                Key::ArrowRight => Some(Dir::Right),
                Key::ArrowUp => Some(Dir::Up),
                Key::ArrowDown => Some(Dir::Down),
                _ => None,
            };
            let taken = match (dir, key) {
                (Some(d), _) if m.is_none() || m.shift_only() => {
                    if !self.kb.cursor.go(&topo, d, m.shift) {
                        self.message = Some((
                            format!("The edge of the tiling: no cell {}.", d.word()),
                            false,
                        ));
                    }
                    true
                }
                (Some(d), _) if m.command && !m.shift && !m.alt => {
                    self.move_selection(d);
                    true
                }
                (None, Key::Space) if m.is_none() => {
                    self.cursor_draw(false);
                    true
                }
                (None, Key::Delete | Key::Backspace) if m.is_none() => {
                    self.cursor_draw(true);
                    true
                }
                (None, Key::Escape) if m.is_none() => {
                    if self.kb.cursor.anchor.take().is_none() {
                        ctx.memory_mut(|mem| mem.surrender_focus(resp.id));
                    }
                    true
                }
                _ => false,
            };
            if taken {
                ctx.input_mut(|i| i.consume_key(m, key));
            }
        }
        // The cursor stays in view.
        if let Some(c) = self.kb.cursor.cell {
            let at = xf.to_screen(topo.centroid(c));
            if !resp.rect.shrink(16.0).contains(at) {
                self.cam = topo.centroid(c);
            }
        }
    }

    /// Space (draw) or Delete (`erase`) on the cursor's cell or the selection; Space pastes a
    /// waiting library pattern instead.
    fn cursor_draw(&mut self, erase: bool) {
        let topo = self.world.topo.clone();
        if !erase
            && self.paste.is_some()
            && let Some(c) = self.kb.cursor.cell
        {
            self.paste_at(c);
            return;
        }
        let Some(id) = self.draw_target.filter(|id| self.world.ca(*id).is_some()) else {
            self.message = Some((
                "No automaton to draw into: add one in the Add pane.".into(),
                true,
            ));
            return;
        };
        let v = if erase { 0 } else { self.draw_value.max(1) };
        let cells: Vec<[u32; 2]> = self
            .kb
            .cursor
            .selected(&topo)
            .iter()
            .map(|&c| [c as u32, v])
            .collect();
        self.exec(Command::SetCells { id, cells });
    }

    /// Ctrl+arrow: move what is selected (or the cursor's cell) one step, every automaton's cells
    /// with it, and the selection and cursor along with them.
    fn move_selection(&mut self, d: Dir) {
        let topo = self.world.topo.clone();
        let Some(shift) = self.kb.cursor.shift(&topo, d) else {
            self.message = Some((format!("Nothing further {} to move to.", d.word()), false));
            return;
        };
        let cells = self.kb.cursor.selected(&topo);
        let mut cmds = vec![];
        let mut lost = 0;
        for (fi, f) in self.world.fields.iter().enumerate() {
            // A sandpile's grains are added, never set: they stay where they are.
            if f.kind == FieldKind::Height {
                continue;
            }
            let Some(ca) = self.world.cas.iter().find(|c| c.field == fi) else {
                continue;
            };
            let full: Vec<(usize, u32)> = cells
                .iter()
                .map(|&c| (c, f.state[c] as u32))
                .filter(|v| v.1 != 0)
                .collect();
            let mut set: Vec<[u32; 2]> = full.iter().map(|(c, _)| [*c as u32, 0]).collect();
            for (c, v) in &full {
                match cell_cursor::moved(&topo, *c, shift) {
                    Some(to) => set.push([to as u32, *v]),
                    None => lost += 1,
                }
            }
            cmds.push(Command::SetCells {
                id: ca.id,
                cells: set,
            });
        }
        for c in cmds {
            self.exec(c);
        }
        let k = &mut self.kb.cursor;
        k.cell = k
            .cell
            .map(|c| cell_cursor::moved(&topo, c, shift).unwrap_or(c));
        k.anchor = k
            .anchor
            .map(|c| cell_cursor::moved(&topo, c, shift).unwrap_or(c));
        if lost > 0 {
            self.message = Some((
                format!("{lost} cells went off the edge of the tiling."),
                false,
            ));
        } else if matches!(shift, Shift::Vector(_)) {
            self.message = Some((
                "Moved to the nearest cells: an aperiodic tiling has no exact moves, so the \
                 pattern can change shape."
                    .into(),
                false,
            ));
        }
    }

    /// The cursor's cell in words, for a screen reader.
    fn cell_words(&self, cell: usize) -> String {
        let topo = &self.world.topo;
        let mut s = format!("Cell {cell}");
        if let Some(g) = topo.lattice() {
            let (u, j, k) = g.decompose(cell);
            if g.ntypes == 1 {
                s.push_str(&format!(", column {}, row {}", u + 1, j + 1));
            } else {
                s.push_str(&format!(
                    ", column {}, row {}, tile {} of {}",
                    u + 1,
                    j + 1,
                    k + 1,
                    g.ntypes
                ));
            }
        }
        let mut states = vec![];
        for ca in &self.world.cas {
            let f = &self.world.fields[ca.field];
            let v = match f.kind {
                FieldKind::Height => format!("{} grains", f.height[cell]),
                FieldKind::Binary => {
                    if f.state[cell] != 0 {
                        "alive".into()
                    } else {
                        "empty".into()
                    }
                }
                FieldKind::Multi(_) if matches!(ca.rule, Rule::Jaca(_)) => match f.state[cell] {
                    0 => "empty".into(),
                    s if s == jetcarp_core::jaca::POS => "+1".into(),
                    _ => "−1".into(),
                },
                FieldKind::Multi(_) => match f.state[cell] {
                    0 => "empty".into(),
                    s => format!("state {s}"),
                },
                FieldKind::Continuous => format!("{:.2}", f.value[cell]),
            };
            states.push(if self.world.cas.len() > 1 {
                format!("{} {v}", ca.name)
            } else {
                v
            });
        }
        if !states.is_empty() {
            s.push_str(": ");
            s.push_str(&states.join("; "));
        }
        if self.kb.cursor.anchor.is_some() {
            let n = self.kb.cursor.selected(topo).len();
            s.push_str(&format!(". {n} cells selected"));
        }
        s
    }

    /// The grid for a screen reader: a grid named after the world, and while it has the keyboard,
    /// the cursor's cell as its active descendant, so moving the cursor reads the new cell.
    pub(super) fn grid_accesskit(
        &mut self,
        ui: &mut egui::Ui,
        resp: &egui::Response,
        xf: &view::TileXf,
    ) {
        let name = if self.scene_name.is_empty() {
            "World grid".to_string()
        } else {
            format!("World grid: {}", self.scene_name)
        };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Grid, true, &name));
        let ctx = ui.ctx().clone();
        let n = self.world.topo.len();
        ctx.accesskit_node_builder(resp.id, |node| {
            node.set_description(format!(
                "{n} cells. The arrow keys move the cell cursor; Space draws, Delete erases, \
                 Shift and the arrows select, Control and the arrows move the selection, Escape \
                 leaves."
            ))
        });
        if !resp.has_focus() {
            return;
        }
        let Some(cell) = self.kb.cursor.cell else {
            return;
        };
        let topo = self.world.topo.clone();
        let words = self.cell_words(cell);
        let selected =
            self.kb.cursor.anchor.is_some() && self.kb.cursor.selected(&topo).contains(&cell);
        let pts: Vec<egui::Pos2> = topo
            .polygon(cell)
            .iter()
            .map(|p| xf.to_screen(*p))
            .collect();
        let bounds = egui::Rect::from_points(&pts);
        let id = resp.id.with(("cursor", cell));
        let child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(resp.rect)
                .accessibility_parent(resp.id),
        );
        child.interact(
            egui::Rect::from_center_size(bounds.center(), egui::Vec2::ZERO),
            id,
            Sense::hover(),
        );
        ctx.accesskit_node_builder(id, |node| {
            node.set_role(egui::accesskit::Role::GridCell);
            node.set_label(words);
            node.set_bounds(egui::accesskit::Rect {
                x0: bounds.min.x.into(),
                y0: bounds.min.y.into(),
                x1: bounds.max.x.into(),
                y1: bounds.max.y.into(),
            });
            if selected {
                node.set_selected(true);
            }
        });
        ctx.accesskit_node_builder(resp.id, |node| {
            node.set_active_descendant(id.accesskit_id())
        });
    }

    /// The cursor and the selection, drawn over the grid while it has the keyboard: the cursor's
    /// cell ringed in the focus colour on a dark halo (seen on any palette), the selected cells
    /// outlined.
    pub(super) fn paint_cursor(&self, ui: &egui::Ui, resp: &egui::Response, xf: &view::TileXf) {
        if !resp.has_focus() {
            return;
        }
        let Some(cell) = self.kb.cursor.cell else {
            return;
        };
        let topo = &self.world.topo;
        let ring = a11y::ring_stroke(ui.visuals(), a11y::high_contrast(ui.ctx()));
        let p = ui.painter_at(resp.rect);
        let screen = |c: usize| -> Vec<egui::Pos2> {
            topo.polygon(c).iter().map(|q| xf.to_screen(*q)).collect()
        };
        if self.kb.cursor.anchor.is_some() {
            let cells = self.kb.cursor.selected(topo);
            if cells.len() <= 5000 {
                // Each selected cell tinted (where it is convex: a hat's or a spectre's outline
                // alone) and outlined, lighter than the cursor's ring.
                let s = egui::Stroke::new(1.0, ring.color.gamma_multiply(0.8));
                let tint = ring.color.gamma_multiply(0.22);
                for &c in cells.iter() {
                    let pts = screen(c);
                    if convex(&pts) {
                        p.add(egui::Shape::convex_polygon(
                            pts.clone(),
                            tint,
                            egui::Stroke::NONE,
                        ));
                    }
                    p.add(egui::Shape::closed_line(pts, s));
                }
            } else {
                let a = self.kb.cursor.anchor.unwrap();
                let r = egui::Rect::from_two_pos(
                    xf.to_screen(topo.centroid(a)),
                    xf.to_screen(topo.centroid(cell)),
                );
                p.rect_stroke(
                    r,
                    0.0,
                    egui::Stroke::new(1.5, ring.color),
                    egui::StrokeKind::Middle,
                );
            }
        }
        // The cursor: a heavier ring than the focus ring, on a dark edge so it shows on light
        // palettes as well as dark ones.
        let w = ring.width.max(2.0) + 1.0;
        let pts = screen(cell);
        p.add(egui::Shape::closed_line(
            pts.clone(),
            egui::Stroke::new(w + 4.0, Color32::from_black_alpha(210)),
        ));
        p.add(egui::Shape::closed_line(
            pts,
            egui::Stroke::new(w, ring.color),
        ));
    }

    /// Mouse-only widgets found last frame (tests).
    #[cfg(test)]
    pub(super) fn kb_chrome(&self, ctx: &egui::Context) -> std::collections::HashSet<egui::Id> {
        a11y::chrome(ctx)
    }
}

/// One line of the table of keys: the keys on Windows and Linux, on macOS (either empty where
/// that system has none), and what they do.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Shortcut {
    pub keys: String,
    pub mac: String,
    pub what: String,
}

/// A group of the table's lines, under its heading.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ShortcutGroup {
    pub name: String,
    pub keys: Vec<Shortcut>,
}

/// Every key the app answers to: moving about, the menus' shortcuts (each with the menu it is
/// in), and the grid's cell cursor. *Help → Keyboard shortcuts*, `jetcarp shortcuts` and the book's
/// keyboard page (`docs/tools/gen-shortcuts.py`) all show this table.
pub fn table() -> Vec<ShortcutGroup> {
    let lines = |l: &[KeyLine]| {
        l.iter()
            .map(|l| Shortcut {
                keys: l.keys.into(),
                mac: l.mac.into(),
                what: l.what.into(),
            })
            .collect()
    };
    let words = |ks: &[egui::KeyboardShortcut], mac: bool| {
        ks.iter()
            .map(|k| key_words(k, mac))
            .collect::<Vec<_>>()
            .join(" or ")
    };
    let mut menus = vec![];
    for a in menu::Action::all() {
        let (pc, mac) = (a.keys_for(false), a.keys_for(true));
        if pc.is_empty() && mac.is_empty() {
            continue;
        }
        let label = a.label();
        let what = match menu::menu_of(a) {
            Some(m) => format!("{m} → {}", label.trim_end_matches('…')),
            None => label,
        };
        menus.push(Shortcut {
            keys: words(pc, false),
            mac: words(mac, true),
            what,
        });
    }
    // macOS's Minimise, handled outside the action table (#98).
    menus.push(Shortcut {
        keys: String::new(),
        mac: "⌘M".into(),
        what: "Minimise the window".into(),
    });
    vec![
        ShortcutGroup {
            name: "Moving about".into(),
            keys: lines(MOVING),
        },
        ShortcutGroup {
            name: "The menus' shortcuts".into(),
            keys: menus,
        },
        ShortcutGroup {
            name: "On the grid, while it has the keyboard".into(),
            keys: lines(GRID),
        },
    ]
}

/// A shortcut in words: `Ctrl+Shift+H` on Windows and Linux, `⇧⌘H` on macOS (`mac`), the
/// modifiers in Apple's order there.
pub(crate) fn key_words(k: &egui::KeyboardShortcut, mac: bool) -> String {
    let key = match k.logical_key {
        Key::ArrowLeft => "←",
        Key::ArrowRight => "→",
        Key::ArrowUp => "↑",
        Key::ArrowDown => "↓",
        Key::Escape => "Esc",
        Key::Plus => "+",
        Key::Minus => "−",
        Key::Equals => "=",
        Key::Enter if mac => "Return",
        other => other.name(),
    };
    let m = k.modifiers;
    if mac {
        let mut s = String::new();
        for (on, c) in [
            (m.ctrl, '⌃'),
            (m.alt, '⌥'),
            (m.shift, '⇧'),
            (m.mac_cmd || m.command, '⌘'),
        ] {
            if on {
                s.push(c);
            }
        }
        s + key
    } else {
        let mut parts = vec![];
        for (on, w) in [
            (m.ctrl || m.command, "Ctrl"),
            (m.alt, "Alt"),
            (m.shift, "Shift"),
        ] {
            if on {
                parts.push(w);
            }
        }
        parts.push(key);
        parts.join("+")
    }
}

/// Whether a polygon on screen is convex (every turn the same way), so it can be filled as one.
fn convex(pts: &[egui::Pos2]) -> bool {
    let n = pts.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b, c) = (pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
        let cross = (b - a).x * (c - b).y - (b - a).y * (c - b).x;
        if cross.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// Take a press of `key` with exactly the modifiers `m` (egui's own matching lets an extra Shift
/// through, so F6 would take Shift+F6 too).
fn take_key(ctx: &egui::Context, key: Key, m: Modifiers) -> bool {
    ctx.input_mut(|i| {
        let at = i.events.iter().position(|e| {
            matches!(e, egui::Event::Key { key: k, pressed: true, modifiers, .. }
                if *k == key && modifiers.matches_exact(m))
        });
        if let Some(at) = at {
            i.events.remove(at);
        }
        at.is_some()
    })
}

/// Whether a bare key belongs to the widget that has the keyboard rather than to the app's
/// shortcuts: Space and Enter press it, the arrows move in it or from it, Tab leaves it.
pub(super) fn widget_key(k: Key) -> bool {
    matches!(
        k,
        Key::Space
            | Key::Enter
            | Key::Tab
            | Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The focus ring stands out at 3:1 or more from every panel and window it is drawn on, in
    /// egui's dark and light themes and both forms of the high-contrast one, and is at least two
    /// points wide (WCAG 2.2's focus appearance).
    #[test]
    fn the_focus_ring_shows_in_every_theme() {
        for high in [false, true] {
            for dark in [true, false] {
                let v = crate::theme::visuals(dark, high);
                let s = a11y::ring_stroke(&v, high);
                assert!(s.width >= 2.0, "{dark} {high}: {}", s.width);
                for (what, bg) in [
                    ("panel", v.panel_fill),
                    ("window", v.window_fill),
                    ("text field", v.text_edit_bg_color()),
                    ("the grid's backdrop", shell::GRID_BG),
                ] {
                    // The grid's backdrop is under the docked panes only in egui's dark theme.
                    if what == "the grid's backdrop" && (high || !dark) {
                        continue;
                    }
                    let c = crate::theme::contrast(s.color, bg);
                    assert!(
                        c >= 3.0,
                        "dark {dark}, high contrast {high}: the ring on the {what} is {c:.2}:1"
                    );
                }
            }
        }
    }

    /// The table names each key as each system writes it, and has every menu shortcut, each with
    /// its menu.
    #[test]
    fn the_table_has_every_shortcut_in_each_system_s_words() {
        use egui::KeyboardShortcut as K;
        let ctrl_shift_h = K::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::H);
        assert_eq!(key_words(&ctrl_shift_h, false), "Ctrl+Shift+H");
        assert_eq!(key_words(&ctrl_shift_h, true), "⇧⌘H");
        let full = K::new(Modifiers::CTRL.plus(Modifiers::MAC_CMD), Key::F);
        assert_eq!(key_words(&full, true), "⌃⌘F");
        assert_eq!(
            key_words(&K::new(Modifiers::NONE, Key::ArrowRight), false),
            "→"
        );
        assert_eq!(key_words(&K::new(Modifiers::NONE, Key::Minus), false), "−");
        let t = table();
        let menus = &t[1].keys;
        for a in menu::Action::all() {
            let Some(k) = a.keys_for(false).first() else {
                continue;
            };
            let w = key_words(k, false);
            assert!(
                menus.iter().any(|s| s.keys.split(" or ").any(|x| x == w)
                    && s.what.ends_with(a.label().trim_end_matches('…'))),
                "{a:?} ({w}) is not in the table"
            );
        }
        let find = |what: &str| menus.iter().find(|s| s.what == what).unwrap();
        assert_eq!(find("View → Hide all panes").keys, "F9");
        assert_eq!(find("View → Auto-hide menu bar").keys, "Ctrl+F10");
        assert_eq!(find("View → Full screen").keys, "F11");
        assert_eq!(find("View → Full screen").mac, "⌃⌘F or F11");
        assert_eq!(find("Minimise the window").keys, "");
    }

    #[test]
    fn the_keyboard_table_s_keys_are_the_widget_s_or_the_app_s() {
        assert!(widget_key(Key::Space) && widget_key(Key::ArrowLeft) && widget_key(Key::Tab));
        assert!(!widget_key(Key::R) && !widget_key(Key::F) && !widget_key(Key::F9));
        // Every line says what it does, in a sentence's worth of words.
        for l in MOVING.iter().chain(GRID) {
            assert!(
                !l.keys.is_empty() && !l.mac.is_empty() && l.what.len() > 10,
                "{}",
                l.keys
            );
        }
    }
}

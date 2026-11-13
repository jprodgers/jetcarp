//! Windows that float inside the main window or pop out to their own OS window.
//!
//! [`show`] draws one window either way: floating, as an `egui::Window` with small buttons in its
//! title bar (pop out, and dock for panes); popped out, as an immediate viewport titled
//! "JETCARP · <title>" that can sit on any monitor. Closing a popped-out window returns it to the
//! main window. The caller keeps the [`WinState`] (panes keep theirs in the pane layout).
//!
//! Tool windows use [`tool_window`], which keeps their state in a registry in egui's memory so
//! each tool's `show(ctx)` needs no extra arguments; the app saves and restores that registry
//! with its settings ([`registry`], [`set_registry`]).

use eframe::egui::{self, Id, Ui, ViewportBuilder, ViewportClass, ViewportId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where an open window shows, and the size of its own OS window when popped out.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct WinState {
    pub popped: bool,
    /// Size asked for when the popped-out window opens, in points (kept between runs).
    pub size: Option<[f32; 2]>,
    /// Content size seen since it opened. On Wayland the content can come out smaller than the
    /// size asked for (client-side decorations), so resizes are tracked as changes from this.
    #[serde(skip)]
    pub seen: Option<[f32; 2]>,
    /// When the popped-out window opened (its first sizes are the compositor settling).
    #[serde(skip)]
    pub opened_at: Option<f64>,
}

/// What the user asked for through a window's chrome this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chrome {
    /// The pop-out button: move to its own OS window.
    PopOut,
    /// The OS window was closed, or its "back" button pressed: return to the main window.
    Return,
    /// The dock button (panes only).
    Dock,
    /// The close button of a floating window (`open` is now false).
    Close,
}

/// Adjusts a floating window's `egui::Window` builder.
pub type Configure<'a> = Box<dyn for<'w> FnOnce(egui::Window<'w>) -> egui::Window<'w> + 'a>;

/// How to show a window. `configure` adjusts the floating `egui::Window` (default position,
/// minimum size, resizing); `default_size` is used both floating and for a new OS window.
pub struct Spec<'a> {
    /// Stable key: the window's egui id and its OS window's viewport id derive from it.
    pub key: &'a str,
    pub title: &'a str,
    pub default_size: [f32; 2],
    /// Show a dock button (panes).
    pub dockable: bool,
    pub configure: Option<Configure<'a>>,
}

impl<'a> Spec<'a> {
    pub fn new(key: &'a str, title: &'a str) -> Self {
        Spec {
            key,
            title,
            default_size: [520.0, 480.0],
            dockable: false,
            configure: None,
        }
    }
    pub fn size(mut self, w: f32, h: f32) -> Self {
        self.default_size = [w, h];
        self
    }
    pub fn dockable(mut self) -> Self {
        self.dockable = true;
        self
    }
    pub fn configure(
        mut self,
        f: impl for<'w> FnOnce(egui::Window<'w>) -> egui::Window<'w> + 'a,
    ) -> Self {
        self.configure = Some(Box::new(f));
        self
    }
}

/// The OS window title for a window titled `title`.
pub fn os_title(title: &str) -> String {
    format!("JETCARP · {title}")
}

/// The viewport id of `key`'s popped-out window.
pub fn viewport_id(key: &str) -> ViewportId {
    ViewportId::from_hash_of(("jetcarp-popout", key))
}

/// Show a window in an OS window of its own, now (an immediate viewport). The contents get the
/// viewport's whole area. Returns what `add` returned, and true when the user closed the window.
/// This is the building block for anything shown in another OS window (wave 2's outputs too).
///
/// `add` can run more than once in a frame, as the main window's code can: egui runs a viewport
/// again in the same frame when a widget asks it to (a grid shown for the first time does). What
/// the last run returned counts, so act on clicks inside `add` rather than through what it
/// returns; a close seen by any run counts.
pub fn os_window<R>(
    ctx: &egui::Context,
    key: &str,
    builder: ViewportBuilder,
    mut add: impl FnMut(&mut Ui, ViewportClass) -> R,
) -> (R, bool, Option<[f32; 2]>) {
    let mut closed = false;
    let (r, size) = ctx.show_viewport_immediate(viewport_id(key), builder, |ui, class| {
        closed |= ui.input(|i| i.viewport().close_requested());
        // Inner size in points (`ViewportInfo::inner_rect` needs a window position, which
        // Wayland does not give).
        let r = ui.ctx().content_rect();
        let size = (class != ViewportClass::EmbeddedWindow).then(|| [r.width(), r.height()]);
        (add(ui, class), size)
    });
    (r, closed, size)
}

/// Show one window, floating or popped out (per `state`). Nothing when `open` is false. Popped
/// out, `add` can run more than once in a frame (see [`os_window`]).
pub fn show<R>(
    ctx: &egui::Context,
    spec: Spec<'_>,
    state: &mut WinState,
    open: &mut bool,
    mut add: impl FnMut(&mut Ui) -> R,
) -> (Option<R>, Vec<Chrome>) {
    let mut events = vec![];
    if !*open {
        return (None, events);
    }
    if state.popped {
        let size = state.size.unwrap_or(spec.default_size);
        let builder = ViewportBuilder::default()
            .with_title(os_title(spec.title))
            .with_app_id("jetcarp")
            .with_inner_size(size)
            .with_min_inner_size([220.0, 120.0]);
        let dockable = spec.dockable;
        // A button pressed in any run counts (the contents can run twice in a frame).
        let mut ev = None;
        let (r, closed, size) = os_window(ctx, spec.key, builder, |ui, _class| {
            egui::Panel::top(Id::unique(("popout-chrome", spec.key)))
                .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::symmetric(6, 2)))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(spec.title).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if dockable {
                                if ui
                                    .small_button("Dock")
                                    .on_hover_text("Dock this pane in the main window (closing this window does the same)")
                                    .clicked()
                                {
                                    ev = Some(Chrome::Dock);
                                }
                            } else if ui
                                .small_button("Back to main window")
                                .on_hover_text("Float this window in the main window again (closing this window does the same)")
                                .clicked()
                            {
                                ev = Some(Chrome::Return);
                            }
                        });
                    });
                });
            egui::CentralPanel::default().show(ui, &mut add).inner
        });
        if let Some(now) = size
            && now[0] > 50.0
            && now[1] > 50.0
        {
            let t = ctx.input(|i| i.time);
            let settling = t - *state.opened_at.get_or_insert(t) < 0.75;
            match state.seen {
                Some(_) if settling => state.seen = Some(now),
                Some(was) if was != now => {
                    let asked = state.size.unwrap_or(spec.default_size);
                    state.size = Some([asked[0] + now[0] - was[0], asked[1] + now[1] - was[1]]);
                    state.seen = Some(now);
                }
                Some(_) => {}
                None => {
                    state.size = Some(state.size.unwrap_or(spec.default_size));
                    state.seen = Some(now);
                }
            }
        }
        if closed {
            events.push(Chrome::Return);
        }
        events.extend(ev);
        for e in &events {
            if matches!(e, Chrome::Return | Chrome::Dock) {
                state.popped = false;
                state.seen = None;
                state.opened_at = None;
            }
        }
        return (Some(r), events);
    }
    let id = Id::unique(("jetcarp-win", spec.key));
    let mut w = egui::Window::new(spec.title)
        .id(id)
        .open(open)
        .default_size(spec.default_size);
    if let Some(c) = spec.configure {
        w = c(w);
    }
    let resp = w.show(ctx, |ui| add(ui));
    let mut out = None;
    if let Some(resp) = resp {
        if title_buttons(ctx, &resp.response, spec.dockable, &mut events) {
            state.popped = true;
        }
        out = resp.inner;
    }
    if !*open {
        events.push(Chrome::Close);
    }
    (out, events)
}

/// Small buttons in a floating window's title bar, left of its close button. True: pop out.
fn title_buttons(
    ctx: &egui::Context,
    win: &egui::Response,
    dockable: bool,
    events: &mut Vec<Chrome>,
) -> bool {
    let style = ctx.global_style();
    let h = ctx.fonts_mut(|f| f.row_height(&egui::TextStyle::Heading.resolve(&style)));
    let m = style.spacing.window_margin;
    let gap = style.spacing.item_spacing.x;
    let rect = win.rect;
    // The close button sits at the right inner margin; ours go left of it.
    let n = if dockable { 2.0 } else { 1.0 };
    let right = rect.right() - m.right as f32 - h - gap;
    let area = egui::Rect::from_min_max(
        egui::pos2(right - n * (h + 2.0), rect.top() + m.top as f32),
        egui::pos2(right, rect.top() + m.top as f32 + h),
    );
    if area.left() < rect.left() + 80.0 {
        return false;
    }
    let mut ui = Ui::new(
        ctx.clone(),
        win.id.with("title-buttons"),
        egui::UiBuilder::new()
            .layer_id(win.layer_id)
            .max_rect(area)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let mut popped = false;
    if ui
        .add(egui::Button::new("↗").frame(false).small())
        .on_hover_text("Pop out to its own window (move it to any monitor)")
        .clicked()
    {
        events.push(Chrome::PopOut);
        popped = true;
    }
    if dockable
        && ui
            .add(egui::Button::new("⊟").frame(false).small())
            .on_hover_text("Dock in the main window")
            .clicked()
    {
        events.push(Chrome::Dock);
    }
    popped
}

/// Pop-out state of every tool window, by key.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(transparent)]
pub struct Registry(pub BTreeMap<String, WinState>);

fn registry_id() -> Id {
    Id::unique("jetcarp-popout-registry")
}

pub fn registry(ctx: &egui::Context) -> Registry {
    ctx.data(|d| d.get_temp::<Registry>(registry_id()))
        .unwrap_or_default()
}

pub fn set_registry(ctx: &egui::Context, r: Registry) {
    ctx.data_mut(|d| d.insert_temp(registry_id(), r));
}

pub fn is_popped(ctx: &egui::Context, key: &str) -> bool {
    registry(ctx).0.get(key).is_some_and(|s| s.popped)
}

pub fn set_popped(ctx: &egui::Context, key: &str, popped: bool) {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Registry>(registry_id())
            .0
            .entry(key.to_string())
            .or_default()
            .popped = popped;
    });
}

/// A tool window: floating or popped out, its state kept in the registry. Returns what `add`
/// returned when shown. Popped out, `add` can run more than once in a frame (see [`os_window`]).
pub fn tool_window<R>(
    ctx: &egui::Context,
    spec: Spec<'_>,
    open: &mut bool,
    add: impl FnMut(&mut Ui) -> R,
) -> Option<R> {
    if !*open {
        return None;
    }
    let key = spec.key.to_string();
    let mut st = registry(ctx).0.get(&key).cloned().unwrap_or_default();
    let (r, _events) = show(ctx, spec, &mut st, open, add);
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Registry>(registry_id())
            .0
            .insert(key, st);
    });
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_round_trip_and_flags() {
        let ctx = egui::Context::default();
        assert!(!is_popped(&ctx, "library"));
        set_popped(&ctx, "library", true);
        assert!(is_popped(&ctx, "library"));
        let r = registry(&ctx);
        let json = serde_json::to_string(&r).unwrap();
        let back: Registry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
        set_popped(&ctx, "library", false);
        assert!(!is_popped(&ctx, "library"));
        set_registry(&ctx, back);
        assert!(is_popped(&ctx, "library"));
    }

    /// A popped-out window's contents can run twice in one frame: eframe draws an immediate
    /// viewport with `Context::run_ui`, which runs it again when a widget asks for a discard (a
    /// grid shown for the first time does). Issue #1: the AI control window, popped out while
    /// its grid showed, crashed the app ("viewport callback runs once").
    #[test]
    fn a_popped_out_window_survives_a_second_pass() {
        // An immediate-viewport renderer as eframe's does it, without a window.
        egui::Context::set_immediate_viewport_renderer(|ctx, mut vp| {
            let input = egui::RawInput {
                viewport_id: vp.ids.this,
                viewports: std::iter::once((vp.ids.this, Default::default())).collect(),
                ..Default::default()
            };
            ctx.run_ui(input, |ui| (vp.viewport_ui_cb)(ui))
                .textures_delta
                .clear();
        });
        let ctx = egui::Context::default();
        ctx.set_embed_viewports(false);
        let mut state = WinState {
            popped: true,
            ..Default::default()
        };
        let mut open = true;
        let mut runs = 0;
        let mut shown = None;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let spec = Spec::new("pass-test", "Pass test");
            let (r, _) = show(ui.ctx(), spec, &mut state, &mut open, |ui| {
                runs += 1;
                egui::Grid::new("grid").show(ui, |ui| {
                    ui.label("Address");
                    ui.label("127.0.0.1:47811");
                    ui.end_row();
                });
                7
            });
            shown = r;
        });
        out.textures_delta.clear();
        assert_eq!(shown, Some(7));
        assert!(runs >= 2, "the new grid asked for a second pass ({runs})");
        assert!(state.popped && open);
    }
}

//! The top bar's *Jump* menu: HashLife jumps (`jetcarp_core::hashlife`).
//!
//! - **Jump ahead (HashLife)**: in place, through `Command::Jump`, when the world qualifies
//!   (Life-like, Generations, WireWorld, or Greenberg–Hastings CAs, B0 included, one or several,
//!   on a wrap-edge lattice whose sides are powers of two). States equal stepping; ages restart
//!   at 1.
//! - **On the unbounded plane**: any lattice world with such CAs: the pattern runs on the
//!   infinite plane (edges ignored) and the same frame is shown as a new world.
//! - **HashLife view…**: the pattern on the plane in its own window (`hlview_ui`), jumped any
//!   distance and drawn from the quadtree at any zoom.

use jetcarp_core::World;
use jetcarp_core::hashlife;
use eframe::egui;

pub enum JumpAction {
    InPlace(u64),
    Plane(u64),
    /// Open the HashLife view on this world's pattern.
    View,
}

/// The menu body. `gens` is the text field's contents.
pub fn menu(ui: &mut egui::Ui, world: &World, gens: &mut String) -> Option<JumpAction> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.label("Generations");
        ui.add(egui::TextEdit::singleline(gens).desired_width(90.0))
            .on_hover_text("A number, or 2^40, 1e6");
    });
    let n = hashlife::parse_gens(gens);
    if let Err(e) = &n {
        ui.colored_label(egui::Color32::LIGHT_RED, e);
    }
    let torus = hashlife::torus_problem(world);
    let plane = hashlife::plane_problem(world);
    let r = ui
        .add_enabled(
            n.is_ok() && torus.is_none(),
            egui::Button::new("Jump ahead (HashLife)"),
        )
        .on_hover_text(
            "Advance this wrap-edge world: the states are exactly what stepping gives. HashLife does \
             not track ages, so every live cell restarts at age 1 (age colours start over).",
        )
        .on_disabled_hover_text(torus.clone().unwrap_or_default());
    if r.clicked() {
        out = n.clone().ok().map(JumpAction::InPlace);
    }
    let r = ui
        .add_enabled(
            n.is_ok() && plane.is_none(),
            egui::Button::new("On the unbounded plane"),
        )
        .on_hover_text(
            "Run the pattern on the infinite plane (edges ignored) and show this frame as a new \
             world.",
        )
        .on_disabled_hover_text(plane.clone().unwrap_or_default());
    if r.clicked() {
        out = n.ok().map(JumpAction::Plane);
    }
    let r = ui
        .add_enabled(plane.is_none(), egui::Button::new("HashLife view…"))
        .on_hover_text(
            "Open the pattern on the unbounded plane in its own view: jump as far as you like, \
             then pan and zoom over the whole universe (drawn from the quadtree at the zoom).",
        )
        .on_disabled_hover_text(plane.clone().unwrap_or_default());
    if r.clicked() {
        out = Some(JumpAction::View);
    }
    if let Some(why) = torus.filter(|_| plane.is_none()) {
        ui.label(
            egui::RichText::new(format!("In place: {why}"))
                .small()
                .weak(),
        );
    }
    out
}

/// The plane jump: a new world with this world's frame and edges.
pub fn plane_jump(world: &World, gens: u64) -> Result<World, String> {
    let opts = hashlife::Options::default();
    let mut p = hashlife::Plane::from_world(world, &opts)?;
    p.step(gens)?;
    let (w, h) = match world.topo.lattice() {
        Some(g) => (g.w as u64, g.h as u64),
        None => return Err("not a lattice".into()),
    };
    let sc = p.to_scene((0, 0, w, h), world.topo.spec.boundary)?;
    World::from_scene(&sc)
}

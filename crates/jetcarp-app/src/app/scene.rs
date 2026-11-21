//! Scenes and files: load, show, save, open, export, rule tables, pasting library patterns.

use super::*;
use crate::file_dialog;
use std::path::{Path, PathBuf};

/// A contact sheet waiting for a file: its kind, first rule, palette and random row.
pub(super) type PendingSheet = (jetcarp_core::sheet::SheetKind, u64, String, bool);

impl JetcarpApp {
    /// Load a scene now (AI control expects the world in place when this returns). The UI uses
    /// `load_scene_bg`, which builds large tilings in the background.
    pub(super) fn load_scene(&mut self, scene: &Scene) {
        let play = self.opens_running();
        self.load_scene_as(scene, play);
    }

    /// [`Self::load_scene`], running afterwards or not (`play`). Automata the scene names no
    /// palette for take the defaults of Settings → Accessibility.
    pub(super) fn load_scene_as(&mut self, scene: &Scene, play: bool) {
        self.cancel_build();
        let filled = self.with_default_palettes(scene);
        let scene: &Scene = &filled;
        match World::from_scene(scene) {
            Ok(w) => {
                self.world = w;
                self.after_scene(scene, None, play);
            }
            Err(e) => self.message = Some((format!("Could not load scene: {e}"), true)),
        }
    }

    /// The world of `scene` is in place: show it (`geom`: its geometry, if already built), and
    /// run it if `play`.
    pub(super) fn after_scene(&mut self, scene: &Scene, geom: Option<Geometry>, play: bool) {
        self.scene_name = scene.name.clone();
        self.scene_desc = scene.description.clone();
        self.on_new_topology_with(geom);
        self.draw_target = self.world.cas.first().map(|c| c.id);
        self.playing = play;
        self.acc = 0.0;
        if let Some(v) = &scene.view {
            if let (Some(c), Some(z)) = (v.get("center"), v.get("zoom").and_then(|z| z.as_f64()))
                && let (Some(x), Some(y)) = (
                    c.get(0).and_then(|x| x.as_f64()),
                    c.get(1).and_then(|x| x.as_f64()),
                )
            {
                // Scenes keep the zoom in points per world unit; the camera is in pixels.
                self.cam = [x, y];
                self.zoom = z * self.ppp() as f64;
                self.fit_pending = false;
            }
            if let Some(s) = v.get("speed").and_then(|s| s.as_f64()) {
                self.speed = s as f32;
            }
        }
        self.screen = Screen::Sim;
        // What loading let pass (a palette this JETCARP does not know) goes in the status line,
        // after the note of a scene opened paused for reduced motion.
        let mut notes = self.world.warnings().to_vec();
        if std::mem::take(&mut self.a11y.paused_open) && !play {
            notes.insert(0, accessibility::PAUSED_NOTE.to_string());
        }
        self.message = (!notes.is_empty()).then(|| (notes.join("; "), false));
    }

    /// Show a world built elsewhere (a script's `show(w)`, a sweep result) in the main view.
    pub(super) fn show_world(&mut self, world: World, title: String, desc: String) {
        self.cancel_build();
        self.world = world;
        self.scene_name = title;
        self.scene_desc = desc;
        self.on_new_topology();
        self.draw_target = self.world.cas.first().map(|c| c.id);
        self.playing = false;
        self.acc = 0.0;
        self.screen = Screen::Sim;
    }

    /// Paste the armed library pattern with its corner on `cell`, into the CA with the
    /// pattern's rule (else the draw target).
    pub(super) fn paste_at(&mut self, cell: usize) {
        let Some(p) = self.paste.clone() else { return };
        let target = self
            .world
            .cas
            .iter()
            .find(|c| c.notation() == p.rule)
            .map(|c| c.id)
            .or(self.draw_target);
        let Some(id) = target else {
            self.message = Some((
                format!("Add a CA first ({}), or use Open in a new world.", p.rule),
                true,
            ));
            return;
        };
        if self
            .exec(Command::PastePattern {
                id,
                pattern: p.source,
                at: jetcarp_core::pattern::PasteAt::Index { cell: cell as u32 },
                orientation: p.orientation,
                phase: 0,
            })
            .is_some()
        {
            self.message = Some((format!("Pasted {}. Esc stops pasting.", p.label), false));
        }
    }

    pub(super) fn empty_world(&mut self) {
        self.t_family = "square".into();
        self.t_boundary = Boundary::Wrap;
        self.t_size = SizeChoice::Fill;
        let spec = self.tiling_spec_from_ui(false);
        match World::new(spec, 1) {
            Ok(w) => {
                self.world = w;
                self.scene_name = "Empty world".into();
                self.scene_desc = "Add a CA from the panel on the left.".into();
                self.on_new_topology();
                self.draw_target = None;
                self.playing = false;
                self.screen = Screen::Sim;
            }
            Err(e) => self.message = Some((e, true)),
        }
    }

    pub(super) fn current_scene(&self) -> Scene {
        let mut s = self.world.to_scene();
        s.name = self.scene_name.clone();
        s.description = self.scene_desc.clone();
        let zoom = self.zoom / self.ppp() as f64;
        s.view = Some(serde_json::json!({ "center": self.cam, "zoom": zoom, "speed": self.speed }));
        s
    }

    /// The context file dialogs ask to repaint when they answer (#136).
    pub(super) fn dialog_ctx(&self) -> egui::Context {
        self.ctx.clone().unwrap_or_default()
    }

    /// The answers of the File menu's dialogs and the rule table's, picked up each frame (#136).
    pub(super) fn file_dialog_answers(&mut self) {
        if let Some(p) = file_dialog::take::<PathBuf>("scene.save") {
            self.save_scene_to(&p);
        }
        if let Some(p) = file_dialog::take::<PathBuf>("scene.open") {
            self.open_scene_from(&p);
        }
        if let Some(p) = file_dialog::take::<PathBuf>("scene.export_png") {
            self.export_png_to(&p);
        }
        if let Some(p) = file_dialog::take::<PathBuf>("scene.export_view") {
            self.export_view_to(&p);
        }
        if let Some(p) = file_dialog::take::<PathBuf>("scene.load_table") {
            self.load_table_from(&p);
        }
        self.anim_export_answer();
    }

    pub(super) fn save_scene(&mut self) {
        file_dialog::open(&self.dialog_ctx(), "scene.save", || {
            rfd::FileDialog::new()
                .add_filter("JETCARP scene", &["json"])
                .set_file_name("scene.json")
                .save_file()
        });
    }

    fn save_scene_to(&mut self, path: &Path) {
        match std::fs::write(&path, self.current_scene().to_json()) {
            Ok(()) => {
                self.message = Some((
                    format!("Saved {} (hash {})", path.display(), self.world.hash_hex()),
                    false,
                ))
            }
            Err(e) => self.message = Some((format!("Could not save: {e}"), true)),
        }
    }

    /// *File → Open scene…*: the scene opens paused at the generation it was saved at, so that
    /// is what shows (Quick Start entries and the rest start running).
    pub(super) fn open_scene(&mut self) {
        file_dialog::open(&self.dialog_ctx(), "scene.open", || {
            rfd::FileDialog::new()
                .add_filter("JETCARP scene", &["json"])
                .pick_file()
        });
    }

    fn open_scene_from(&mut self, path: &Path) {
        match std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| Scene::from_json(&t))
        {
            Ok(s) => self.load_scene_bg_as(&s, false),
            Err(e) => self.message = Some((format!("Could not open: {e}"), true)),
        }
    }

    pub(super) fn export_png(&mut self) {
        file_dialog::open(&self.dialog_ctx(), "scene.export_png", || {
            rfd::FileDialog::new()
                .add_filter("PNG image", &["png"])
                .set_file_name("jetcarp.png")
                .save_file()
        });
    }

    fn export_png_to(&mut self, path: &Path) {
        let opts = self.export_options();
        let mut meta = render::png_metadata(&self.world);
        if let Some(m) = meta.iter_mut().find(|m| m.0 == "jetcarp:scene") {
            m.1 = self.current_scene().to_json();
        }
        match render::write_png_streaming(path, &self.world, &opts, &meta) {
            Ok((w, h)) => {
                self.message = Some((
                    format!(
                        "Exported {} ({w}x{h}), generation {}, hash {}",
                        path.display(),
                        self.world.generation,
                        self.world.hash_hex()
                    ),
                    false,
                ))
            }
            Err(e) => self.message = Some((format!("Could not export: {e}"), true)),
        }
    }

    /// *Export the view as PNG…*: what the grid shows now (the camera's centre and zoom), at the
    /// canvas's size in physical pixels, with the scene in its metadata as the world export has.
    pub(super) fn export_view(&mut self) {
        file_dialog::open(&self.dialog_ctx(), "scene.export_view", || {
            rfd::FileDialog::new()
                .add_filter("PNG image", &["png"])
                .set_file_name("jetcarp-view.png")
                .save_file()
        });
    }

    fn export_view_to(&mut self, path: &Path) {
        let [cw, ch] = self.canvas_px();
        let (w, h) = (cw.round().max(1.0) as u32, ch.round().max(1.0) as u32);
        let outline = if self.shell.cell_outlines {
            RenderOptions::default().outline_min_px
        } else {
            0.0
        };
        let mut meta = render::png_metadata(&self.world);
        if let Some(m) = meta.iter_mut().find(|m| m.0 == "jetcarp:scene") {
            m.1 = self.current_scene().to_json();
        }
        // As the grid draws it: a wrap-edge world is not repeated past its edges.
        let opts = render::ViewOptions {
            outline_min_px: outline,
            repeat: false,
            colors: None,
        };
        let img = render::render_view(&self.world, w, h, self.cam, self.zoom, &opts);
        let res = render::write_png(path, &img, &meta);
        self.message = Some(match res {
            Ok(_) => (
                format!(
                    "Exported the view: {} ({w}x{h}), generation {}, hash {}",
                    path.display(),
                    self.world.generation,
                    self.world.hash_hex()
                ),
                false,
            ),
            Err(e) => (format!("Could not export: {e}"), true),
        });
    }

    /// PNG export size: 2400 px on the long side, or a fixed number of pixels per tile.
    pub(super) fn export_options(&self) -> RenderOptions {
        let tile = geom::area(&self.world.topo.shapes[0]).sqrt().max(1e-9);
        RenderOptions {
            max_side: 2400,
            scale: (self.export_px > 0.0).then(|| self.export_px as f64 / tile),
            ..Default::default()
        }
    }

    /// Called every frame: asks where to save a contact sheet that is waiting for a file, and makes
    /// it once the dialog answers.
    pub(super) fn export_sheet(&mut self) {
        if let Some(answer) = file_dialog::answered::<PathBuf>("scene.sheet") {
            if let (Some(path), Some(sheet)) = (answer, self.pending_sheet.take()) {
                self.export_sheet_to(&path, sheet);
            }
            return;
        }
        if self.pending_sheet.is_some() {
            // No second dialog while one is open.
            file_dialog::open(&self.dialog_ctx(), "scene.sheet", || {
                rfd::FileDialog::new()
                    .add_filter("PNG image", &["png"])
                    .set_file_name("contact-sheet.png")
                    .save_file()
            });
        }
    }

    fn export_sheet_to(&mut self, path: &Path, (kind, first, palette, random): PendingSheet) {
        let count = if kind == jetcarp_core::sheet::SheetKind::HexBinary {
            16
        } else {
            64
        };
        let o = jetcarp_core::sheet::SheetOptions {
            kind,
            first: if kind == jetcarp_core::sheet::SheetKind::HexBinary {
                0
            } else {
                first
            },
            count,
            random_row: random.then_some(0.5),
            palette,
            ..Default::default()
        };
        let res = jetcarp_core::sheet::contact_sheet(&o).and_then(|img| {
            let meta = vec![
                ("jetcarp:version", VERSION.to_string()),
                (
                    "jetcarp:sheet",
                    format!("{kind:?} from {} count {count}", o.first),
                ),
            ];
            render::write_png(path, &img, &meta).map(|_| img)
        });
        self.message = Some(match res {
            Ok(img) => (
                format!(
                    "Contact sheet saved: {} ({}x{})",
                    path.display(),
                    img.width,
                    img.height
                ),
                false,
            ),
            Err(e) => (format!("Contact sheet failed: {e}"), true),
        });
    }

    pub(super) fn load_table(&mut self) {
        file_dialog::open(&self.dialog_ctx(), "scene.load_table", || {
            rfd::FileDialog::new()
                .add_filter("JETCARP rule table", &["json"])
                .pick_file()
        });
    }

    fn load_table_from(&mut self, path: &Path) {
        let parsed = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| {
                serde_json::from_str::<jetcarp_core::table::RuleTable>(&t)
                    .map_err(|e| e.to_string())
            });
        match parsed {
            Ok(table) => {
                let name = table.name.clone();
                if self.exec(Command::AddTable { table }).is_some()
                    && let Some(a) = self.exec(Command::AddCa {
                        ca: CaSpec::new(&format!("Table {name}")),
                    })
                {
                    self.draw_target = a.new_id;
                    self.message = Some((format!("Loaded table '{name}'."), false));
                }
            }
            Err(e) => self.message = Some((format!("Rule table: {e}"), true)),
        }
    }
}

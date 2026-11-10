//! Scenes and files: load, show, save, open, export, rule tables, pasting library patterns.

use super::*;

impl JetcarpApp {
    /// Load a scene now (AI control expects the world in place when this returns). The UI uses
    /// `load_scene_bg`, which builds large tilings in the background.
    pub(super) fn load_scene(&mut self, scene: &Scene) {
        self.cancel_build();
        match World::from_scene(scene) {
            Ok(w) => {
                self.world = w;
                self.after_scene(scene, None);
            }
            Err(e) => self.message = Some((format!("Could not load scene: {e}"), true)),
        }
    }

    /// The world of `scene` is in place: show it (`geom`: its geometry, if already built).
    pub(super) fn after_scene(&mut self, scene: &Scene, geom: Option<Geometry>) {
        self.scene_name = scene.name.clone();
        self.scene_desc = scene.description.clone();
        self.on_new_topology_with(geom);
        self.draw_target = self.world.cas.first().map(|c| c.id);
        self.playing = self.autoplay;
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
        self.message = None;
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
                at: jetcarp_core::library::PasteAt::Index { cell: cell as u32 },
                orientation: p.orientation,
                phase: 0,
            })
            .is_some()
        {
            self.message = Some((format!("Pasted {}. Esc stops pasting.", p.label), false));
        }
    }

    pub(super) fn blank_slate(&mut self) {
        self.t_family = "square".into();
        self.t_boundary = Boundary::Wrap;
        self.t_size = SizeChoice::Fill;
        let spec = self.tiling_spec_from_ui(false);
        match World::new(spec, 1) {
            Ok(w) => {
                self.world = w;
                self.scene_name = "Blank slate".into();
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

    pub(super) fn save_scene(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JETCARP scene", &["json"])
            .set_file_name("scene.json")
            .save_file()
        else {
            return;
        };
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

    pub(super) fn open_scene(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JETCARP scene", &["json"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| Scene::from_json(&t))
        {
            Ok(s) => self.load_scene_bg(&s),
            Err(e) => self.message = Some((format!("Could not open: {e}"), true)),
        }
    }

    pub(super) fn export_png(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .set_file_name("jetcarp.png")
            .save_file()
        else {
            return;
        };
        let opts = self.export_options();
        let mut meta = render::png_metadata(&self.world);
        if let Some(m) = meta.iter_mut().find(|m| m.0 == "jetcarp:scene") {
            m.1 = self.current_scene().to_json();
        }
        match render::write_png_streaming(&path, &self.world, &opts, &meta) {
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

    /// PNG export size: 2400 px on the long side, or a fixed number of pixels per tile.
    pub(super) fn export_options(&self) -> RenderOptions {
        let tile = geom::area(&self.world.topo.shapes[0]).sqrt().max(1e-9);
        RenderOptions {
            max_side: 2400,
            scale: (self.export_px > 0.0).then(|| self.export_px as f64 / tile),
            ..Default::default()
        }
    }

    pub(super) fn export_sheet(&mut self) {
        let Some((kind, first, palette, random)) = self.pending_sheet.take() else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG image", &["png"])
            .set_file_name("contact-sheet.png")
            .save_file()
        else {
            return;
        };
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
            render::write_png(&path, &img, &meta).map(|_| img)
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
        let Some(path) = rfd::FileDialog::new()
            .add_filter("JETCARP rule table", &["json"])
            .pick_file()
        else {
            return;
        };
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| {
                serde_json::from_str::<jetcarp_core::table::RuleTable>(&t).map_err(|e| e.to_string())
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

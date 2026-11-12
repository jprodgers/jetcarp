//! Tool windows: Scripts, Results, Library, Catalog tools, Ants, Sandpile, HashLife view, Hunts,
//! palettes, editors, AI control, help. Each can float or pop out (`crate::popout`).

use super::*;

impl JetcarpApp {
    /// The screen scripts fill (`size: "screen"`): the canvas in pixels at the cell size.
    pub(super) fn script_screen(&self, _ppp: f32) -> jetcarp_script::Screen {
        let c = self.canvas_px();
        jetcarp_script::Screen {
            width: (c[0].round() as u32).max(16),
            height: (c[1].round() as u32).max(16),
            cell_px: self.shell.display.cell_px as f64,
        }
    }

    /// The Scripts window, the results browser, and the library (every frame, on both screens).
    pub(super) fn script_windows(&mut self, ctx: &egui::Context) {
        use crate::{results, scripts_ui};
        let mut acts = self.scripts.poll(ctx);
        let host = scripts_ui::Host {
            screen: self.script_screen(ctx.pixels_per_point()),
            world: &self.world,
        };
        acts.extend(self.scripts.show(ctx, &host));
        for a in acts {
            match a {
                scripts_ui::Action::ShowWorld(w, title) => {
                    self.show_world(*w, title, "Shown by a script.".into())
                }
                scripts_ui::Action::Browse(f) => self.results.open_folder(&f),
                scripts_ui::Action::Message(m, err) => {
                    // A job ended: it may have promoted finds or added patterns.
                    self.message = Some((m, err));
                    self.refresh_finds();
                    if self.library.open {
                        self.library.reload();
                    }
                }
            }
        }
        let root = self.out_root.clone();
        for a in self.results.show(ctx, &root) {
            match a {
                results::Action::Open { world, title, desc } => {
                    self.message = Some((
                        format!(
                            "Opened {title} at generation {}. R restarts it, Space runs it.",
                            world.generation
                        ),
                        false,
                    ));
                    self.show_world(*world, title, desc);
                }
                results::Action::Message(m, err) => self.message = Some((m, err)),
                results::Action::LibraryChanged => self.library.reload(),
                results::Action::QuickStartChanged => self.refresh_finds(),
            }
        }
        for a in self.library.show(ctx) {
            use crate::library_ui::Action as L;
            match a {
                L::Arm(p) => {
                    self.message = Some((
                        format!(
                            "Click a cell in the view to paste {} (more clicks paste more; Esc stops).",
                            p.label
                        ),
                        false,
                    ));
                    self.paste = Some(*p);
                }
                L::Open(sc) => self.load_scene_bg(&sc),
                L::QuickStartChanged => self.refresh_finds(),
                L::Message(m, err) => self.message = Some((m, err)),
            }
        }
    }

    /// The Catalog tools and Ants windows (`catalog_tools_ui.rs`, `ants_ui.rs`).
    pub(super) fn catalog_windows(&mut self, ctx: &egui::Context) {
        let selected = self.library.selected_id();
        for a in self.catalog_tools.show(ctx, selected) {
            use crate::catalog_tools_ui::Action as C;
            match a {
                C::Open(sc) => self.load_scene_bg(&sc),
                C::LibraryChanged => self.library.reload(),
                C::Message(m, err) => self.message = Some((m, err)),
            }
        }
        for a in self.ants.show(ctx) {
            use crate::ants_ui::Action as A;
            match a {
                A::Open(sc) => {
                    self.screen = Screen::Sim;
                    self.load_scene_bg(&sc);
                }
                A::Message(m, err) => self.message = Some((m, err)),
            }
        }
    }

    /// Rebuild the Quick Start list when the catalog's Quick Start entries changed (a promotion
    /// here, from a script, MCP, or the CLI).
    pub(super) fn refresh_finds(&mut self) {
        self.menu.refresh_if_changed();
    }

    /// The setup wizard (`crate::setup_ui`). Once a setup chose a new data folder, the windows
    /// that hold the library open it again from there.
    pub(super) fn setup_window(&mut self, ctx: &egui::Context) {
        self.setup.show(ctx);
        if self.setup.take_data_changed() {
            let palettes = self.tool_palettes();
            self.menu = start::Menu::new();
            self.dash = Default::default();
            self.library = Default::default();
            self.ants = Default::default();
            self.catalog_tools = Default::default();
            self.pile = Default::default();
            self.set_tool_palettes(palettes);
        }
    }

    /// Every tool window (every frame, on both screens).
    pub(super) fn tool_windows(&mut self, ctx: &egui::Context) {
        self.setup_window(ctx);
        self.script_windows(ctx);
        self.pile.show(ctx);
        self.hlview.show(ctx);
        self.dash.show(ctx);
        self.catalog_windows(ctx);
        self.shortcuts_window(ctx);
        self.about_window(ctx);
        self.reseed_tick(ctx);
        if self.screen != Screen::Sim {
            self.end_palette_preview();
            return;
        }
        self.palette_window(ctx);
        self.wallpaper_window(ctx);
        self.ai_window(ctx);
        for a in self
            .editor
            .show(ctx)
            .into_iter()
            .chain(self.cell_editor.show(ctx))
        {
            match a {
                crate::editor::EditorAction::Apply(mut spec) => {
                    let ui_spec = {
                        self.t_family = spec.family.clone();
                        self.tiling_spec_from_ui(false)
                    };
                    spec.size = ui_spec.size;
                    spec.boundary = ui_spec.boundary;
                    if self.exec(Command::SetTiling { tiling: spec }).is_some() {
                        self.message =
                            Some(("Tiling rebuilt from the editor's tile.".into(), false));
                    }
                }
                crate::editor::EditorAction::Message(m, err) => self.message = Some((m, err)),
            }
        }
    }

    /// Each tool window's palette choice (for the settings file).
    pub(super) fn tool_palettes(&self) -> crate::palette_pick::ToolPalettes {
        crate::palette_pick::ToolPalettes {
            sandpile: self.pile.palette.clone(),
            sandpile_reverse: self.pile.reverse,
            ants: self.ants.palette.clone(),
            hashlife: self.hlview.palette.clone(),
            library: self.library.palette.clone(),
            hunts: self.dash.palette.clone(),
        }
    }

    /// Put saved palette choices back in the tool windows (at start).
    pub(super) fn set_tool_palettes(&mut self, p: crate::palette_pick::ToolPalettes) {
        let p = p.sanitize();
        self.pile.palette = p.sandpile;
        self.pile.reverse = p.sandpile_reverse;
        self.ants.palette = p.ants;
        self.hlview.palette = p.hashlife;
        self.library.palette = p.library;
        self.dash.palette = p.hunts;
    }

    /// `JETCARP_TOOL_PALETTE=<name>` (headless checks): every tool window in that palette;
    /// `JETCARP_PILE_REVERSE=1` also reverses the sandpile's.
    pub(super) fn tool_palette_env(&mut self) {
        if let Ok(n) = std::env::var("JETCARP_TOOL_PALETTE") {
            let o = Some(n.clone());
            self.set_tool_palettes(crate::palette_pick::ToolPalettes {
                sandpile: n,
                sandpile_reverse: std::env::var("JETCARP_PILE_REVERSE").is_ok(),
                ants: o.clone(),
                hashlife: o.clone(),
                library: o.clone(),
                hunts: o,
            });
        }
    }
}

//! Quick Start: the browser for everything the app can open (`jetcarp_core::qsmenu`: the atlas of
//! search results, "My finds", and the classics).
//!
//! A sidebar filters by category, tiling group, rule family, and source (with counts); the main
//! area is a search box, the sort, and a virtual grid (only rows on screen are laid out). The
//! default view is a seeded shuffle of the best entries across all categories. Everything per
//! frame is proportional to the cards on screen: the filter, the sort, and the counts are
//! cached and recomputed only when they change. Live previews run on background threads for
//! the cards on screen only (see [`crate::previews`]), starting from each entry's mature state.
//!
//! The menu loads on a background thread and reloads when the catalog's Quick Start rows or the
//! atlas file change.

use crate::previews::{MAX_LIVE, Pool, Source};
use crate::thumbs;
use eframe::egui::{self, Color32, Rect, RichText, Sense};
use jetcarp_core::Scene;
use jetcarp_core::explore::atlas;
use jetcarp_core::library::Library;
use jetcarp_core::qsmenu::{self, Facets, Filter, Item, Origin, Sort};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

/// Cards in each opening row.
pub const ROW_LEN: usize = 12;

/// Sort and filter state of the menu.
pub struct View {
    pub filter: Filter,
    pub sort: Sort,
    pub card_w: f32,
    pub live: bool,
}

impl Default for View {
    fn default() -> Self {
        View {
            filter: Filter::default(),
            sort: Sort::Featured,
            card_w: 216.0,
            live: true,
        }
    }
}

/// What a frame of the menu asked for.
pub enum MenuAction {
    Open(usize),
    /// Open at preview size (Shift-click), when the entry has one.
    OpenPreview(usize),
}

/// The Quick Start menu.
pub struct Menu {
    pub items: Vec<Item>,
    pub view: View,
    loading: Option<Receiver<Vec<Item>>>,
    /// The catalog's Quick Start stamp the entries were loaded at, and the atlas file's.
    stamp: String,
    atlas_stamp: Option<(PathBuf, u64, Option<std::time::SystemTime>)>,
    seed: u64,
    order: Vec<usize>,
    order_key: Option<(Filter, Sort, u64)>,
    scroll_key: (Filter, Sort),
    scroll_top: bool,
    popular: Vec<usize>,
    recent: Vec<usize>,
    random: Vec<usize>,
    /// Bumped when entries, usage, or the shuffle change.
    rev: u64,
    pool: Pool,
    clock: Instant,
    last_check: f64,
    lib: Library,
    facets: Option<(u64, Filter, std::sync::Arc<Facets>)>,
}

fn stamp(lib: &Library) -> String {
    lib.quickstart_stamp()
}

fn atlas_stamp() -> Option<(PathBuf, u64, Option<std::time::SystemTime>)> {
    let p = atlas::locate()?;
    let m = std::fs::metadata(&p).ok()?;
    Some((p, m.len(), m.modified().ok()))
}

fn source_of(it: &Item) -> Source {
    match &it.atlas {
        Some(a) => Source::Atlas {
            path: a.path.clone(),
            id: a.id,
        },
        None => Source::Scene(Box::new(it.q.scene.clone())),
    }
}

/// Quietly tinted colour of a category.
fn category_color(cat: &str) -> Color32 {
    match cat {
        "Gliders & ships" => Color32::from_rgb(0xff, 0xc8, 0x3c),
        "Waves & spirals" => Color32::from_rgb(0x58, 0xc8, 0xff),
        "Oscillators" => Color32::from_rgb(0xff, 0x8a, 0xc8),
        "Turbulence" => Color32::from_rgb(0xff, 0x70, 0x50),
        "Living textures" => Color32::from_rgb(0x7c, 0xe0, 0x90),
        "Labyrinths" => Color32::from_rgb(0xb4, 0xa0, 0xff),
        "Growth" => Color32::from_rgb(0x90, 0xd0, 0x70),
        "Fractals & rows" => Color32::from_rgb(0x58, 0xe0, 0xc2),
        "Battles" => Color32::from_rgb(0xff, 0xa0, 0x60),
        "Ants & turmites" => Color32::from_rgb(0xff, 0xb0, 0xb0),
        "Sandpiles" => Color32::from_rgb(0xe0, 0xc0, 0x90),
        "My finds" => Color32::from_rgb(0xff, 0xc8, 0x3c),
        _ => Color32::from_gray(150),
    }
}

impl Menu {
    /// The built-in entries now, the catalog's and the atlas's on a background thread. The
    /// Featured shuffle's seed is `JETCARP_QS_SEED`, else drawn from the clock.
    pub fn new() -> Menu {
        let seed = std::env::var("JETCARP_QS_SEED")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(1)
            });
        let lib = Library::open_default();
        let mut items = qsmenu::built_in_items();
        qsmenu::shuffle(&mut items, seed);
        let mut m = Menu {
            items,
            view: View::default(),
            loading: None,
            stamp: String::new(),
            atlas_stamp: None,
            seed,
            order: vec![],
            order_key: None,
            scroll_key: (Filter::default(), Sort::Featured),
            scroll_top: false,
            popular: vec![],
            recent: vec![],
            random: vec![],
            rev: 0,
            pool: Pool::new(),
            clock: Instant::now(),
            last_check: 0.0,
            lib,
            facets: None,
        };
        m.rows();
        m.reload();
        m
    }

    fn now(&self) -> f64 {
        self.clock.elapsed().as_secs_f64()
    }

    /// Load the catalog and the atlas again in the background.
    pub fn reload(&mut self) {
        let (tx, rx) = channel();
        let path = self.lib.path().to_path_buf();
        let seed = self.seed;
        std::thread::Builder::new()
            .name("jetcarp-qs-load".into())
            .spawn(move || {
                let lib = Library::open(path);
                let mut items = qsmenu::load(&lib);
                qsmenu::shuffle(&mut items, seed);
                let _ = tx.send(items);
            })
            .ok();
        self.loading = Some(rx);
        self.stamp = stamp(&self.lib);
        self.atlas_stamp = atlas_stamp();
    }

    /// Reload when the catalog's Quick Start rows or the atlas file changed; forget previews
    /// off screen for a while. Called every frame on every screen; checks about once a second.
    pub fn refresh_if_changed(&mut self) {
        let now = self.now();
        self.pool.forget(now);
        if now - self.last_check < 1.0 {
            return;
        }
        self.last_check = now;
        let a = atlas_stamp();
        if a != self.atlas_stamp {
            self.pool.clear();
            self.reload();
        } else if stamp(&self.lib) != self.stamp {
            self.reload();
        }
    }

    fn poll(&mut self) {
        let Some(rx) = &self.loading else { return };
        if let Ok(items) = rx.try_recv() {
            self.loading = None;
            self.items = items;
            self.rev += 1;
            self.order_key = None;
            self.rows();
        }
    }

    fn rows(&mut self) {
        self.popular = qsmenu::popular(&self.items, ROW_LEN);
        self.recent = qsmenu::recent(&self.items, ROW_LEN);
        self.random = qsmenu::random(&self.items, self.seed, ROW_LEN);
    }

    /// Entry index by id.
    pub fn find(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|e| e.q.id == id)
    }

    /// The full scene of an entry: what opening it loads.
    pub fn full_scene(&self, i: usize) -> Result<Scene, String> {
        let it = &self.items[i];
        match &it.atlas {
            Some(a) => atlas::load_full(&a.path, a.id),
            None => Ok(it.q.scene.clone()),
        }
    }

    /// The small scene an entry's card runs, for "open at preview size": `None` when the card
    /// runs the full scene.
    pub fn preview_scene(&self, i: usize) -> Option<Scene> {
        let a = self.items[i].atlas.as_ref()?;
        let conn = atlas::open(&a.path).ok()?;
        atlas::load_card(&conn, a.id).ok().map(|c| c.preview)
    }

    /// A scene by entry id; catalog entries not loaded yet are read directly.
    pub fn scene_of(&self, id: &str) -> Option<Scene> {
        if let Some(i) = self.find(id) {
            return self.full_scene(i).ok();
        }
        if let Some(n) = id.strip_prefix("atlas:") {
            let path = atlas::locate()?;
            return atlas::load_full(&path, n.parse().ok()?).ok();
        }
        let bare = id.strip_prefix("find:")?;
        self.lib.get_quickstart(bare).ok().map(|q| q.scene)
    }

    /// Count an opening: in memory now (Popular and Recently run follow), in the catalog on a
    /// background thread.
    pub fn note_open(&mut self, i: usize) {
        let id = self.items[i].q.id.clone();
        qsmenu::note_open(&mut self.items, &id, &jetcarp_core::library::now_utc());
        self.rev += 1;
        self.order_key = None;
        self.popular = qsmenu::popular(&self.items, ROW_LEN);
        self.recent = qsmenu::recent(&self.items, ROW_LEN);
        let path = self.lib.path().to_path_buf();
        std::thread::spawn(move || {
            if let Err(e) = Library::open(path).note_quickstart_open(&id) {
                eprintln!("Quick Start usage: {e}");
            }
        });
    }

    /// The order the grid shows, recomputed only when the filter, the sort, or the entries
    /// change.
    fn update_order(&mut self) {
        let key = (self.view.filter.clone(), self.view.sort, self.rev);
        if self.order_key.as_ref() != Some(&key) {
            self.order = qsmenu::select(&self.items, &self.view.filter, self.view.sort);
            self.order_key = Some(key);
        }
        let k = (self.view.filter.clone(), self.view.sort);
        if self.scroll_key != k {
            self.scroll_key = k;
            self.scroll_top = true;
        }
    }

    fn update_facets(&mut self) {
        if self
            .facets
            .as_ref()
            .is_none_or(|f| f.0 != self.rev || f.1 != self.view.filter)
        {
            self.facets = Some((
                self.rev,
                self.view.filter.clone(),
                std::sync::Arc::new(qsmenu::facets_for(&self.items, &self.view.filter)),
            ));
        }
    }

    /// A new Featured shuffle.
    fn shuffle(&mut self) {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        qsmenu::shuffle(&mut self.items, self.seed);
        self.random = qsmenu::random(&self.items, self.seed, ROW_LEN);
        self.rev += 1;
        self.order_key = None;
        self.scroll_top = true;
    }

    /// The whole Quick Start screen below the header. Returns the entry to open.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<MenuAction> {
        self.poll();
        let ctx = ui.ctx().clone();
        let now = self.now();
        self.pool.begin_frame(&ctx, now);
        self.update_order();
        self.update_facets();
        let mut act = None;
        egui::Panel::left("qs-sidebar")
            .resizable(false)
            .exact_size(226.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(6, 4)))
            .show(ui, |ui| self.sidebar(ui));
        let mut side = 240;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)))
            .show(ui, |ui| {
                self.top_bar(ui);
                self.update_order();
                self.strip(ui, now, &mut act);
                self.grid(ui, now, &mut act, &mut side);
            });
        self.pool.end_frame(self.view.live, side);
        ctx.request_repaint_after(std::time::Duration::from_millis(40));
        act
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let Some(f) = self.facets.as_ref().map(|f| f.2.clone()) else {
            return;
        };
        let filter = &mut self.view.filter;
        let header = |ui: &mut egui::Ui, t: &str| {
            ui.add_space(8.0);
            ui.label(
                RichText::new(t.to_uppercase())
                    .size(11.0)
                    .strong()
                    .color(Color32::from_gray(150)),
            );
            ui.add_space(2.0);
        };
        egui::ScrollArea::vertical()
            .id_salt("qs-side-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add_space(2.0);
                header(ui, "Categories");
                if facet_row(ui, "All", f.all, filter.behaviour.is_empty(), None) {
                    filter.behaviour.clear();
                }
                for (name, n) in &f.categories {
                    let sel = filter.behaviour == *name;
                    if facet_row(ui, name, *n, sel, Some(category_color(name))) {
                        filter.behaviour = if sel { String::new() } else { name.clone() };
                    }
                }
                if f.origins.len() > 1 || filter.origin.is_some() {
                    header(ui, "Source");
                    for (o, n) in &f.origins {
                        let sel = filter.origin == Some(*o);
                        if facet_row(ui, o.label(), *n, sel, None) {
                            filter.origin = if sel { None } else { Some(*o) };
                        }
                    }
                }
                header(ui, "Tilings");
                for (name, n) in &f.grids {
                    let sel = filter.grid == *name;
                    if facet_row(ui, name, *n, sel, None) {
                        filter.grid = if sel { String::new() } else { name.clone() };
                    }
                }
                header(ui, "Rule families");
                for (name, n) in &f.families {
                    let sel = filter.family == *name;
                    if facet_row(ui, name, *n, sel, None) {
                        filter.family = if sel { String::new() } else { name.clone() };
                    }
                }
                ui.add_space(8.0);
            });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let total = self.items.len();
        let shown = self.order.len();
        let loading = self.loading.is_some();
        let mut shuffle = false;
        ui.horizontal(|ui| {
            let v = &mut self.view;
            let edit = ui.add(
                egui::TextEdit::singleline(&mut v.filter.text)
                    .hint_text("Search name, rule, tiling, family, category, seed")
                    .desired_width(320.0),
            );
            let _ = edit;
            if !v.filter.text.is_empty() && ui.small_button("x").clicked() {
                v.filter.text.clear();
            }
            ui.separator();
            ui.label("Sort");
            egui::ComboBox::from_id_salt("qs-sort")
                .selected_text(v.sort.label())
                .show_ui(ui, |ui| {
                    for (s, label) in Sort::ALL {
                        ui.selectable_value(&mut v.sort, s, label);
                    }
                });
            if ui
                .button("Shuffle")
                .on_hover_text("Draw another selection of the best entries")
                .clicked()
            {
                v.sort = Sort::Featured;
                shuffle = true;
            }
            ui.separator();
            ui.label("Cards");
            ui.add(egui::Slider::new(&mut v.card_w, 140.0..=340.0).show_value(false));
            ui.checkbox(&mut v.live, "Live").on_hover_text(format!(
                "Run the cards on screen (up to {MAX_LIVE} at once)"
            ));
            if !v.filter.is_empty()
                && ui
                    .button("Clear")
                    .on_hover_text("Clear the search and every category")
                    .clicked()
            {
                v.filter = Filter::default();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let text = if loading {
                    "loading…".to_string()
                } else if shown == total {
                    format!("{total} entries")
                } else {
                    format!("{shown} of {total} entries")
                };
                ui.label(RichText::new(text).weak());
            });
        });
        if shuffle {
            self.shuffle();
        }
    }

    /// The rows (Popular, Recently run, Random): a strip, collapsed by default.
    fn strip(&mut self, ui: &mut egui::Ui, now: f64, act: &mut Option<MenuAction>) {
        let rows = [
            ("Popular", self.popular.clone()),
            ("Recently run", self.recent.clone()),
            ("Random", self.random.clone()),
        ];
        egui::CollapsingHeader::new("Popular, recent, random")
            .id_salt("qs-strip")
            .default_open(false)
            .show(ui, |ui| {
                for (title, idx) in rows {
                    if idx.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(title).strong());
                    let w = 168.0;
                    let h = (w * 0.62f32).round();
                    egui::ScrollArea::horizontal()
                        .id_salt(("qs-row", title))
                        .max_height(h + 62.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for &i in &idx {
                                    if let Some(a) = self.card(ui, i, w, h, now) {
                                        *act = Some(a);
                                    }
                                }
                            });
                        });
                }
            });
    }

    fn grid(&mut self, ui: &mut egui::Ui, now: f64, act: &mut Option<MenuAction>, side: &mut u32) {
        ui.add_space(4.0);
        if self.order.is_empty() {
            thumbs::empty_note(ui, "Nothing matches. Clear the search or a category.");
            return;
        }
        let order = std::mem::take(&mut self.order);
        let gap = 10.0;
        let avail = ui.available_width() - 14.0;
        let cols = (((avail + gap) / (self.view.card_w + gap)).floor() as usize).max(1);
        let cw = ((avail + gap) / cols as f32 - gap).max(80.0);
        let img_h = (cw * 0.62).round();
        let row_h = img_h + CAPTION_H + 8.0;
        let rows = order.len().div_ceil(cols);
        *side = ((cw * ui.ctx().pixels_per_point() / 20.0).ceil() as u32 * 20).clamp(120, 480);
        let mut area = egui::ScrollArea::vertical()
            .id_salt("qs-grid")
            .auto_shrink([false, false]);
        if self.scroll_top {
            area = area.vertical_scroll_offset(0.0);
            self.scroll_top = false;
        }
        area.show_rows(ui, row_h, rows, |ui, range| {
            for r in range {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for c in 0..cols {
                        let Some(&i) = order.get(r * cols + c) else {
                            break;
                        };
                        if let Some(a) = self.card(ui, i, cw, img_h, now) {
                            *act = Some(a);
                        }
                    }
                });
            }
        });
        self.order = order;
    }

    /// One card; the action when clicked.
    fn card(
        &mut self,
        ui: &mut egui::Ui,
        i: usize,
        w: f32,
        img_h: f32,
        now: f64,
    ) -> Option<MenuAction> {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, img_h + CAPTION_H), Sense::click());
        if !ui.is_rect_visible(rect) {
            return None;
        }
        let it = &self.items[i];
        self.pool.see(it.id(), now, || source_of(it));
        let p = ui.painter_at(rect);
        let hovered = resp.hovered();
        let cat = category_color(&it.behaviour);
        p.rect_filled(
            rect,
            6.0,
            if hovered {
                Color32::from_gray(40)
            } else {
                Color32::from_gray(26)
            },
        );
        let img_rect =
            Rect::from_min_size(rect.min + egui::vec2(3.0, 3.0), egui::vec2(w - 6.0, img_h));
        p.rect_filled(img_rect, 4.0, Color32::from_rgb(0x12, 0x14, 0x19));
        if let Some(t) = self.pool.texture(it.id()) {
            let (src_w, src_h) = (t.size_vec2().x, t.size_vec2().y);
            // Crop to fill the card's picture area.
            let (want, have) = (img_rect.width() / img_rect.height(), src_w / src_h);
            let uv = if have > want {
                let f = want / have;
                Rect::from_min_max(
                    egui::pos2(0.5 - 0.5 * f, 0.0),
                    egui::pos2(0.5 + 0.5 * f, 1.0),
                )
            } else {
                let f = have / want;
                Rect::from_min_max(
                    egui::pos2(0.0, 0.5 - 0.5 * f),
                    egui::pos2(1.0, 0.5 + 0.5 * f),
                )
            };
            p.image(t.id(), img_rect, uv, Color32::WHITE);
        } else {
            p.text(
                img_rect.center(),
                egui::Align2::CENTER_CENTER,
                "…",
                egui::FontId::proportional(14.0),
                Color32::from_gray(90),
            );
        }
        if hovered {
            p.rect_stroke(
                img_rect,
                4.0,
                egui::Stroke::new(1.5, cat),
                egui::StrokeKind::Outside,
            );
        }
        if it.origin != Origin::Atlas {
            let tag = it.origin.label();
            let g = p.layout_no_wrap(tag.into(), egui::FontId::proportional(10.0), Color32::BLACK);
            let r = Rect::from_min_size(
                img_rect.min + egui::vec2(4.0, 4.0),
                g.size() + egui::vec2(8.0, 2.0),
            );
            p.rect_filled(
                r,
                3.0,
                if it.origin == Origin::MyFinds {
                    Color32::from_rgb(0xff, 0xc8, 0x3c)
                } else {
                    Color32::from_gray(190)
                },
            );
            p.galley(r.min + egui::vec2(4.0, 1.0), g, Color32::BLACK);
        }
        let ty = img_rect.bottom() + 5.0;
        let clip = p.with_clip_rect(Rect::from_min_max(
            egui::pos2(rect.left() + 4.0, ty - 2.0),
            egui::pos2(rect.right() - 4.0, rect.bottom()),
        ));
        let text_x = rect.left() + 7.0;
        clip.text(
            egui::pos2(text_x, ty),
            egui::Align2::LEFT_TOP,
            &it.title,
            egui::FontId::proportional(13.5),
            Color32::from_gray(235),
        );
        clip.text(
            egui::pos2(text_x, ty + 18.0),
            egui::Align2::LEFT_TOP,
            &it.q.rule,
            egui::FontId::monospace(10.5),
            Color32::from_gray(150),
        );
        // Category first, in its colour: the tiling name can be long and clip.
        let g = clip.layout_no_wrap(it.behaviour.clone(), egui::FontId::proportional(11.0), cat);
        let wide = g.size().x;
        clip.galley(egui::pos2(text_x, ty + 33.0), g, Color32::WHITE);
        clip.text(
            egui::pos2(text_x + wide, ty + 33.0),
            egui::Align2::LEFT_TOP,
            format!(" · {}", it.q.tiling),
            egui::FontId::proportional(11.0),
            Color32::from_gray(135),
        );
        if let Some(r) = it.rating {
            let n = r.round() as usize;
            for k in 0..5 {
                thumbs::star(
                    &p,
                    egui::pos2(
                        rect.right() - 12.0 - 13.0 * (4 - k) as f32,
                        img_rect.bottom() - 9.0,
                    ),
                    5.5,
                    k < n,
                    Color32::from_rgb(0xff, 0xc8, 0x3c),
                );
            }
        }
        let clicked = resp.clicked();
        let shift = ui.input(|i| i.modifiers.shift);
        let mut action = None;
        if clicked {
            action = Some(if shift && it.atlas.is_some() {
                MenuAction::OpenPreview(i)
            } else {
                MenuAction::Open(i)
            });
        }
        let mut tip = format!("{}\n{}", it.title, it.description);
        if it.opens > 0 {
            tip.push_str(&format!("\n\nOpened {} times", it.opens));
        }
        if let Some(src) = &it.q.source {
            tip.push_str(&format!("\n\nSource: {src}"));
        }
        if it.atlas.is_some() {
            tip.push_str("\n\nClick to open at full size; Shift-click at preview size.");
        }
        resp.on_hover_text(tip);
        action
    }
}

/// Height of a card's caption (title, rule, tiling and category).
const CAPTION_H: f32 = 58.0;

/// One line of the sidebar; true when clicked.
fn facet_row(
    ui: &mut egui::Ui,
    label: &str,
    count: usize,
    selected: bool,
    color: Option<Color32>,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 19.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return false;
    }
    let p = ui.painter_at(rect);
    if selected {
        p.rect_filled(rect, 3.0, Color32::from_rgb(0x34, 0x3c, 0x50));
    } else if resp.hovered() {
        p.rect_filled(rect, 3.0, Color32::from_gray(38));
    }
    let mut x = rect.left() + 5.0;
    if let Some(c) = color {
        p.circle_filled(egui::pos2(x + 3.0, rect.center().y), 3.0, c);
        x += 10.0;
    }
    p.text(
        egui::pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        if selected {
            Color32::WHITE
        } else {
            Color32::from_gray(205)
        },
    );
    p.text(
        egui::pos2(rect.right() - 6.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        count.to_string(),
        egui::FontId::proportional(11.5),
        Color32::from_gray(130),
    );
    resp.clicked()
}

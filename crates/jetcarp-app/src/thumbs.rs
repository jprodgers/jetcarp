//! Thumbnails for large grids: sweep results (PNG files) and the pattern library (pictures
//! rendered from records).
//!
//! [`ThumbCache`] decodes or renders and shrinks images on background threads, newest request
//! first, and keeps a bounded number of textures (least recently drawn are dropped). Requests
//! that are no longer on screen are dropped at the start of each frame, so fast scrolling through
//! thousands of images only loads what stops on screen. [`grid`] draws a virtual grid: only the
//! rows in view are laid out.

use eframe::egui::{
    self, Color32, ColorImage, Rect, RichText, Sense, TextureHandle, TextureOptions,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};

/// Longest side of a decoded thumbnail, in pixels.
pub const THUMB_SIDE: u32 = 360;

/// Makes a thumbnail image (on a worker thread).
pub type RenderFn = Arc<dyn Fn() -> Result<ColorImage, String> + Send + Sync>;

/// Where a thumbnail comes from.
#[derive(Clone)]
pub enum Loader {
    Png(PathBuf),
    Render(RenderFn),
}

/// A thumbnail: a cache key and how to make it.
#[derive(Clone)]
pub struct Thumb {
    pub key: String,
    pub load: Loader,
}

impl Thumb {
    pub fn png(path: PathBuf) -> Thumb {
        Thumb {
            key: path.display().to_string(),
            load: Loader::Png(path),
        }
    }
    pub fn render(key: String, f: RenderFn) -> Thumb {
        Thumb {
            key,
            load: Loader::Render(f),
        }
    }
}

struct Queue {
    items: Mutex<Vec<Thumb>>,
    ready: Condvar,
}

pub struct ThumbCache {
    textures: HashMap<String, (TextureHandle, u64)>,
    /// Requested and not back yet (queued or being made).
    pending: HashSet<String>,
    failed: HashMap<String, String>,
    queue: Arc<Queue>,
    done_rx: Receiver<(String, Result<ColorImage, String>)>,
    frame: u64,
    pub capacity: usize,
}

impl ThumbCache {
    pub fn new(threads: usize) -> ThumbCache {
        let queue = Arc::new(Queue {
            items: Mutex::new(vec![]),
            ready: Condvar::new(),
        });
        let (tx, rx) = channel();
        for k in 0..threads.max(1) {
            let q = queue.clone();
            let tx: Sender<_> = tx.clone();
            let _ = std::thread::Builder::new()
                .name(format!("jetcarp-thumbs-{k}"))
                .spawn(move || {
                    loop {
                        let t = {
                            let mut items = q.items.lock().unwrap();
                            loop {
                                if let Some(p) = items.pop() {
                                    break p;
                                }
                                items = q.ready.wait(items).unwrap();
                            }
                        };
                        let img = match &t.load {
                            Loader::Png(p) => load_thumb(p, THUMB_SIDE),
                            Loader::Render(f) => f(),
                        };
                        if tx.send((t.key, img)).is_err() {
                            return;
                        }
                    }
                });
        }
        ThumbCache {
            textures: HashMap::new(),
            pending: HashSet::new(),
            failed: HashMap::new(),
            queue,
            done_rx: rx,
            frame: 0,
            capacity: 800,
        }
    }

    /// Call once per frame before drawing: drops stale requests, uploads finished thumbnails,
    /// and trims the cache. Returns true while work is in flight (keep repainting).
    pub fn begin_frame(&mut self, ctx: &egui::Context) -> bool {
        self.frame += 1;
        {
            let mut q = self.queue.items.lock().unwrap();
            for t in q.drain(..) {
                self.pending.remove(&t.key);
            }
        }
        let mut uploaded = 0;
        while uploaded < 48 {
            let Ok((key, r)) = self.done_rx.try_recv() else {
                break;
            };
            self.pending.remove(&key);
            match r {
                Ok(img) => {
                    let t = ctx.load_texture(key.clone(), img, TextureOptions::LINEAR);
                    self.textures.insert(key, (t, self.frame));
                    uploaded += 1;
                }
                Err(e) => {
                    self.failed.insert(key, e);
                }
            }
        }
        if self.textures.len() > self.capacity {
            let mut by_age: Vec<(u64, String)> = self
                .textures
                .iter()
                .map(|(p, (_, f))| (*f, p.clone()))
                .collect();
            by_age.sort();
            let drop = self.textures.len() - self.capacity;
            for (_, p) in by_age.into_iter().take(drop) {
                self.textures.remove(&p);
            }
        }
        !self.pending.is_empty()
    }

    /// The thumbnail if it is ready; otherwise it is requested.
    pub fn get(&mut self, t: &Thumb) -> Option<&TextureHandle> {
        if let Some((_, f)) = self.textures.get_mut(&t.key) {
            *f = self.frame;
            return self.textures.get(&t.key).map(|x| &x.0);
        }
        if self.failed.contains_key(&t.key) || self.pending.contains(&t.key) {
            return None;
        }
        self.pending.insert(t.key.clone());
        self.queue.items.lock().unwrap().push(t.clone());
        self.queue.ready.notify_one();
        None
    }

    pub fn error(&self, key: &str) -> Option<&str> {
        self.failed.get(key).map(String::as_str)
    }

    pub fn clear(&mut self) {
        self.textures.clear();
        self.failed.clear();
    }
}

/// A jetcarp-core image as an egui image, shrunk to fit `side` pixels.
pub fn color_image(img: &jetcarp_core::render::Image, side: u32) -> ColorImage {
    let (w, h) = (img.width as usize, img.height as usize);
    shrink(w, h, side, |x, y| {
        let i = (y * w + x) * 4;
        [
            img.rgba[i],
            img.rgba[i + 1],
            img.rgba[i + 2],
            img.rgba[i + 3],
        ]
    })
}

/// Box-filter an image of `w` x `h` pixels down to fit `side` (never enlarged).
fn shrink(w: usize, h: usize, side: u32, px: impl Fn(usize, usize) -> [u8; 4]) -> ColorImage {
    let k = (w.max(h) as f64 / side as f64).max(1.0);
    let (tw, th) = (
        ((w as f64 / k).round() as usize).max(1),
        ((h as f64 / k).round() as usize).max(1),
    );
    let mut out = Vec::with_capacity(tw * th * 4);
    for ty in 0..th {
        let y0 = ty * h / th;
        let y1 = ((ty + 1) * h / th).max(y0 + 1);
        for tx in 0..tw {
            let x0 = tx * w / tw;
            let x1 = ((tx + 1) * w / tw).max(x0 + 1);
            // Box filter (fine lines stay visible as a blend instead of vanishing).
            let mut acc = [0u32; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = px(x, y);
                    for c in 0..4 {
                        acc[c] += p[c] as u32;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            out.extend(acc.iter().map(|a| (a / n) as u8));
        }
    }
    ColorImage::from_rgba_unmultiplied([tw, th], &out)
}

/// Decode a PNG (any colour type, indexed included) and shrink it to fit `side` pixels.
pub fn load_thumb(path: &Path, side: u32) -> Result<ColorImage, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut dec = png::Decoder::new(std::io::BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("image too large")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width as usize, info.height as usize);
    let ch = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Err("indexed PNG was not expanded".into()),
    };
    let px = |x: usize, y: usize| -> [u8; 4] {
        let i = (y * w + x) * ch;
        let s = &buf[i..i + ch];
        match ch {
            1 => [s[0], s[0], s[0], 255],
            2 => [s[0], s[0], s[0], s[1]],
            3 => [s[0], s[1], s[2], 255],
            _ => [s[0], s[1], s[2], s[3]],
        }
    };
    Ok(shrink(w, h, side, px))
}

/// One cell of a [`grid`].
pub struct Cell {
    pub image: Option<Thumb>,
    /// Shown when there is no image.
    pub placeholder: String,
    pub title: String,
    pub subtitle: String,
    /// `Some(on)` draws a star that can be toggled.
    pub star: Option<bool>,
    pub selected: bool,
}

#[derive(Default)]
pub struct GridResponse {
    pub clicked: Option<usize>,
    pub double_clicked: Option<usize>,
    pub star_toggled: Option<usize>,
    pub hovered: Option<usize>,
    /// Range of items on screen.
    pub visible: std::ops::Range<usize>,
}

/// Layout of a [`grid`]: `n` items about `cell_w` wide (stretched to fill rows), images
/// `aspect` (h / w); `scroll_to` brings an item into view.
pub struct GridSpec<'a> {
    pub id: &'a str,
    pub n: usize,
    pub cell_w: f32,
    pub aspect: f32,
    pub scroll_to: Option<usize>,
}

/// A virtual grid of thumbnails. Only rows in view call `item`.
pub fn grid(
    ui: &mut egui::Ui,
    cache: &mut ThumbCache,
    spec: GridSpec,
    item: &mut dyn FnMut(usize) -> Cell,
    tooltip: &mut dyn FnMut(&mut egui::Ui, usize),
) -> GridResponse {
    let GridSpec {
        id,
        n,
        cell_w,
        aspect,
        scroll_to,
    } = spec;
    let mut out = GridResponse::default();
    let gap = 8.0;
    let avail = ui.available_width() - 14.0; // leave room for the scroll bar
    let cols = (((avail + gap) / (cell_w + gap)).floor() as usize).max(1);
    // Stretch cells to fill the row.
    let cell_w = ((avail + gap) / cols as f32 - gap).max(40.0);
    let img_h = (cell_w * aspect).round();
    let text_h = 34.0;
    let row_h = img_h + text_h + 6.0;
    let rows = n.div_ceil(cols);
    let mut area = egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false]);
    if let Some(k) = scroll_to {
        let y = (k / cols) as f32 * (row_h + ui.spacing().item_spacing.y);
        area = area.vertical_scroll_offset(y.max(0.0));
    }
    let spacing = ui.spacing().item_spacing.y;
    area.show_rows(ui, row_h, rows, |ui, range| {
        out.visible = (range.start * cols).min(n)..(range.end * cols).min(n);
        for r in range {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for c in 0..cols {
                    let k = r * cols + c;
                    if k >= n {
                        break;
                    }
                    let cell = item(k);
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(cell_w, row_h), Sense::click());
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    let p = ui.painter_at(rect);
                    let hovered = resp.hovered();
                    let bg = if cell.selected {
                        Color32::from_rgb(0x3a, 0x2c, 0x1c)
                    } else if hovered {
                        Color32::from_gray(44)
                    } else {
                        Color32::from_gray(30)
                    };
                    p.rect_filled(rect, 4.0, bg);
                    if cell.selected {
                        p.rect_stroke(
                            rect,
                            4.0,
                            egui::Stroke::new(1.5, Color32::from_rgb(0xff, 0x8a, 0x3c)),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let img_rect = Rect::from_min_size(
                        rect.min + egui::vec2(3.0, 3.0),
                        egui::vec2(cell_w - 6.0, img_h),
                    );
                    match cell.image.as_ref() {
                        Some(th) => match cache.get(th) {
                            Some(t) => {
                                let s = t.size_vec2();
                                let k = (img_rect.width() / s.x).min(img_rect.height() / s.y);
                                let r = Rect::from_center_size(img_rect.center(), s * k);
                                p.image(
                                    t.id(),
                                    r,
                                    Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                            }
                            None => {
                                let msg = if cache.error(&th.key).is_some() {
                                    "unreadable"
                                } else {
                                    "…"
                                };
                                p.text(
                                    img_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    msg,
                                    egui::FontId::proportional(13.0),
                                    Color32::from_gray(110),
                                );
                            }
                        },
                        None => {
                            p.rect_filled(img_rect, 2.0, Color32::from_gray(24));
                            p.text(
                                img_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                &cell.placeholder,
                                egui::FontId::proportional(12.0),
                                Color32::from_gray(110),
                            );
                        }
                    }
                    let ty = img_rect.bottom() + 3.0;
                    let star_w = if cell.star.is_some() { 20.0 } else { 0.0 };
                    let title = ui.fonts_mut(|f| {
                        f.layout(
                            cell.title.clone(),
                            egui::FontId::proportional(13.0),
                            Color32::from_gray(225),
                            f32::INFINITY,
                        )
                    });
                    let clip = Rect::from_min_max(
                        egui::pos2(rect.left() + 4.0, ty),
                        egui::pos2(rect.right() - 4.0 - star_w, rect.bottom()),
                    );
                    p.with_clip_rect(clip).galley(
                        egui::pos2(rect.left() + 5.0, ty),
                        title,
                        Color32::WHITE,
                    );
                    p.with_clip_rect(clip).text(
                        egui::pos2(rect.left() + 5.0, ty + 16.0),
                        egui::Align2::LEFT_TOP,
                        &cell.subtitle,
                        egui::FontId::proportional(11.0),
                        Color32::from_gray(140),
                    );
                    let mut star_hit = false;
                    if let Some(on) = cell.star {
                        let sr = Rect::from_min_size(
                            egui::pos2(rect.right() - 22.0, ty - 1.0),
                            egui::vec2(20.0, 18.0),
                        );
                        let over = resp.hover_pos().is_some_and(|q| sr.contains(q));
                        let color = if on {
                            Color32::from_rgb(0xff, 0xc8, 0x3c)
                        } else if over {
                            Color32::from_gray(200)
                        } else {
                            Color32::from_gray(90)
                        };
                        star(&p, sr.center(), 7.0, on, color);
                        star_hit = resp.clicked()
                            && resp.interact_pointer_pos().is_some_and(|q| sr.contains(q));
                    }
                    if star_hit {
                        out.star_toggled = Some(k);
                    } else if resp.double_clicked() {
                        out.double_clicked = Some(k);
                    } else if resp.clicked() {
                        out.clicked = Some(k);
                    }
                    if hovered {
                        out.hovered = Some(k);
                        resp.on_hover_ui(|ui| tooltip(ui, k));
                    }
                }
            });
        }
        let _ = spacing;
    });
    out
}

/// A five-pointed star of outer radius `r` (the default fonts have no star glyph).
pub fn star(p: &egui::Painter, c: egui::Pos2, r: f32, filled: bool, color: Color32) {
    let pts: Vec<egui::Pos2> = (0..10)
        .map(|i| {
            let a = std::f32::consts::PI * (i as f32 / 5.0 - 0.5);
            let rr = if i % 2 == 0 { r } else { r * 0.42 };
            c + egui::vec2(a.cos(), a.sin()) * rr
        })
        .collect();
    if filled {
        // The inner pentagon is convex; each point is a triangle on it.
        let inner: Vec<egui::Pos2> = pts.iter().skip(1).step_by(2).copied().collect();
        p.add(egui::Shape::convex_polygon(
            inner,
            color,
            egui::Stroke::NONE,
        ));
        for i in 0..5 {
            let tri = vec![pts[(2 * i + 9) % 10], pts[2 * i], pts[2 * i + 1]];
            p.add(egui::Shape::convex_polygon(tri, color, egui::Stroke::NONE));
        }
    } else {
        p.add(egui::Shape::closed_line(pts, egui::Stroke::new(1.3, color)));
    }
}

/// A star button: true when clicked.
pub fn star_button(ui: &mut egui::Ui, on: bool, text: &str) -> egui::Response {
    let galley = ui.fonts_mut(|f| {
        f.layout_no_wrap(
            text.to_string(),
            egui::TextStyle::Button.resolve(ui.style()),
            ui.visuals().text_color(),
        )
    });
    let pad = ui.spacing().button_padding;
    let size = egui::vec2(
        galley.size().x + 22.0 + 2.0 * pad.x,
        galley.size().y.max(16.0) + 2.0 * pad.y,
    );
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let v = ui.style().interact(&resp);
    ui.painter()
        .rect_filled(rect, v.corner_radius, v.weak_bg_fill);
    let color = if on {
        Color32::from_rgb(0xff, 0xc8, 0x3c)
    } else {
        v.fg_stroke.color
    };
    star(
        ui.painter(),
        egui::pos2(rect.left() + pad.x + 8.0, rect.center().y),
        7.0,
        on,
        color,
    );
    ui.painter().galley(
        egui::pos2(
            rect.left() + pad.x + 20.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        v.text_color(),
    );
    resp
}

/// A small label for the grid's empty state.
pub fn empty_note(ui: &mut egui::Ui, text: &str) {
    ui.add_space(30.0);
    ui.vertical_centered(|ui| ui.label(RichText::new(text).weak().size(15.0)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_pngs_shrink() {
        // A 400x200 indexed image (left half colour 1, right half colour 0).
        let dir = std::env::temp_dir().join(format!("jetcarp-thumbs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.png");
        {
            let f = std::fs::File::create(&path).unwrap();
            let mut e = png::Encoder::new(std::io::BufWriter::new(f), 400, 200);
            e.set_color(png::ColorType::Indexed);
            e.set_depth(png::BitDepth::Eight);
            e.set_palette(vec![0, 0, 0, 255, 0, 0]);
            let mut w = e.write_header().unwrap();
            let data: Vec<u8> = (0..200 * 400).map(|i| u8::from(i % 400 < 200)).collect();
            w.write_image_data(&data).unwrap();
        }
        let img = load_thumb(&path, 100).unwrap();
        assert_eq!(img.size, [100, 50]);
        assert_eq!(img.pixels[0], Color32::from_rgb(255, 0, 0));
        assert_eq!(img.pixels[99], Color32::from_rgb(0, 0, 0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! The app's fonts (Settings → Accessibility → Font). By default its words are in Atkinson
//! Hyperlegible Next and its monospaced text in Atkinson Hyperlegible Mono: fonts whose letters
//! are easier to tell apart, designed by the Braille Institute for low-vision readers
//! ([`jetcarp_display::fonts`]). egui's original fonts can be chosen instead, or any font
//! installed on the system ([`scan`]), or a font file.
//!
//! Whatever is chosen comes first, and egui's built-in fonts stay behind it for any character it
//! lacks: Ubuntu Light (Greek, Cyrillic and more Latin), egui's icons, then Hack (arrows, boxes and
//! dots). With Atkinson, headings are in Atkinson Hyperlegible Next Bold.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use serde::{Deserialize, Serialize};
use skrifa::MetadataProvider as _;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The names the fonts go by in egui's definitions.
pub const NEXT: &str = jetcarp_display::fonts::NEXT_NAME;
pub const NEXT_BOLD: &str = "Atkinson Hyperlegible Next Bold";
pub const MONO: &str = jetcarp_display::fonts::MONO_NAME;
/// A font chosen from the system or a file, for text and for monospaced text.
pub const CHOSEN: &str = "chosen font";
pub const CHOSEN_MONO: &str = "chosen monospace font";

/// egui's built-in fonts, behind every choice, in egui's own order (Hack after the proportional
/// font, as the app has always had it, for arrows and dots).
pub const BEHIND_TEXT: [&str; 3] = ["Ubuntu-Light", "egui-icons", "Hack"];
pub const BEHIND_MONO: [&str; 3] = ["Hack", "Ubuntu-Light", "egui-icons"];

/// The family headings are drawn in ([`egui::TextStyle::Heading`]).
const HEADING: &str = "heading";

pub fn heading_family() -> FontFamily {
    FontFamily::Name(HEADING.into())
}

/// A font setting, as saved in `app.json`.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FontChoice {
    /// Atkinson Hyperlegible Next, or Mono for monospaced text: JETCARP's.
    #[default]
    Atkinson,
    /// egui's original: Ubuntu Light, or Hack for monospaced text.
    Egui,
    /// A font file: one installed on the system, or one chosen with *Choose a font file…*. `index`
    /// is the face in a collection (`.ttc`).
    File {
        family: String,
        path: PathBuf,
        #[serde(default)]
        index: u32,
    },
}

impl FontChoice {
    /// In words, for the setting's list.
    pub fn label(&self, mono: bool) -> String {
        match (self, mono) {
            (FontChoice::Atkinson, false) => "Atkinson Hyperlegible Next (JETCARP's)".into(),
            (FontChoice::Atkinson, true) => "Atkinson Hyperlegible Mono (JETCARP's)".into(),
            (FontChoice::Egui, false) => "Ubuntu Light (egui's original)".into(),
            (FontChoice::Egui, true) => "Hack (egui's original)".into(),
            (FontChoice::File { family, .. }, _) => family.clone(),
        }
    }
}

/// A choice ready to draw with.
#[derive(Clone)]
pub enum Face {
    Atkinson,
    Egui,
    Loaded(Arc<FontData>),
}

/// egui's font definitions for text in `text` and monospaced text in `mono`, egui's built-in
/// fonts behind each.
pub fn definitions(text: &Face, mono: &Face) -> FontDefinitions {
    use jetcarp_display::fonts as f;
    let mut d = FontDefinitions::default();
    let mut add = |name: &str, data: Arc<FontData>| {
        d.font_data.insert(name.to_owned(), data);
    };
    let (first, heading): (&str, &str) = match text {
        Face::Atkinson => {
            add(NEXT, Arc::new(FontData::from_static(f::NEXT_REGULAR)));
            add(NEXT_BOLD, Arc::new(FontData::from_static(f::NEXT_BOLD)));
            (NEXT, NEXT_BOLD)
        }
        Face::Egui => ("", ""),
        Face::Loaded(data) => {
            add(CHOSEN, data.clone());
            (CHOSEN, CHOSEN)
        }
    };
    let first_mono = match mono {
        Face::Atkinson => {
            add(MONO, Arc::new(FontData::from_static(f::MONO_REGULAR)));
            MONO
        }
        Face::Egui => "",
        Face::Loaded(data) => {
            add(CHOSEN_MONO, data.clone());
            CHOSEN_MONO
        }
    };
    let chain = |first: &str, behind: &[&str]| -> Vec<String> {
        std::iter::once(first)
            .filter(|n| !n.is_empty())
            .chain(behind.iter().copied())
            .map(str::to_owned)
            .collect()
    };
    d.families
        .insert(FontFamily::Proportional, chain(first, &BEHIND_TEXT));
    d.families
        .insert(heading_family(), chain(heading, &BEHIND_TEXT));
    d.families
        .insert(FontFamily::Monospace, chain(first_mono, &BEHIND_MONO));
    d
}

/// Put these fonts in place in `ctx` (from its next frame), headings in their own family.
pub fn install(ctx: &egui::Context, text: &Face, mono: &Face) {
    ctx.set_fonts(definitions(text, mono));
    ctx.all_styles_mut(|s| {
        if let Some(h) = s.text_styles.get_mut(&egui::TextStyle::Heading) {
            h.family = heading_family();
        }
    });
}

/// JETCARP's fonts in `ctx` (the offscreen pictures' contexts, and the app's at start).
pub fn install_default(ctx: &egui::Context) {
    install(ctx, &Face::Atkinson, &Face::Atkinson);
}

/// A font file read and checked.
pub struct Loaded {
    pub family: String,
    pub index: u32,
    pub data: Arc<FontData>,
}

/// Larger files are not fonts anyone would draw words in (the largest CJK collections are about
/// 120 MB).
const MAX_FILE: u64 = 256 << 20;

/// The characters a font must have to draw JETCARP's words.
fn basic_latin() -> impl Iterator<Item = char> {
    ('A'..='Z').chain('a'..='z').chain('0'..='9')
}

/// Read the font file at `path` and check that egui can draw JETCARP's words in it: a TrueType or
/// OpenType font (or collection) with outlines for basic Latin letters and digits. `index` is the
/// face of a collection; with `None` the first face that passes. The error is a plain sentence.
pub fn load(path: &Path, index: Option<u32>) -> Result<Loaded, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "that file".into());
    let meta =
        std::fs::metadata(path).map_err(|e| format!("JETCARP couldn't read {name}: {e}."))?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err(format!("{name} isn't a font file JETCARP can use."));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("JETCARP couldn't read {name}: {e}."))?;
    let count = match skrifa::raw::FileRef::new(&bytes) {
        Ok(skrifa::raw::FileRef::Font(_)) => 1,
        Ok(skrifa::raw::FileRef::Collection(c)) => c.len(),
        Err(_) => {
            return Err(format!(
                "{name} isn't a font JETCARP can read: it reads TrueType and OpenType fonts \
                 (.ttf, .otf and .ttc files)."
            ));
        }
    };
    let faces: Vec<u32> = match index {
        Some(i) => vec![i],
        None => (0..count).collect(),
    };
    let mut why = None;
    for i in faces {
        match check_face(&bytes, i) {
            Ok(family) => {
                return Ok(Loaded {
                    family,
                    index: i,
                    data: Arc::new(FontData::from_blob(Arc::new(bytes), i)),
                });
            }
            Err(e) => {
                why.get_or_insert(e);
            }
        }
    }
    Err(format!(
        "{name} {}",
        why.unwrap_or_else(|| "has no font in it.".into())
    ))
}

/// The face's family name, when egui can draw basic Latin in it.
fn check_face(bytes: &[u8], index: u32) -> Result<String, String> {
    let not_a_font = || "isn't a font JETCARP can read.".to_string();
    let font = skrifa::FontRef::from_index(bytes, index).map_err(|_| not_a_font())?;
    // A font has a character map and outlines (TrueType's or CFF's).
    let has = |t: &[u8; 4]| font.table_data(skrifa::Tag::new(t)).is_some();
    if !has(b"cmap") || !(has(b"glyf") || has(b"CFF ") || has(b"CFF2")) {
        return Err(not_a_font());
    }
    let charmap = font.charmap();
    let outlines = font.outline_glyphs();
    let mut missing = basic_latin()
        .filter(|c| charmap.map(*c).is_none())
        .peekable();
    if missing.peek().is_some() {
        return Err(
            "has no basic Latin letters and digits (A to Z, a to z, 0 to 9), so JETCARP's words \
             can't be drawn in it."
                .into(),
        );
    }
    if basic_latin().any(|c| charmap.map(c).and_then(|g| outlines.get(g)).is_none()) {
        return Err(
            "has no outlines for its letters (a bitmap or colour font), so JETCARP can't draw \
             its words in it."
                .into(),
        );
    }
    use skrifa::string::StringId;
    let family = [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
        .into_iter()
        .find_map(|id| {
            font.localized_strings(id)
                .english_or_first()
                .map(|s| s.chars().collect::<String>())
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "Unnamed font".into());
    Ok(family)
}

/// A font installed on the system: its family, and the file and face drawn for it (the upright
/// one nearest regular weight).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub family: String,
    pub path: PathBuf,
    pub index: u32,
}

impl Installed {
    pub fn choice(&self) -> FontChoice {
        FontChoice::File {
            family: self.family.clone(),
            path: self.path.clone(),
            index: self.index,
        }
    }
}

/// The standard folders fonts are installed in on this system.
pub fn system_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir();
    let mut v: Vec<PathBuf> = vec![];
    if cfg!(windows) {
        let windir = std::env::var_os("WINDIR")
            .or_else(|| std::env::var_os("SystemRoot"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        v.push(windir.join("Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            v.push(PathBuf::from(local).join(r"Microsoft\Windows\Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        v.push("/System/Library/Fonts".into());
        v.push("/Library/Fonts".into());
        if let Some(h) = &home {
            v.push(h.join("Library/Fonts"));
        }
    } else {
        // fontconfig's folders.
        v.push("/usr/share/fonts".into());
        v.push("/usr/local/share/fonts".into());
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        if let Some(d) = data {
            v.push(d.join("fonts"));
        }
        if let Some(h) = &home {
            v.push(h.join(".fonts"));
        }
    }
    v
}

/// Font files looked at in one scan, at most (a system with more is unusual).
const MAX_FILES: usize = 50_000;
const MAX_DEPTH: usize = 12;

/// The font families installed in `dirs` that JETCARP can draw its words in, one face each,
/// sorted by name. Only the parts of each file that name it and say what it covers are read.
pub fn scan(dirs: &[PathBuf]) -> Vec<Installed> {
    let mut files = vec![];
    let mut seen = std::collections::HashSet::new();
    for d in dirs {
        walk(d, 0, &mut files, &mut seen);
    }
    files.sort();
    files.dedup();
    // Family (folded) → (how far from upright regular, the face).
    let mut best: std::collections::BTreeMap<String, (u32, Installed)> = Default::default();
    for path in files {
        for face in faces_in(&path) {
            if !face.latin || face.family.starts_with('.') {
                continue;
            }
            let rank = u32::from(face.italic) * 1000 + face.weight.abs_diff(400) as u32;
            let key = face.family.to_lowercase();
            let better = best.get(&key).is_none_or(|(r, _)| rank < *r);
            if better {
                best.insert(
                    key,
                    (
                        rank,
                        Installed {
                            family: face.family,
                            path: path.clone(),
                            index: face.index,
                        },
                    ),
                );
            }
        }
    }
    best.into_values().map(|(_, i)| i).collect()
}

fn walk(
    dir: &Path,
    depth: usize,
    files: &mut Vec<PathBuf>,
    seen: &mut std::collections::HashSet<PathBuf>,
) {
    if depth > MAX_DEPTH || files.len() >= MAX_FILES {
        return;
    }
    // Folders linked from more than one place are walked once.
    let Ok(real) = std::fs::canonicalize(dir) else {
        return;
    };
    if !seen.insert(real) {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if files.len() >= MAX_FILES {
            return;
        }
        if p.is_dir() {
            walk(&p, depth + 1, files, seen);
        } else if p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            ["ttf", "otf", "ttc", "otc"].contains(&e.to_ascii_lowercase().as_str())
        }) {
            files.push(p);
        }
    }
}

/// What a scan needs of one face.
#[derive(Debug)]
struct FaceInfo {
    index: u32,
    family: String,
    weight: u16,
    italic: bool,
    latin: bool,
}

/// `len` bytes of `f` from `at`.
fn read_at(f: &mut std::fs::File, at: u64, len: usize) -> Option<Vec<u8>> {
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut buf = vec![0; len];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// The faces of a font file or collection, read from its table directories and its `name`,
/// `cmap` and `OS/2` tables alone.
fn faces_in(path: &Path) -> Vec<FaceInfo> {
    let Ok(mut f) = std::fs::File::open(path) else {
        return vec![];
    };
    let Some(head) = read_at(&mut f, 0, 12) else {
        return vec![];
    };
    let offsets: Vec<u64> = if &head[..4] == b"ttcf" {
        let n = be32(&head, 8).unwrap_or(0).min(256) as usize;
        let Some(o) = read_at(&mut f, 12, 4 * n) else {
            return vec![];
        };
        (0..n)
            .filter_map(|i| be32(&o, 4 * i))
            .map(u64::from)
            .collect()
    } else {
        vec![0]
    };
    offsets
        .into_iter()
        .enumerate()
        .filter_map(|(i, off)| face_at(&mut f, off, i as u32))
        .collect()
}

fn face_at(f: &mut std::fs::File, off: u64, index: u32) -> Option<FaceInfo> {
    use skrifa::raw::{FontData, FontRead, tables};
    let dir = read_at(f, off, 12)?;
    let version = be32(&dir, 0)?;
    // TrueType (1.0 or 'true') and CFF ('OTTO') outlines.
    if ![0x0001_0000, 0x7472_7565, 0x4F54_544F].contains(&version) {
        return None;
    }
    let n = be16(&dir, 4)?.min(1024) as usize;
    let recs = read_at(f, off + 12, 16 * n)?;
    let table = |tag: &[u8; 4], cap: u32| -> Option<(u64, usize)> {
        (0..n).find_map(|i| {
            let r = &recs[16 * i..16 * i + 16];
            (&r[..4] == tag).then(|| {
                let (o, l) = (be32(r, 8).unwrap_or(0), be32(r, 12).unwrap_or(0));
                (u64::from(o), l.min(cap) as usize)
            })
        })
    };
    let (name_at, name_len) = table(b"name", 1 << 20)?;
    let name_bytes = read_at(f, name_at, name_len)?;
    let name = tables::name::Name::read(FontData::new(&name_bytes)).ok()?;
    let family = name_string(&name, 16).or_else(|| name_string(&name, 1))?;
    let (cmap_at, cmap_len) = table(b"cmap", 16 << 20)?;
    let cmap_bytes = read_at(f, cmap_at, cmap_len)?;
    let cmap = tables::cmap::Cmap::read(FontData::new(&cmap_bytes)).ok()?;
    let latin = basic_latin().all(|c| cmap.map_codepoint(c).is_some());
    let (mut weight, mut italic) = (400, false);
    if let Some((o, l)) = table(b"OS/2", 512)
        && let Some(b) = read_at(f, o, l)
        && let Ok(os2) = tables::os2::Os2::read(FontData::new(&b))
    {
        weight = os2.us_weight_class();
        let sel = os2.fs_selection();
        italic = sel.intersects(tables::os2::SelectionFlags::ITALIC)
            || sel.intersects(tables::os2::SelectionFlags::OBLIQUE);
    }
    let sub = name_string(&name, 17).or_else(|| name_string(&name, 2));
    if sub.is_some_and(|s| {
        let s = s.to_lowercase();
        s.contains("italic") || s.contains("oblique")
    }) {
        italic = true;
    }
    Some(FaceInfo {
        index,
        family: family.trim().to_owned(),
        weight,
        italic,
        latin,
    })
}

/// Name `id`, in English where the font has it: Windows' US English, then any Windows or Unicode
/// record, then Mac Roman English.
fn name_string(name: &skrifa::raw::tables::name::Name, id: u16) -> Option<String> {
    let records = name.name_record();
    let rank = |r: &skrifa::raw::tables::name::NameRecord| match (r.platform_id(), r.language_id())
    {
        (3, 0x409) => 0,
        (3, _) => 1,
        (0, _) => 2,
        (1, 0) => 3,
        _ => 9,
    };
    records
        .iter()
        .filter(|r| r.name_id().to_u16() == id && rank(r) < 9)
        .min_by_key(|r| rank(r))
        .and_then(|r| r.string(name.string_data()).ok())
        .map(|s| s.chars().collect::<String>())
        .filter(|s| !s.trim().is_empty())
}

/// The installed fonts, found once on a thread of their own when the setting is first shown.
#[derive(Default)]
pub struct Scanner {
    /// The folders looked in: the system's ([`system_dirs`]), none in tests unless they say.
    pub dirs: Vec<PathBuf>,
    pub found: Option<Arc<Vec<Installed>>>,
    rx: Option<std::sync::mpsc::Receiver<Vec<Installed>>>,
}

impl Scanner {
    pub fn new(dirs: Vec<PathBuf>) -> Scanner {
        Scanner {
            dirs,
            found: None,
            rx: None,
        }
    }

    /// Start looking, unless it has (`ctx` is asked to draw again when done).
    pub fn start(&mut self, ctx: &egui::Context) {
        if self.found.is_some() || self.rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let dirs = self.dirs.clone();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("font scan".into())
            .spawn(move || {
                let _ = tx.send(scan(&dirs));
                ctx.request_repaint();
            });
        if spawned.is_ok() {
            self.rx = Some(rx);
        } else {
            self.found = Some(Arc::new(scan(&self.dirs)));
        }
    }

    /// The fonts, once found.
    pub fn poll(&mut self) -> Option<Arc<Vec<Installed>>> {
        if let Some(rx) = &self.rx
            && let Ok(v) = rx.try_recv()
        {
            self.found = Some(Arc::new(v));
            self.rx = None;
        }
        self.found.clone()
    }

    /// Wait for the scan (tests).
    #[cfg(test)]
    pub fn wait(&mut self) -> Arc<Vec<Installed>> {
        if let Some(rx) = self.rx.take() {
            self.found = Some(Arc::new(rx.recv().unwrap_or_default()));
        }
        self.found.clone().expect("a scan was started")
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// egui's own bytes of one of its built-in fonts.
    pub(crate) fn builtin(name: &str) -> Vec<u8> {
        FontDefinitions::default().font_data[name].bytes().to_vec()
    }

    fn ctx_with(text: &Face, mono: &Face) -> egui::Context {
        let ctx = egui::Context::default();
        install(&ctx, text, mono);
        ctx.run_pass(Default::default(), |_| {})
            .textures_delta
            .clear();
        ctx
    }

    #[test]
    fn the_default_fonts_are_atkinson() {
        let d = definitions(&Face::Atkinson, &Face::Atkinson);
        let p = &d.families[&FontFamily::Proportional];
        let m = &d.families[&FontFamily::Monospace];
        assert_eq!(p[0], NEXT, "{p:?}");
        assert_eq!(m[0], MONO, "{m:?}");
        assert_eq!(d.families[&heading_family()][0], NEXT_BOLD);
        assert_eq!(
            d.font_data[NEXT].bytes(),
            jetcarp_display::fonts::NEXT_REGULAR
        );
        assert_eq!(
            d.font_data[MONO].bytes(),
            jetcarp_display::fonts::MONO_REGULAR
        );
        assert_eq!(
            d.font_data[NEXT_BOLD].bytes(),
            jetcarp_display::fonts::NEXT_BOLD
        );
        // egui's fonts stay behind them, in egui's order.
        assert_eq!(&p[1..], &BEHIND_TEXT.map(String::from));
        assert_eq!(&m[1..], &BEHIND_MONO.map(String::from));
        // In a context: what egui draws with.
        let ctx = ctx_with(&Face::Atkinson, &Face::Atkinson);
        let in_ctx = ctx.fonts(|f| f.definitions().clone());
        assert_eq!(in_ctx.families[&FontFamily::Proportional][0], NEXT);
        assert_eq!(in_ctx.families[&FontFamily::Monospace][0], MONO);
        let style = ctx.global_style();
        assert_eq!(
            style.text_styles[&egui::TextStyle::Heading].family,
            heading_family()
        );
    }

    #[test]
    fn egui_s_original_fonts_are_as_they_were() {
        let d = definitions(&Face::Egui, &Face::Egui);
        assert_eq!(
            d.families[&FontFamily::Proportional],
            BEHIND_TEXT.map(String::from)
        );
        assert_eq!(
            d.families[&FontFamily::Monospace],
            BEHIND_MONO.map(String::from)
        );
        // As the app had them before Atkinson: egui's defaults with Hack after Ubuntu Light.
        let mut before = FontDefinitions::default();
        before
            .families
            .get_mut(&FontFamily::Proportional)
            .unwrap()
            .push("Hack".into());
        assert_eq!(
            d.families[&FontFamily::Proportional],
            before.families[&FontFamily::Proportional]
        );
        assert_eq!(
            d.families[&FontFamily::Monospace],
            before.families[&FontFamily::Monospace]
        );
    }

    /// Every character in the app's strings: string literals in its source and the display
    /// crate's (the caption), non-ASCII ones.
    fn interface_characters() -> std::collections::BTreeSet<char> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut set = std::collections::BTreeSet::new();
        let mut stack = vec![root.join("src"), root.join("../jetcarp-display/src")];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let text = std::fs::read_to_string(&p).unwrap();
                    // Not tests' strings, nor comments: only what is shown.
                    let shown = text.split("#[cfg(test)]").next().unwrap_or("");
                    for line in shown.lines() {
                        let code = line.split("//").next().unwrap_or("");
                        let mut inside = false;
                        let mut escaped = false;
                        for c in code.chars() {
                            if inside && !escaped && c == '"' {
                                inside = false;
                            } else if !inside && c == '"' {
                                inside = true;
                            } else if inside && !c.is_ascii() {
                                set.insert(c);
                            }
                            escaped = inside && c == '\\' && !escaped;
                        }
                    }
                }
            }
        }
        set
    }

    /// Characters the app shows that egui's own fonts never drew either (a font of the system's
    /// can be chosen for them): the macOS key symbols and the Reseed pane's star.
    const NEVER_DRAWN: &str = "⌃⌘⌥⌦⌫★";

    #[test]
    fn the_fallback_draws_every_interface_symbol() {
        let chars = interface_characters();
        assert!(chars.contains(&'×') && chars.contains(&'→'), "{chars:?}");
        let atkinson = ctx_with(&Face::Atkinson, &Face::Atkinson);
        let original = ctx_with(&Face::Egui, &Face::Egui);
        let draws = |ctx: &egui::Context, family: FontFamily, c: char| {
            ctx.fonts_mut(|f| f.has_glyph(&egui::FontId::new(14.0, family), c))
        };
        let mut lost = vec![];
        let mut undrawn = vec![];
        for &c in &chars {
            for family in [
                FontFamily::Proportional,
                FontFamily::Monospace,
                heading_family(),
            ] {
                let before = draws(&original, family.clone(), c);
                let now = draws(&atkinson, family.clone(), c);
                if before && !now {
                    lost.push(format!("{c} (U+{:04X}) in {family:?}", c as u32));
                }
                if !now && !NEVER_DRAWN.contains(c) {
                    undrawn.push(format!("{c} (U+{:04X}) in {family:?}", c as u32));
                }
            }
        }
        assert!(
            lost.is_empty(),
            "egui drew these and Atkinson's set doesn't: {lost:?}"
        );
        assert!(undrawn.is_empty(), "not drawn: {undrawn:?}");
        // Greek and more of Latin come from Ubuntu Light behind Atkinson; CJK is drawn by none
        // of the built-in fonts (a system font can be chosen).
        for c in "ΔΣλπφαβγΩμ ÆŒçéüñßøå°±–—“”€".chars() {
            assert!(draws(&atkinson, FontFamily::Proportional, c), "{c}");
            assert!(draws(&atkinson, FontFamily::Monospace, c), "{c}");
        }
        for c in "中文日本語".chars() {
            assert_eq!(
                draws(&atkinson, FontFamily::Proportional, c),
                draws(&original, FontFamily::Proportional, c),
                "{c}"
            );
        }
    }

    pub(crate) fn scratch(name: &str) -> jetcarp_core::scratch::ScratchDir {
        jetcarp_core::scratch::ScratchDir::new(&format!("fonts-{name}"))
    }

    #[test]
    fn installed_fonts_are_found_by_family_and_unusable_files_are_left_out() {
        let dir = scratch("scan");
        let fonts = dir.join("share/fonts");
        std::fs::create_dir_all(fonts.join("truetype/hack")).unwrap();
        std::fs::write(
            fonts.join("truetype/hack/Hack-Regular.ttf"),
            builtin("Hack"),
        )
        .unwrap();
        std::fs::write(fonts.join("Ubuntu-Light.TTF"), builtin("Ubuntu-Light")).unwrap();
        // Not fonts, or no Latin: left out.
        std::fs::write(fonts.join("broken.ttf"), b"not a font at all").unwrap();
        std::fs::write(fonts.join("icons.ttf"), builtin("egui-icons")).unwrap();
        std::fs::write(fonts.join("notes.txt"), b"hello").unwrap();
        let found = scan(&[fonts.clone(), dir.join("nowhere")]);
        let names: Vec<&str> = found.iter().map(|f| f.family.as_str()).collect();
        assert_eq!(names, ["Hack", "Ubuntu"], "{found:?}");
        assert!(found[0].path.ends_with("truetype/hack/Hack-Regular.ttf"));
        // A scan on its own thread finds the same.
        let mut s = Scanner::new(vec![fonts]);
        s.start(&egui::Context::default());
        assert_eq!(*s.wait(), found);
    }

    #[test]
    fn a_bad_font_file_is_refused_with_a_plain_message() {
        let dir = scratch("bad");
        std::fs::create_dir_all(&*dir).unwrap();
        let junk = dir.join("junk.ttf");
        std::fs::write(&junk, b"\0\x01\0\0 this is not a font").unwrap();
        let e = load(&junk, None).err().expect("refused");
        assert!(e.starts_with("junk.ttf isn't a font"), "{e}");
        let icons = dir.join("icons.ttf");
        std::fs::write(&icons, builtin("egui-icons")).unwrap();
        let e = load(&icons, None).err().expect("refused");
        assert!(e.contains("no basic Latin"), "{e}");
        let e = load(&dir.join("gone.ttf"), None).err().expect("refused");
        assert!(e.starts_with("JETCARP couldn't read gone.ttf"), "{e}");
        // No local path in what is shown.
        for p in [&junk, &icons] {
            let e = load(p, None).err().unwrap();
            assert!(!e.contains(&*dir.to_string_lossy()), "{e}");
        }
        let ok = load(
            &{
                let p = dir.join("Hack-Regular.ttf");
                std::fs::write(&p, builtin("Hack")).unwrap();
                p
            },
            None,
        )
        .expect("Hack is a font");
        assert_eq!(ok.family, "Hack");
        assert_eq!(ok.index, 0);
    }

    #[test]
    fn a_chosen_font_comes_first_with_egui_s_behind_it() {
        let data = Arc::new(FontData::from_owned(builtin("Hack")));
        let d = definitions(&Face::Loaded(data.clone()), &Face::Loaded(data));
        assert_eq!(d.families[&FontFamily::Proportional][0], CHOSEN);
        assert_eq!(
            &d.families[&FontFamily::Proportional][1..],
            &BEHIND_TEXT.map(String::from)
        );
        assert_eq!(d.families[&FontFamily::Monospace][0], CHOSEN_MONO);
        assert_eq!(d.families[&heading_family()][0], CHOSEN);
        assert_eq!(d.font_data[CHOSEN].bytes(), &builtin("Hack")[..]);
    }

    #[test]
    fn font_choices_are_saved_by_name() {
        let c = FontChoice::File {
            family: "Hack".into(),
            path: "/usr/share/fonts/hack/Hack-Regular.ttf".into(),
            index: 0,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(
            json,
            r#"{"file":{"family":"Hack","path":"/usr/share/fonts/hack/Hack-Regular.ttf","index":0}}"#
        );
        assert_eq!(serde_json::from_str::<FontChoice>(&json).unwrap(), c);
        assert_eq!(
            serde_json::to_string(&FontChoice::Atkinson).unwrap(),
            r#""atkinson""#
        );
        assert_eq!(
            serde_json::to_string(&FontChoice::Egui).unwrap(),
            r#""egui""#
        );
    }
}

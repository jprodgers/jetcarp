//! The fonts JETCARP ships, and the font setting the app and the desktop background share.
//!
//! JETCARP ships Atkinson Hyperlegible Next and its monospaced sibling, Atkinson Hyperlegible
//! Mono, fonts whose letters are easier to tell apart (designed by the Braille Institute for
//! low-vision readers). The app draws its words in them by default, and so does the caption on
//! surfaces with no egui ([`crate::caption::CaptionPainter`]).
//!
//! The files are the copyright holders' own, whole and unmodified, under the SIL Open Font
//! License 1.1, each family's licence beside it (`fonts/*/OFL.txt`); `fonts/README.md` gives
//! where each came from and its SHA-256.
//!
//! Anyone can choose another font in the app (Settings → Accessibility): the choice
//! ([`FontChoice`]) is saved in `app.json`, where the desktop background reads it too
//! ([`saved_in`]). The fonts installed on the system are found here ([`scan`]), and so are the
//! installed fonts that draw a character none of the chosen fonts has ([`Fallbacks`]). Only the
//! parts of each file that name it and say what it covers are read, with skrifa (egui's own font
//! parser).

use serde::{Deserialize, Serialize};
use skrifa::MetadataProvider as _;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Atkinson Hyperlegible Next Regular.
pub const NEXT_REGULAR: &[u8] =
    include_bytes!("../fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Regular.ttf");

/// Atkinson Hyperlegible Next Bold (the app's headings).
pub const NEXT_BOLD: &[u8] =
    include_bytes!("../fonts/atkinson-hyperlegible-next/AtkinsonHyperlegibleNext-Bold.ttf");

/// Atkinson Hyperlegible Mono Regular.
pub const MONO_REGULAR: &[u8] =
    include_bytes!("../fonts/atkinson-hyperlegible-mono/AtkinsonHyperlegibleMono-Regular.ttf");

/// The family names, as the fonts give them.
pub const NEXT_NAME: &str = "Atkinson Hyperlegible Next";
pub const MONO_NAME: &str = "Atkinson Hyperlegible Mono";

/// A font setting, as saved in `app.json` (`font` and `mono_font` in `accessibility`).
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

/// The app's settings file in settings folder `settings`.
pub const APP_SETTINGS: &str = "app.json";

/// The fonts chosen in the app, as saved in settings folder `settings`: (text, monospaced). A
/// file that is missing or can't be read, or a setting that can't be, gives JETCARP's.
pub fn saved_in(settings: &Path) -> (FontChoice, FontChoice) {
    let Some(v) = std::fs::read_to_string(settings.join(APP_SETTINGS))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    else {
        return Default::default();
    };
    let pick = |key: &str| {
        v.pointer(&format!("/accessibility/{key}"))
            .and_then(|x| FontChoice::deserialize(x).ok())
            .unwrap_or_default()
    };
    (pick("font"), pick("mono_font"))
}

/// A font file read and checked: its family, the face drawn, and the whole file.
pub struct FontFile {
    pub family: String,
    pub index: u32,
    pub bytes: Vec<u8>,
}

/// Larger files are not fonts anyone would draw words in (the largest CJK collections are about
/// 120 MB).
const MAX_FILE: u64 = 256 << 20;

/// The characters a font must have to draw JETCARP's words.
fn basic_latin() -> impl Iterator<Item = char> {
    ('A'..='Z').chain('a'..='z').chain('0'..='9')
}

/// The file's name alone, for messages (never its folder).
fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "that file".into())
}

/// Read the font file at `path` and check that JETCARP's words can be drawn in it: a TrueType or
/// OpenType font (or collection) with outlines for basic Latin letters and digits. `index` is the
/// face of a collection; with `None` the first face that passes. The error is a plain sentence
/// that names the file, never its folder.
pub fn load(path: &Path, index: Option<u32>) -> Result<FontFile, String> {
    let name = file_name(path);
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
                return Ok(FontFile {
                    family,
                    index: i,
                    bytes,
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

/// The face's family name, when JETCARP's words can be drawn in it.
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
    if basic_latin().any(|c| charmap.map(c).is_none()) {
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
    // Family (folded) → (how far from upright regular, the face).
    let mut best: std::collections::BTreeMap<String, (u32, Installed)> = Default::default();
    for face in font_files(dirs).iter().flat_map(|p| faces_in(p, false)) {
        if !face.latin || face.family.starts_with('.') {
            continue;
        }
        let rank = face.rank();
        let key = face.family.to_lowercase();
        if best.get(&key).is_none_or(|(r, _)| rank < *r) {
            best.insert(
                key,
                (
                    rank,
                    Installed {
                        family: face.family,
                        path: face.path,
                        index: face.index,
                    },
                ),
            );
        }
    }
    best.into_values().map(|(_, i)| i).collect()
}

/// Every font file in `dirs`, sorted, each once.
fn font_files(dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = vec![];
    let mut seen = std::collections::HashSet::new();
    for d in dirs {
        walk(d, 0, &mut files, &mut seen);
    }
    files.sort();
    files.dedup();
    files
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
    path: PathBuf,
    index: u32,
    family: String,
    weight: u16,
    italic: bool,
    latin: bool,
    /// It has outlines (TrueType's or CFF's), not only bitmaps.
    outlines: bool,
    /// The file's size in bytes.
    size: u64,
    /// The characters it maps, as ranges (when asked for).
    coverage: Vec<(u32, u32)>,
}

impl FaceInfo {
    /// How far from upright regular: italics last, then by distance from weight 400.
    fn rank(&self) -> u32 {
        u32::from(self.italic) * 1000 + u32::from(self.weight.abs_diff(400))
    }
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
/// `cmap` and `OS/2` tables alone. With `coverage`, each face's characters too.
fn faces_in(path: &Path, coverage: bool) -> Vec<FaceInfo> {
    let Ok(mut f) = std::fs::File::open(path) else {
        return vec![];
    };
    let size = f.metadata().map_or(0, |m| m.len());
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
        .filter_map(|(i, off)| face_at(&mut f, off, i as u32, coverage))
        .map(|mut face| {
            face.path = path.to_path_buf();
            face.size = size;
            face
        })
        .collect()
}

fn face_at(f: &mut std::fs::File, off: u64, index: u32, coverage: bool) -> Option<FaceInfo> {
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
    let outlines = [b"glyf", b"CFF ", b"CFF2"]
        .into_iter()
        .any(|t| table(t, 0).is_some());
    let (name_at, name_len) = table(b"name", 1 << 20)?;
    let name_bytes = read_at(f, name_at, name_len)?;
    let name = tables::name::Name::read(FontData::new(&name_bytes)).ok()?;
    let family = name_string(&name, 16).or_else(|| name_string(&name, 1))?;
    let (cmap_at, cmap_len) = table(b"cmap", 16 << 20)?;
    let cmap_bytes = read_at(f, cmap_at, cmap_len)?;
    let cmap = tables::cmap::Cmap::read(FontData::new(&cmap_bytes)).ok()?;
    let latin = basic_latin().all(|c| cmap.map_codepoint(c).is_some());
    let mut ranges = vec![];
    if coverage {
        let glyphs = table(b"maxp", 6)
            .and_then(|(o, l)| read_at(f, o, l))
            .and_then(|b| be16(&b, 4))
            .map_or(u32::from(u16::MAX), u32::from);
        ranges = coverage_of(&cmap, glyphs);
    }
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
        path: PathBuf::new(),
        index,
        family: family.trim().to_owned(),
        weight,
        italic,
        latin,
        outlines,
        size: 0,
        coverage: ranges,
    })
}

/// The characters `cmap` maps to a glyph, as sorted, merged ranges, from its fullest Unicode
/// subtable. A symbol font's (Windows' symbol encoding, which puts its glyphs where letters'
/// codes would be) maps none.
fn coverage_of(cmap: &skrifa::raw::tables::cmap::Cmap<'_>, glyphs: u32) -> Vec<(u32, u32)> {
    use skrifa::raw::tables::cmap::{CmapIterLimits, PlatformId};
    let Some((_, record, sub)) = cmap.best_subtable() else {
        return vec![];
    };
    if (record.platform_id(), record.encoding_id()) == (PlatformId::Windows, 0) {
        return vec![];
    }
    let limits = CmapIterLimits {
        max_char: char::MAX as u32,
        glyph_count: glyphs,
    };
    let mut cps: Vec<u32> = sub
        .iter_with_limits(limits)
        .filter(|(_, g)| g.to_u32() != 0)
        .map(|(c, _)| c)
        .collect();
    cps.sort_unstable();
    let mut out: Vec<(u32, u32)> = vec![];
    for c in cps {
        match out.last_mut() {
            Some((_, end)) if c <= *end + 1 => *end = (*end).max(c),
            _ => out.push((c, c)),
        }
    }
    out
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

/// An installed font that draws a character the chosen fonts lack.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fallback {
    pub family: String,
    pub path: PathBuf,
    pub index: u32,
}

/// The installed fonts asked, last of all, for a character none of the chosen fonts (nor egui's
/// own) has: the macOS key symbols (⌘ ⌥ ⌃ ⌫ ⌦), which macOS's own fonts have; Chinese, Japanese
/// and Korean, wherever a font for them is installed. Found once, from each file's character map
/// alone; a font file is read whole only when one of its characters is drawn.
#[derive(Default)]
pub struct Fallbacks {
    /// Each face with outlines, best first: upright regular before the rest, then the smallest
    /// file (a symbol font before a large CJK collection), then by path, so the same fonts give
    /// the same answer every time.
    faces: Vec<(Vec<(u32, u32)>, Fallback)>,
}

impl Fallbacks {
    /// The fonts in `dirs` that can stand in for a missing character. Fonts that only draw a
    /// placeholder for every character (macOS's LastResort), hidden ones (named with a leading
    /// dot), and bitmap and colour fonts with no outlines are left out.
    pub fn find(dirs: &[PathBuf]) -> Fallbacks {
        let mut faces: Vec<FaceInfo> = font_files(dirs)
            .iter()
            .flat_map(|p| faces_in(p, true))
            .filter(|f| {
                f.outlines
                    && !f.coverage.is_empty()
                    && !f.family.starts_with('.')
                    && !f.family.eq_ignore_ascii_case("LastResort")
            })
            .collect();
        faces.sort_by(|a, b| {
            (a.rank(), a.size, &a.path, a.index).cmp(&(b.rank(), b.size, &b.path, b.index))
        });
        Fallbacks {
            faces: faces
                .into_iter()
                .map(|f| {
                    (
                        f.coverage,
                        Fallback {
                            family: f.family,
                            path: f.path,
                            index: f.index,
                        },
                    )
                })
                .collect(),
        }
    }

    /// The font to draw `c` in, if an installed one has it.
    pub fn for_char(&self, c: char) -> Option<&Fallback> {
        let c = c as u32;
        self.faces
            .iter()
            .find(|(ranges, _)| {
                let i = ranges.partition_point(|&(_, end)| end < c);
                ranges.get(i).is_some_and(|&(start, _)| start <= c)
            })
            .map(|(_, f)| f)
    }

    /// How many faces were found.
    pub fn len(&self) -> usize {
        self.faces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
}

/// Font files made for tests, which can't count on any font being installed: one that maps only
/// the characters a test names, so it stands in for a system font that has them.
#[doc(hidden)]
pub mod stand_in {
    use skrifa::MetadataProvider as _;

    /// A TrueType font named `family` that maps each of `chars`, and nothing else, to the glyph
    /// that `base` (a TrueType font) draws for `like`. Its other tables are `base`'s.
    pub fn font_with(base: &[u8], chars: &str, like: char, family: &str) -> Vec<u8> {
        let font = skrifa::FontRef::new(base).expect("a font");
        let glyph = font
            .charmap()
            .map(like)
            .expect("the base font has it")
            .to_u32();
        let mut cps: Vec<u32> = chars.chars().map(u32::from).collect();
        cps.sort_unstable();
        cps.dedup();
        let mut tables: Vec<([u8; 4], Vec<u8>)> = font
            .table_directory()
            .table_records()
            .iter()
            .map(|r| r.tag().to_be_bytes())
            .filter(|t| t != b"cmap" && t != b"name")
            .map(|t| {
                (
                    t,
                    font.table_data(skrifa::Tag::new(&t))
                        .unwrap()
                        .as_bytes()
                        .to_vec(),
                )
            })
            .collect();
        tables.push((*b"cmap", cmap12(&cps, glyph)));
        tables.push((*b"name", name(family)));
        sfnt(tables)
    }

    /// A font collection (`.ttc`) of `fonts`, each a font file of one face such as
    /// [`font_with`] makes: face `i` is the `i`th.
    pub fn collection(fonts: &[Vec<u8>]) -> Vec<u8> {
        let mut out = vec![];
        out.extend(b"ttcf");
        out.extend(0x0001_0000u32.to_be_bytes());
        out.extend((fonts.len() as u32).to_be_bytes());
        let mut at = 12 + 4 * fonts.len();
        let mut body = vec![];
        for font in fonts {
            out.extend((at as u32).to_be_bytes());
            // A collection's table offsets count from the start of the collection.
            let mut f = font.clone();
            let n = usize::from(u16::from_be_bytes([f[4], f[5]]));
            for i in 0..n {
                let o = 12 + 16 * i + 8;
                let v = u32::from_be_bytes(f[o..o + 4].try_into().unwrap()) + at as u32;
                f[o..o + 4].copy_from_slice(&v.to_be_bytes());
            }
            f.resize(f.len().div_ceil(4) * 4, 0);
            at += f.len();
            body.extend(f);
        }
        out.extend(body);
        out
    }

    /// A character map of one Windows full-Unicode subtable (format 12), `cps` to `glyph`.
    fn cmap12(cps: &[u32], glyph: u32) -> Vec<u8> {
        let mut t = vec![];
        let n = cps.len() as u32;
        t.extend(0u16.to_be_bytes()); // version
        t.extend(1u16.to_be_bytes()); // one subtable
        t.extend(3u16.to_be_bytes()); // Windows
        t.extend(10u16.to_be_bytes()); // full Unicode
        t.extend(12u32.to_be_bytes()); // offset
        t.extend(12u16.to_be_bytes()); // format 12
        t.extend(0u16.to_be_bytes());
        t.extend((16 + 12 * n).to_be_bytes());
        t.extend(0u32.to_be_bytes()); // language
        t.extend(n.to_be_bytes());
        for &c in cps {
            t.extend(c.to_be_bytes());
            t.extend(c.to_be_bytes());
            t.extend(glyph.to_be_bytes());
        }
        t
    }

    /// A naming table giving the family and "Regular", in Windows' US English.
    fn name(family: &str) -> Vec<u8> {
        let strings: [(u16, Vec<u8>); 2] = [
            (
                1,
                family.encode_utf16().flat_map(u16::to_be_bytes).collect(),
            ),
            (
                2,
                "Regular"
                    .encode_utf16()
                    .flat_map(u16::to_be_bytes)
                    .collect(),
            ),
        ];
        let mut t = vec![];
        t.extend(0u16.to_be_bytes()); // format 0
        t.extend((strings.len() as u16).to_be_bytes());
        t.extend((6 + 12 * strings.len() as u16).to_be_bytes());
        let mut at = 0u16;
        for (id, bytes) in &strings {
            for v in [3u16, 1, 0x409, *id, bytes.len() as u16, at] {
                t.extend(v.to_be_bytes());
            }
            at += bytes.len() as u16;
        }
        for (_, bytes) in &strings {
            t.extend(bytes);
        }
        t
    }

    /// A font file of `tables`, each four-byte aligned with its checksum.
    fn sfnt(mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
        tables.sort_by_key(|(t, _)| *t);
        let n = tables.len() as u16;
        let pow = 1u16 << (15 - n.leading_zeros() as u16).min(15);
        let mut out = vec![];
        out.extend(0x0001_0000u32.to_be_bytes());
        for v in [n, pow * 16, pow.trailing_zeros() as u16, n * 16 - pow * 16] {
            out.extend(v.to_be_bytes());
        }
        let mut offset = 12 + 16 * u32::from(n);
        let mut data = vec![];
        for (tag, bytes) in &tables {
            let mut padded = bytes.clone();
            padded.resize(bytes.len().div_ceil(4) * 4, 0);
            let sum = padded
                .chunks(4)
                .map(|w| u32::from_be_bytes(w.try_into().unwrap()))
                .fold(0u32, u32::wrapping_add);
            out.extend(tag);
            out.extend(sum.to_be_bytes());
            out.extend(offset.to_be_bytes());
            out.extend((bytes.len() as u32).to_be_bytes());
            offset += padded.len() as u32;
            data.extend(padded);
        }
        out.extend(data);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ab_glyph::{Font, FontRef};

    /// The files are the ones `fonts/README.md` names, byte for byte.
    #[test]
    fn the_shipped_fonts_are_the_ones_the_readme_names() {
        let readme = include_str!("../fonts/README.md");
        for (file, bytes) in [
            ("AtkinsonHyperlegibleNext-Regular.ttf", NEXT_REGULAR),
            ("AtkinsonHyperlegibleNext-Bold.ttf", NEXT_BOLD),
            ("AtkinsonHyperlegibleMono-Regular.ttf", MONO_REGULAR),
        ] {
            let line = readme
                .lines()
                .find(|l| l.contains(file))
                .unwrap_or_else(|| panic!("{file} is not in fonts/README.md"));
            assert!(
                line.contains(&jetcarp_core::sha256::sha256_hex(bytes)),
                "{file}'s SHA-256 is not the README's: {line}"
            );
            assert!(FontRef::try_from_slice(bytes).is_ok(), "{file} parses");
        }
    }

    #[test]
    fn the_fonts_draw_basic_latin() {
        for bytes in [NEXT_REGULAR, NEXT_BOLD, MONO_REGULAR] {
            let f = FontRef::try_from_slice(bytes).unwrap();
            for c in (0x21u8..0x7f).map(char::from) {
                assert_ne!(f.glyph_id(c).0, 0, "{c:?}");
            }
        }
    }

    /// The last-resort fonts are found by character: upright regular first, then the smallest
    /// file, so the answer is the same every time; LastResort, hidden fonts and symbol fonts
    /// are left out.
    #[test]
    fn fallbacks_are_found_by_character() {
        let dir = jetcarp_core::scratch::ScratchDir::new("fonts-fallbacks");
        std::fs::create_dir_all(dir.join("more")).unwrap();
        let hack = epaint_default_fonts::HACK_REGULAR;
        let keys = "⌃⌘⌥⌦⌫";
        let small = stand_in::font_with(hack, keys, 'X', "Key Symbols");
        // The same characters in a larger file: found second.
        let mut big = stand_in::font_with(hack, "⌘中文", 'O', "Big CJK");
        big.extend(vec![0; 4096]);
        std::fs::write(dir.join("KeySymbols.ttf"), &small).unwrap();
        std::fs::write(dir.join("more/BigCJK.otf"), &big).unwrap();
        // Left out: a placeholder for everything, a hidden font, a file that is no font.
        let all: String = ('\u{2300}'..='\u{23FF}').chain('一'..='龥').collect();
        let last = stand_in::font_with(hack, &all, 'X', "LastResort");
        std::fs::write(dir.join("LastResort.otf"), last).unwrap();
        let hidden = stand_in::font_with(hack, &all, 'X', ".Hidden UI");
        std::fs::write(dir.join("Hidden.ttf"), hidden).unwrap();
        std::fs::write(dir.join("junk.ttf"), b"not a font").unwrap();
        let f = Fallbacks::find(&[dir.join(""), dir.join("nowhere")]);
        assert_eq!(f.len(), 2);
        for c in keys.chars() {
            let got = f.for_char(c).unwrap_or_else(|| panic!("{c}"));
            assert_eq!(got.family, "Key Symbols", "{c}");
            assert!(got.path.ends_with("KeySymbols.ttf"));
        }
        assert_eq!(f.for_char('中').unwrap().family, "Big CJK");
        assert_eq!(f.for_char('文').unwrap().family, "Big CJK");
        assert!(
            f.for_char('⏰').is_none(),
            "only LastResort and the hidden font have it"
        );
        assert!(f.for_char('A').is_none(), "no font found maps it");
        // A stand-in parses as the font it says it is.
        let parsed = skrifa::FontRef::new(&small).unwrap();
        assert!(parsed.charmap().map('⌘').is_some());
        assert!(parsed.charmap().map('A').is_none());
        assert!(Fallbacks::find(&[]).is_empty());
    }

    /// How long reading the installed fonts' character maps takes, here (`--ignored`): the
    /// system's folders, or those in `JETCARP_FONT_DIRS` (separated as `PATH` is).
    #[test]
    #[ignore = "a measurement of this machine's installed fonts"]
    fn time_the_fallback_scan() {
        let dirs: Vec<PathBuf> = match std::env::var_os("JETCARP_FONT_DIRS") {
            Some(v) => std::env::split_paths(&v).collect(),
            None => system_dirs(),
        };
        let files = font_files(&dirs).len();
        for run in ["first", "second"] {
            let t0 = std::time::Instant::now();
            let f = Fallbacks::find(&dirs);
            let took = t0.elapsed();
            let ranges: usize = f.faces.iter().map(|(r, _)| r.len()).sum();
            eprintln!(
                "{run} run: {files} files, {} faces, {ranges} ranges, {took:?}",
                f.len()
            );
        }
        let f = Fallbacks::find(&dirs);
        for c in "⌃⌘⌥⌦⌫★中한".chars() {
            let got = f
                .for_char(c)
                .map(|f| (f.family.clone(), file_name(&f.path)));
            eprintln!("{c}: {got:?}");
        }
        let t0 = std::time::Instant::now();
        let families = scan(&dirs).len();
        eprintln!(
            "the settings' scan: {families} families, {:?}",
            t0.elapsed()
        );
        // What the app's start waits for: the thread that reads them being started.
        let t0 = std::time::Instant::now();
        let thread = std::thread::spawn(move || Fallbacks::find(&dirs).len());
        let started = t0.elapsed();
        let n = thread.join().unwrap();
        eprintln!("starting the thread: {started:?} ({n} faces found on it)");
    }
}

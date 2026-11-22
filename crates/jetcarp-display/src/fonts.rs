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
    for face in font_files(dirs)
        .iter()
        .flat_map(|p| faces_in(p, false))
    {
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
}

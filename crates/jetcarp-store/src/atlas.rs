//! The atlas: the published result of a search, the file Quick Start browses. Entries are
//! selected from a search database, given a category and palette, and stored with a mature
//! state so a preview can start where the action is. This is the reading half (finding the
//! file and loading entries); choosing and writing entries is `jetcarp_search::explore::atlas`.

use flate2::read::DeflateDecoder;
use jetcarp_core::world::{Scene, World};
use rusqlite::{Connection, params};
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};

fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut d = DeflateDecoder::new(data);
    let mut out = Vec::new();
    d.read_to_end(&mut out).ok()?;
    Some(out)
}

/// One published entry.
#[derive(Clone, Debug)]
pub struct AtlasEntry {
    pub id: i64,
    pub title: String,
    pub category: String,
    pub family: String,
    pub tiling: String,
    pub tiling_group: String,
    pub rule: String,
    pub seed: String,
    pub score: f64,
    /// A scene that runs on the small grid the entry was found on (for previews).
    pub preview: Scene,
    /// The same rule and start on the larger grid it was confirmed on (what "open" loads).
    pub full: Scene,
    /// Generation of the mature state.
    pub mature_gen: u32,
    /// The mature states, deflated, or `None` if the preview starts from the seed.
    pub mature: Option<Vec<u8>>,
}

/// What the app needs to list an entry: everything but its scenes and mature state, so a
/// thousands-long menu holds no scene and no state in memory. [`load_card`] and [`load_full`]
/// read those by id when a card is visible or an entry is opened.
#[derive(Clone, Debug, PartialEq)]
pub struct AtlasMeta {
    pub id: i64,
    pub title: String,
    pub category: String,
    pub family: String,
    pub tiling: String,
    pub tiling_group: String,
    pub rule: String,
    pub seed: String,
    pub score: f64,
    pub mature_gen: u32,
    pub has_mature: bool,
}

/// The file in this order: `$JETCARP_ATLAS` (when set to an existing file),
/// `<data>/explore/atlas.sqlite` ([`jetcarp_core::paths::explore_dir`]), then `scenes/atlas.sqlite`
/// found walking up from the executable (a copy shipped beside it).
pub fn locate() -> Option<PathBuf> {
    let env = std::env::var_os("JETCARP_ATLAS");
    let explore = jetcarp_core::paths::explore_dir();
    let exe = std::env::current_exe().ok();
    locate_from(env.as_deref(), Some(&explore), exe.as_deref())
}

/// [`locate`] with the three places given (for tests): `explore` is the search databases' folder.
pub fn locate_from(
    env: Option<&OsStr>,
    explore: Option<&Path>,
    exe: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(p) = env.filter(|p| !p.is_empty()).map(PathBuf::from)
        && p.is_file()
    {
        return Some(p);
    }
    if let Some(d) = explore {
        let p = d.join("atlas.sqlite");
        if p.is_file() {
            return Some(p);
        }
    }
    let mut dir = exe?.parent();
    while let Some(d) = dir {
        let p = d.join("scenes").join("atlas.sqlite");
        if p.is_file() {
            return Some(p);
        }
        dir = d.parent();
    }
    None
}

/// An open atlas file (a read-only SQLite connection).
pub type AtlasConn = Connection;

/// Open an atlas file for reading.
pub fn open(path: &Path) -> Result<AtlasConn, String> {
    Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|x| x.to_string())
}

/// Everything but the scenes and mature state of every entry, best score first.
pub fn load_meta(path: &Path) -> Result<Vec<AtlasMeta>, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let c = open(path)?;
    let mut st = c
        .prepare(
            "SELECT id,title,category,family,tiling,tiling_group,rule,seed,score,mature_gen,
             mature IS NOT NULL FROM entries ORDER BY score DESC, id",
        )
        .map_err(e)?;
    let rows = st
        .query_map([], |r| {
            Ok(AtlasMeta {
                id: r.get(0)?,
                title: r.get(1)?,
                category: r.get(2)?,
                family: r.get(3)?,
                tiling: r.get(4)?,
                tiling_group: r.get(5)?,
                rule: r.get(6)?,
                seed: r.get(7)?,
                score: r.get(8)?,
                mature_gen: r.get::<_, i64>(9)? as u32,
                has_mature: r.get(10)?,
            })
        })
        .map_err(e)?;
    Ok(rows.flatten().collect())
}

/// What a card needs to start: the preview scene and the mature state, still deflated.
pub struct AtlasCard {
    pub preview: Scene,
    pub mature_gen: u32,
    pub mature: Option<Vec<u8>>,
}

/// Read one card by entry id.
pub fn load_card(conn: &AtlasConn, id: i64) -> Result<AtlasCard, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let (preview, mature_gen, mature): (String, i64, Option<Vec<u8>>) = conn
        .query_row(
            "SELECT preview, mature_gen, mature FROM entries WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(e)?;
    Ok(AtlasCard {
        preview: serde_json::from_str(&preview).map_err(|x| x.to_string())?,
        mature_gen: mature_gen as u32,
        mature,
    })
}

/// The full scene of an entry: what "open" loads.
pub fn load_full(path: &Path, id: i64) -> Result<Scene, String> {
    let c = open(path)?;
    let full: String = c
        .query_row("SELECT full FROM entries WHERE id=?1", params![id], |r| {
            r.get(0)
        })
        .map_err(|x| x.to_string())?;
    serde_json::from_str(&full).map_err(|x| x.to_string())
}

/// A readable account of a seed (the search's JSON), for captions and text search: `noise
/// pattern, density 0.85` or `random, density 0.35`. Never fails: an unknown seed stays as it is.
pub fn describe_seed(json: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return json.to_string();
    };
    let num = |x: Option<&serde_json::Value>, name: &str| {
        x.and_then(|d| d.as_f64())
            .map(|d| format!(", {name} {}", (d * 100.0).round() / 100.0))
    };
    match &v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(o) => match o.iter().next() {
            Some((kind, inner)) if kind == "pattern" => {
                let shape = inner
                    .pointer("/shape/kind")
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                let d = num(inner.pointer("/fill/density"), "density").unwrap_or_default();
                format!("{shape} pattern{d}")
            }
            Some((kind, inner)) => {
                let d = num(inner.get("density"), "density")
                    .or_else(|| num(inner.get("count"), "count"))
                    .or_else(|| num(inner.get("radius"), "radius"))
                    .unwrap_or_default();
                format!("{kind}{d}")
            }
            None => "default seed".to_string(),
        },
        _ => json.to_string(),
    }
}

/// Load all entries of an atlas.
pub fn load(path: &Path) -> Result<Vec<AtlasEntry>, String> {
    let e = |x: rusqlite::Error| x.to_string();
    let c = open(path)?;
    let mut st = c
        .prepare(
            "SELECT id,title,category,family,tiling,tiling_group,rule,seed,score,preview,full,
             mature_gen,mature FROM entries ORDER BY score DESC",
        )
        .map_err(e)?;
    let rows = st
        .query_map([], |r| {
            let (prev, full): (String, String) = (r.get(9)?, r.get(10)?);
            Ok(AtlasEntry {
                id: r.get(0)?,
                title: r.get(1)?,
                category: r.get(2)?,
                family: r.get(3)?,
                tiling: r.get(4)?,
                tiling_group: r.get(5)?,
                rule: r.get(6)?,
                seed: r.get(7)?,
                score: r.get(8)?,
                preview: serde_json::from_str(&prev).unwrap_or_else(|_| {
                    Scene::new(jetcarp_core::topology::TilingSpec::new("square"))
                }),
                full: serde_json::from_str(&full).unwrap_or_else(|_| {
                    Scene::new(jetcarp_core::topology::TilingSpec::new("square"))
                }),
                mature_gen: r.get::<_, i64>(11)? as u32,
                mature: r.get(12)?,
            })
        })
        .map_err(e)?;
    Ok(rows.flatten().collect())
}

/// The world of an entry at its mature state (from the seed when it has none).
pub fn preview_world(entry: &AtlasEntry) -> Result<World, String> {
    let mut w = World::from_scene(&entry.preview)?;
    if let Some(blob) = &entry.mature
        && let Some(state) = inflate(blob)
    {
        apply_mature(&mut w, &state, entry.mature_gen);
    }
    Ok(w)
}

/// Inflate a deflated mature state (a card keeps the result only while it is on screen).
pub fn inflate_state(blob: &[u8]) -> Option<Vec<u8>> {
    inflate(blob)
}

/// Put a mature state into a freshly built world, when it fits.
pub fn apply_mature(w: &mut World, state: &[u8], generation: u32) {
    if let Some(f) = w.fields.first_mut()
        && f.state.len() == state.len()
    {
        f.state.copy_from_slice(state);
        for (a, &s) in f.age.iter_mut().zip(state) {
            *a = (s != 0) as u16;
        }
        w.generation = generation as u64;
    }
}

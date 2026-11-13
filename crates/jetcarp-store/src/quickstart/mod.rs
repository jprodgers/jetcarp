//! Quick Start: every entry the app can open, in the library's one `quickstart` table
//! ([`crate::library::quickstart`]): the built-in entries, entries from packs, the atlas a search
//! publishes, and promotions. This module makes the built-in rows ([`builtin_rows`], written
//! when [`builtin_version`] changes), promotes a library pattern ([`promote`]), reads an entry's
//! scene and card by handle ([`scene`], [`card`]), and names what the menu shows: categories,
//! grid groups and rule families. The menu itself is [`menu`].

pub mod menu;

pub use crate::library::{QsSource, QuickStartRow};

use crate::{Library, Update};
use flate2::read::DeflateDecoder;
use jetcarp_core::explore::space::Fam;
use jetcarp_core::pattern::CurationState;
use jetcarp_core::quickstart::{COMPUTING, QuickStart};
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::TilingSpec;
use jetcarp_core::world::{Scene, World};
use std::io::Read;
use std::sync::OnceLock;

/// The built-in entries as rows (`source` builtin), in the build's order
/// (`jetcarp_core::quickstart::entries`: featured scenes first, then one per named rule, the JACA
/// presets and the tiling showcases), each under its built-in id as id and handle.
pub fn builtin_rows() -> &'static [QuickStartRow] {
    static ROWS: OnceLock<Vec<QuickStartRow>> = OnceLock::new();
    ROWS.get_or_init(|| {
        let mut seen = std::collections::HashSet::new();
        jetcarp_core::quickstart::entries()
            .into_iter()
            .filter(|q| seen.insert(q.id.clone()))
            .enumerate()
            .map(|(k, q)| builtin_row(q, k))
            .collect()
    })
}

fn builtin_row(q: QuickStart, rank: usize) -> QuickStartRow {
    let category = classic_category(&q).to_string();
    let family = if q.family == COMPUTING {
        COMPUTING.to_string()
    } else {
        family_of_rule(&q.rule, &q.family)
    };
    let mut r = QuickStartRow::for_scene(&q.id, "scene", q.scene);
    r.handle = q.id;
    r.source = QsSource::Builtin;
    r.source_ref = q.source.unwrap_or_default();
    r.category = category;
    r.family = family;
    r.tiling_name = q.tiling;
    r.rule = q.rule;
    r.rank = rank as i64;
    r.featured = q.featured;
    r
}

/// The build's built-in version, which a library keeps in `meta.builtin_version` when it has
/// these built-in rows: the engine ([`jetcarp_core::pin::engine_geometry`]) and the first 12 hex
/// digits of a SHA-256 over every built-in row's contents.
pub fn builtin_version() -> &'static str {
    static V: OnceLock<String> = OnceLock::new();
    V.get_or_init(|| {
        let mut h = jetcarp_core::sha256::Sha256::new();
        for r in builtin_rows() {
            let v = serde_json::json!([
                r.handle,
                r.source_ref,
                r.title,
                r.description,
                r.category,
                r.grid_group,
                r.family,
                r.tiling_key,
                r.tiling_name,
                r.rule,
                r.rule_family,
                r.behaviour,
                r.scene.to_json(),
                r.rank,
                r.featured,
            ]);
            h.update(v.to_string().as_bytes());
            h.update(b"\n");
        }
        let hex = jetcarp_core::sha256::hex(&h.finish());
        format!("{}+b{}", jetcarp_core::pin::engine_geometry(), &hex[..12])
    })
}

/// Promote a library pattern: a Quick Start entry for it (id the pattern's id, handle
/// `find:<id>`; promoting again replaces it), and the pattern's state becomes promoted. Returns
/// the scene and the entry's id.
pub fn promote(
    lib: &Library,
    key: &str,
    title: Option<&str>,
    description: Option<&str>,
) -> Result<(Scene, String), String> {
    let rec = lib.get(key)?;
    let sc = jetcarp_core::pattern::promote::showcase_scene(&rec, title, description)?;
    let mut row = QuickStartRow::for_scene(&rec.id, "pattern", sc.clone());
    row.target_id = Some(rec.id.clone());
    row.tiling_name = rec.tiling_name.clone();
    row.behaviour = rec.class.name().to_string();
    row.category.clear();
    row.score = Some(rec.score);
    lib.put_quickstart(&row)?;
    lib.update(
        &rec.id,
        &Update {
            state: Some(CurationState::Promoted),
            ..Default::default()
        },
    )?;
    Ok((sc, rec.id))
}

/// A built-in row by handle (or id), as this build makes it, when the library has no file yet:
/// what that library holds once it is made. Reading never makes a library's file (its first
/// write does, and opening it writes the built-ins).
pub fn unmade_builtin(lib: &Library, key: &str) -> Option<&'static QuickStartRow> {
    if lib.path().exists() {
        return None;
    }
    let key = key.trim();
    builtin_rows()
        .iter()
        .find(|r| r.handle == key || r.id == key)
}

/// An entry's scene by handle (or id): what opening it loads.
pub fn scene(lib: &Library, key: &str) -> Result<Scene, String> {
    match unmade_builtin(lib, key) {
        Some(r) => Ok(r.scene.clone()),
        None => lib.quickstart_scene(key),
    }
}

/// What an entry's card runs: a small scene, and the state (inflated) and generation it starts
/// from, if it has one.
#[derive(Clone, Debug)]
pub struct Card {
    pub preview: Scene,
    pub mature_gen: u32,
    pub mature: Option<Vec<u8>>,
    /// The entry has a preview of its own (an atlas entry's small grid); otherwise `preview` is
    /// its scene made smaller.
    pub own_preview: bool,
}

/// An entry's card by handle (or id): its own preview, or its scene made smaller (about 6,000
/// cells, more for long-range rules).
pub fn card(lib: &Library, key: &str) -> Result<Card, String> {
    let (scene, preview, mature_gen, mature) = match unmade_builtin(lib, key) {
        Some(r) => (
            r.scene.clone(),
            r.preview.clone(),
            r.mature_gen,
            r.mature.clone(),
        ),
        None => lib.quickstart_card(key)?,
    };
    let own_preview = preview.is_some();
    let preview = preview.unwrap_or_else(|| {
        let cells = jetcarp_core::quickstart::preview_cells(&scene, 6000).unwrap_or(6000);
        jetcarp_core::quickstart::preview_scene(&scene, cells)
    });
    Ok(Card {
        preview,
        mature_gen,
        mature: mature.as_deref().and_then(inflate_state),
        own_preview,
    })
}

// ---------------------------------------------------------------------------------------------
// Mature states

/// Inflate a deflated mature state.
pub fn inflate_state(blob: &[u8]) -> Option<Vec<u8>> {
    let mut d = DeflateDecoder::new(blob);
    let mut out = Vec::new();
    d.read_to_end(&mut out).ok()?;
    Some(out)
}

/// Deflate a mature state for the table.
pub fn deflate_state(state: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::DeflateEncoder;
    use std::io::Write;
    let mut e = DeflateEncoder::new(Vec::with_capacity(state.len() / 4), Compression::best());
    let _ = e.write_all(state);
    e.finish().unwrap_or_default()
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

// ---------------------------------------------------------------------------------------------
// What the menu calls things

/// The categories shown first in the sidebar, in order; a category not listed follows.
pub const CATEGORY_ORDER: [&str; 23] = [
    "My finds",
    "Gliders & ships",
    "Waves & spirals",
    "Spots & stripes",
    "Oscillators",
    "Turbulence",
    "Living textures",
    "Wildfire",
    "Labyrinths",
    "Growth",
    "Fractals & rows",
    "Battles",
    "Ants & turmites",
    "Sandpiles",
    "Long fades",
    "Ash & debris",
    "Static",
    "Quiet rows",
    "Quick deaths",
    "Frozen at once",
    "Classics",
    "Computing",
    "Packs",
];

/// The grid groups in order, for the sidebar.
pub const GRID_ORDER: [&str; 8] = [
    "Squares",
    "Hexagons",
    "Triangles",
    "Archimedean",
    "k-uniform",
    "Isohedral",
    "Aperiodic",
    "Other periodic",
];

/// A short readable name for a rule family (the explore search's and the rule catalog's
/// merged).
pub fn family_label(fam: Fam) -> &'static str {
    match fam {
        Fam::LifeLike => "Life-like",
        Fam::Generations => "Generations",
        Fam::Cyclic => "Cyclic",
        Fam::Rps => "Rock-paper-scissors",
        Fam::GreenbergHastings => "Greenberg-Hastings",
        Fam::Hodgepodge => "Hodgepodge",
        Fam::WireWorld => "WireWorld",
        Fam::Ltl => "Larger than Life",
        Fam::Jaca => "JACA",
        Fam::Priority => "Priority",
        Fam::ForestFire => "Forest fire",
        Fam::Turmite => "Agents",
        Fam::Layered => "Layered",
        Fam::Wolfram => "Elementary",
        Fam::HexWolfram | Fam::HexTri => "Hexagonal elementary",
        Fam::Margolus => "Margolus block",
        Fam::Lenia => "Continuous",
        Fam::Sandpile => "Sandpile",
        Fam::Table => "Table",
        Fam::VonNeumann => "Construction",
        Fam::SpectreGlider => "Spectre glider",
    }
}

/// The family label for the explore search's family name (`life_like`, ...).
pub fn family_label_named(name: &str) -> String {
    jetcarp_core::explore::space::ALL_FAMS
        .iter()
        .find(|f| f.name() == name)
        .map(|f| family_label(*f).to_string())
        .unwrap_or_else(|| name.to_string())
}

/// The family of a rule, or `fallback` when it does not parse.
pub fn family_of_rule(rule: &str, fallback: &str) -> String {
    match Rule::parse(rule) {
        Ok((r, _)) => family_label(Fam::of(&r)).to_string(),
        Err(_) => fallback.to_string(),
    }
}

/// The grid group for the explore search's tiling group name (`square`, `k-uniform`, ...).
pub fn group_label(group: &str) -> &'static str {
    match group {
        "square" => "Squares",
        "hexagon" => "Hexagons",
        "triangle" => "Triangles",
        "archimedean" => "Archimedean",
        "k-uniform" => "k-uniform",
        "isohedral" => "Isohedral",
        "aperiodic" => "Aperiodic",
        _ => "Other periodic",
    }
}

/// Display name of an explore search's tiling key (`square`, `ih28`, `catalog:k2-001`).
pub fn tiling_label(key: &str) -> String {
    if key == "square" {
        return "Square".into();
    }
    if let Some(id) = key.strip_prefix("catalog:") {
        return jetcarp_core::catalog::get(id)
            .map_or_else(|| key.to_string(), |t| t.id.to_string());
    }
    jetcarp_core::families::family(key)
        .or_else(|| jetcarp_core::isohedral::family(key))
        .map_or_else(|| key.to_string(), |f| f.name.to_string())
}

/// The grid group of a tiling.
pub fn grid_group(t: &TilingSpec) -> &'static str {
    use jetcarp_core::families::FamilyKind;
    let f = t.family.as_str();
    match f {
        "square" => return "Squares",
        "hex" | "hex_flat" => return "Hexagons",
        "triangle" => return "Triangles",
        "oct" | "trihex" | "elongtri" | "snubsq" | "snubhex" | "rhombitrihex" | "trunchex"
        | "trunctrihex" => return "Archimedean",
        "periodic" => {
            let id = t
                .params
                .as_ref()
                .and_then(|p| p.get("catalog"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if id.starts_with("ih") {
                return "Isohedral";
            }
            return "k-uniform";
        }
        _ => {}
    }
    if jetcarp_core::isohedral::family(f).is_some() {
        return "Isohedral";
    }
    match jetcarp_core::families::family(f).map(|x| x.kind) {
        Some(FamilyKind::Aperiodic) => "Aperiodic",
        _ => "Other periodic",
    }
}

/// Text before a family name in a title made from a category.
fn category_mood(category: &str) -> &'static str {
    match category {
        "Spots & stripes" => "Patterned",
        "Gliders & ships" => "Gliding",
        "Oscillators" => "Oscillating",
        "Waves & spirals" => "Wave",
        "Labyrinths" => "Labyrinth",
        "Turbulence" => "Turbulent",
        "Living textures" => "Living",
        "Growth" => "Growing",
        "Ash & debris" => "Ash",
        "Static" => "Static",
        "Long fades" => "Fading",
        "Quick deaths" => "Short-lived",
        "Frozen at once" => "Frozen",
        "Battles" => "Battle",
        "Fractals & rows" => "Fractal",
        "Quiet rows" => "Quiet",
        _ => "",
    }
}

/// An atlas entry's title: its rule's name in the rule catalog, a notation that reads as a name
/// (`Rule 150`, `Ant RRL`, `Turmite LRRL`), a mood (from its category) and a short notation
/// (`Oscillating B2/S13/C7`), or a mood and its family (`Turbulent Larger than Life`).
pub fn atlas_title(rule: &str, category: &str, family: &str) -> String {
    static NAMED: OnceLock<std::collections::HashMap<&'static str, &'static str>> = OnceLock::new();
    let named = NAMED.get_or_init(|| {
        jetcarp_core::rules::catalog()
            .iter()
            .map(|e| (e.notation, e.name))
            .collect()
    });
    if let Some(n) = named.get(rule) {
        return (*n).to_string();
    }
    if let Some(n) = rule
        .strip_prefix('W')
        .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    {
        return format!("Rule {n}");
    }
    if (rule.starts_with("Ant ") || rule.starts_with("Turmite ")) && rule.len() <= 24 {
        return rule.to_string();
    }
    let mood = category_mood(category);
    // A rule table (Priority) has no short notation; its number of states tells some apart
    // (`Oscillating Priority 6`).
    let rule = match rule.split_once(':') {
        Some((head, _)) if head.starts_with("Priority ") => head,
        _ => rule,
    };
    // A short notation tells entries of one family apart (`Battle JACA +B3/S23 -B36/S23`,
    // `Fractal HexW3 5033`); a long one (Larger than Life) gives way to the family. JACA's two
    // rules and blast still read as a name at 40 characters.
    let limit = if rule.starts_with("JACA ") { 40 } else { 26 };
    if rule.len() <= limit && rule != family {
        return if mood.is_empty() {
            rule.to_string()
        } else {
            format!("{mood} {rule}")
        };
    }
    if mood.is_empty() {
        family.to_string()
    } else {
        format!("{mood} {family}")
    }
}

/// The nearest category for a built-in entry.
pub fn classic_category(q: &QuickStart) -> &'static str {
    if q.family == COMPUTING {
        return "Computing";
    }
    let Some(ca) = q.scene.cas.first() else {
        return "Classics";
    };
    let Ok((r, _)) = Rule::parse(&ca.rule) else {
        return "Classics";
    };
    if r.is_rows() {
        return "Fractals & rows";
    }
    if r.is_agent() {
        return "Ants & turmites";
    }
    match r {
        Rule::Sandpile => "Sandpiles",
        Rule::Lenia(_) => "Living textures",
        Rule::VonNeumann => "Computing",
        _ => "Classics",
    }
}

/// The category of a pattern's class or an ant's outcome, when it has one.
fn find_category(behaviour: &str) -> Option<&'static str> {
    Some(match behaviour {
        "spaceship" | "spaceships" | "mover" | "movers" | "gun" | "guns" | "puffer" | "puffers" => {
            "Gliders & ships"
        }
        "oscillator" | "oscillators" => "Oscillators",
        "still life" | "still lifes" => "Static",
        _ => return None,
    })
}

/// The category of an entry of `source` with this behaviour: a category the menu knows (as an
/// atlas entry carries through a pack), a pattern's class mapped to one, else "My finds" for a
/// promotion, "Packs" for a pack's entry, "Classics" for a built-in.
pub fn category_of(source: QsSource, behaviour: &str) -> String {
    if source != QsSource::Promotion
        && let Some(c) = CATEGORY_ORDER.iter().find(|c| **c == behaviour)
    {
        return c.to_string();
    }
    if let Some(c) = find_category(behaviour) {
        return c.into();
    }
    match source {
        QsSource::Promotion => "My finds",
        QsSource::Pack => "Packs",
        QsSource::Builtin => "Classics",
        QsSource::Atlas => "Living textures",
    }
    .into()
}

/// A rule's notation and neighbourhood in canonical form, or its text when it does not parse.
pub fn canonical_rule(rule: &str) -> String {
    match Rule::parse(rule) {
        Ok((r, nb)) => format!("{}|{nb:?}", r.notation()),
        Err(_) => rule.trim().to_string(),
    }
}

/// A readable account of a seed (the search's JSON), for descriptions and text search: `noise
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

//! The Quick Start menu: every entry the app can open, from three sources, with the categories,
//! search, sorting, opening rows, and ratings the Quick Start screen uses.
//!
//! **Sources** ([`Origin`]): the **Atlas** (`explore::atlas`, the published result of the long
//! search: only its metadata is held, a card reads its preview by id when it is on screen and an
//! open reads the full scene by id); **My finds** (catalog `quickstart` rows made from patterns,
//! sweep runs, and scenes, status `auto` or `approved`); and **Classics** (the built-in entries,
//! `jetcarp_core::quickstart::entries()`, kept only for rules the atlas does not cover, and always
//! for the hand-made featured scenes, so every named rule is reachable). Catalog rows with any
//! other status (`candidate`, `rejected`) are hidden.
//!
//! **Categories**: the behaviour (the atlas's category; a built-in or a find gets the nearest
//! one, or "Classics" / "My finds"), the grid group ([`grid_group`]: squares, hexagons,
//! triangles, Archimedean, k-uniform, isohedral, aperiodic, other), and the rule family
//! ([`family_label`]: Life-like, Generations, ...).
//!
//! **Order**: [`Sort::Featured`] is a seeded [`shuffle`] of the best entries of every category
//! (the app draws a new seed at each start and on "Shuffle"); Score, Rule, and Tiling are
//! plain sorts. Filtering is [`select`] and the sidebar's counts are [`facets_for`]: each dimension
//! is counted with the filters of the others applied.
//!
//! **Opening rows.** [`popular`]: entries opened at least once, most opens first, then the
//! user's rating, then the latest open. [`recent`]: latest open first. [`random`]: a seeded draw
//! from the whole menu.
//!
//! **Usage and ratings.** Opening an entry calls `Library::note_quickstart_open` with its id
//! (`find:<id>`, `atlas:<id>`, `rule:<notation>`, a bundled name). A rating goes to the most
//! specific target ([`rating_target`]).

use crate::atlas::{self, AtlasMeta};
use crate::quickstart;
use crate::{Library, QsStatus, QuickStartRow, UsageRow};
use jetcarp_core::explore::space::Fam;
use jetcarp_core::quickstart::QuickStart;
use jetcarp_core::rng::Rng;
use jetcarp_core::rules::Rule;
use jetcarp_core::topology::TilingSpec;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where an entry comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Origin {
    Atlas,
    MyFinds,
    Classics,
}

impl Origin {
    pub const ALL: [Origin; 3] = [Origin::Atlas, Origin::MyFinds, Origin::Classics];
    pub fn label(self) -> &'static str {
        match self {
            Origin::Atlas => "Atlas",
            Origin::MyFinds => "My finds",
            Origin::Classics => "Classics",
        }
    }
}

/// How to read an atlas entry's preview and full scene from its file.
#[derive(Clone, Debug)]
pub struct AtlasRef {
    pub path: Arc<PathBuf>,
    pub id: i64,
}

/// One entry of the menu.
#[derive(Clone, Debug)]
pub struct Item {
    /// The entry as the app opens it (id, scene, rule, tiling name). An atlas entry's scene here
    /// is an empty stand-in: its real scenes are read through [`Item::atlas`] on demand.
    pub q: QuickStart,
    pub title: String,
    pub description: String,
    pub origin: Origin,
    pub grid_group: String,
    /// Rule family (see [`family_label`]).
    pub family: String,
    /// The category (the atlas's, or the nearest for a built-in or a find).
    pub behaviour: String,
    pub score: Option<f64>,
    /// When it was added (empty for built-ins).
    pub created: String,
    /// The catalog row, for catalog entries.
    pub row: Option<QuickStartRow>,
    /// The file to read an atlas entry's scenes from.
    pub atlas: Option<AtlasRef>,
    /// The user's latest rating.
    pub rating: Option<f64>,
    pub opens: u64,
    pub last_opened: Option<String>,
    /// Position in [`Sort::Featured`] (see [`shuffle`]).
    pub shuffle_rank: u32,
    /// Built-in order (featured first), then catalog order.
    pub rank: usize,
    /// Lower-case text that search looks in.
    hay: String,
}

impl Item {
    /// The id the app opens and counts it by.
    pub fn id(&self) -> &str {
        &self.q.id
    }
    /// Does it match every word of a search?
    pub fn matches(&self, text: &str) -> bool {
        text.split_whitespace()
            .all(|w| self.hay.contains(&w.to_lowercase()))
    }
}

/// A short readable name for a rule family (the atlas's and the rule catalog's merged).
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

/// The family label for the atlas's family name (`life_like`, ...).
pub fn family_label_named(name: &str) -> String {
    jetcarp_core::explore::space::ALL_FAMS
        .iter()
        .find(|f| f.name() == name)
        .map(|f| family_label(*f).to_string())
        .unwrap_or_else(|| name.to_string())
}

/// The family of a rule, or `fallback` when it does not parse.
fn family_of_rule(rule: &str, fallback: &str) -> String {
    match Rule::parse(rule) {
        Ok((r, _)) => family_label(Fam::of(&r)).to_string(),
        Err(_) => fallback.to_string(),
    }
}

/// The grid group for the atlas's tiling group name.
fn group_label(group: &str) -> &'static str {
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

/// Display name of a tiling key (`square`, `ih28`, `catalog:k2-001`), memoized by the caller.
fn tiling_label(key: &str) -> String {
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

/// Text before a family name in a title made from a category.
fn category_mood(category: &str) -> &'static str {
    match category {
        "Spots & stripes" => "Patterned",
        "Wildfire" => "",
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
        "Ants & turmites" => "",
        "Fractals & rows" => "Fractal",
        "Quiet rows" => "Quiet",
        "Sandpiles" => "",
        _ => "",
    }
}

/// A short title: the rule's catalog name, or a mood and family. Filled per atlas entry, with
/// the rule catalog's names memoized in `named`.
fn atlas_title(meta: &AtlasMeta, family: &str, named: &HashMap<&str, &str>) -> String {
    if let Some(n) = named.get(meta.rule.as_str()) {
        return (*n).to_string();
    }
    let mood = category_mood(&meta.category);
    if mood.is_empty() {
        family.to_string()
    } else {
        format!("{mood} {family}")
    }
}

/// The atlas's entries as menu items, sharing the file's path.
pub fn atlas_items(metas: Vec<AtlasMeta>, path: Arc<PathBuf>) -> Vec<Item> {
    let named: HashMap<&str, &str> = jetcarp_core::rules::catalog()
        .iter()
        .map(|e| (e.notation, e.name))
        .collect();
    let mut labels: HashMap<String, String> = HashMap::new();
    let mut fams: HashMap<String, String> = HashMap::new();
    let mut out = Vec::with_capacity(metas.len());
    for (i, m) in metas.into_iter().enumerate() {
        let family = fams
            .entry(m.family.clone())
            .or_insert_with(|| family_label_named(&m.family))
            .clone();
        let tiling = labels
            .entry(m.tiling.clone())
            .or_insert_with(|| tiling_label(&m.tiling))
            .clone();
        let grid_group = group_label(&m.tiling_group).to_string();
        let title = atlas_title(&m, &family, &named);
        let seed = atlas::describe_seed(&m.seed);
        let description = format!("{}\nSeed: {seed}\nScore {:.2}", m.title, m.score);
        let hay = [
            title.as_str(),
            &m.rule,
            &tiling,
            &m.tiling,
            &grid_group,
            &family,
            &m.category,
            &m.title,
            &seed,
            Origin::Atlas.label(),
        ]
        .join("\n")
        .to_lowercase();
        let id = format!("atlas:{}", m.id);
        let q = QuickStart {
            id,
            scene: jetcarp_core::world::Scene::new(TilingSpec::new("square")),
            family: family.clone(),
            tiling,
            states: 0,
            featured: false,
            rule: m.rule.clone(),
            source: None,
        };
        out.push(Item {
            title,
            description,
            origin: Origin::Atlas,
            grid_group,
            family,
            behaviour: m.category,
            score: Some(m.score),
            created: String::new(),
            row: None,
            atlas: Some(AtlasRef {
                path: path.clone(),
                id: m.id,
            }),
            rating: None,
            opens: 0,
            last_opened: None,
            shuffle_rank: i as u32,
            rank: i,
            hay,
            q,
        });
    }
    out
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

/// The nearest category for a built-in entry.
fn classic_category(q: &QuickStart) -> &'static str {
    if q.family == jetcarp_core::quickstart::COMPUTING {
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

/// The category for a catalog row's behaviour (a pattern's class, a sweep's label).
fn find_category(behaviour: &str) -> String {
    match behaviour {
        "spaceship" | "spaceships" | "mover" | "movers" | "gun" | "guns" | "puffer" | "puffers" => {
            "Gliders & ships".into()
        }
        "oscillator" | "oscillators" => "Oscillators".into(),
        "still life" | "still lifes" => "Static".into(),
        _ => "My finds".into(),
    }
}

/// A built-in or catalog entry as a menu item.
fn scene_item(q: QuickStart, origin: Origin, row: Option<QuickStartRow>, rank: usize) -> Item {
    let (title, description, behaviour, score, created) = match &row {
        Some(r) => (
            if r.title.is_empty() {
                q.scene.name.clone()
            } else {
                r.title.clone()
            },
            r.description.clone(),
            find_category(&r.behaviour),
            r.score,
            r.created.clone(),
        ),
        None => (
            q.scene.name.clone(),
            q.scene.description.clone(),
            classic_category(&q).to_string(),
            None,
            String::new(),
        ),
    };
    let family = if q.family == jetcarp_core::quickstart::COMPUTING {
        jetcarp_core::quickstart::COMPUTING.to_string()
    } else {
        family_of_rule(&q.rule, &q.family)
    };
    let grid_group = grid_group(&q.scene.tiling).to_string();
    let hay = [
        title.as_str(),
        &q.rule,
        &q.tiling,
        &grid_group,
        &family,
        &behaviour,
        &description,
        &q.id,
        origin.label(),
    ]
    .join("\n")
    .to_lowercase();
    Item {
        title,
        description,
        origin,
        grid_group,
        family,
        behaviour,
        score,
        created,
        row,
        atlas: None,
        rating: None,
        opens: 0,
        last_opened: None,
        shuffle_rank: rank as u32,
        rank,
        hay,
        q,
    }
}

/// A rule's notation and neighbourhood in canonical form, or its text when it does not parse.
pub fn canonical_rule(rule: &str) -> String {
    match Rule::parse(rule) {
        Ok((r, nb)) => format!("{}|{nb:?}", r.notation()),
        Err(_) => rule.trim().to_string(),
    }
}

/// The built-in entries as menu items (the Classics), leaving out rules in `covered` other than
/// the hand-made featured scenes (the atlas has its own example of those).
pub fn classics_items(covered: &HashSet<String>) -> Vec<Item> {
    jetcarp_core::quickstart::entries()
        .into_iter()
        .filter(|q| q.featured || !covered.contains(&canonical_rule(&q.rule)))
        .enumerate()
        .map(|(i, q)| scene_item(q, Origin::Classics, None, i))
        .collect()
}

/// The built-in entries as menu items, before the atlas has loaded.
pub fn built_in_items() -> Vec<Item> {
    classics_items(&HashSet::new())
}

/// Catalog rows (status `auto` or `approved`) as menu items. Rows the old settings search left
/// in a catalog (`source_kind = 'setting'`) stay out of the menu.
pub fn catalog_items(lib: &Library, rows: Vec<QuickStartRow>) -> Vec<Item> {
    rows.into_iter()
        .filter(|r| r.status.shown() && r.source_kind != "setting")
        .enumerate()
        .map(|(i, r)| {
            let q = quickstart::find_entry(lib, r.clone());
            scene_item(q, Origin::MyFinds, Some(r), i)
        })
        .collect()
}

/// The menu from the catalog and an atlas file (`None` or an unreadable file: no atlas).
pub fn load_with(lib: &Library, atlas_path: Option<&Path>) -> Vec<Item> {
    let rows = lib
        .quickstart(&[QsStatus::Auto, QsStatus::Approved])
        .unwrap_or_default();
    let mut items = catalog_items(lib, rows);
    let path = atlas_path.map(|p| Arc::new(p.to_path_buf()));
    let metas = atlas_path
        .and_then(|p| match atlas::load_meta(p) {
            Ok(m) => Some(m),
            Err(e) => {
                eprintln!("atlas {}: {e}", p.display());
                None
            }
        })
        .unwrap_or_default();
    let covered: HashSet<String> = metas.iter().map(|m| canonical_rule(&m.rule)).collect();
    if let Some(path) = path {
        items.extend(atlas_items(metas, path));
    }
    items.extend(classics_items(&covered));
    let usage = lib.quickstart_usage().unwrap_or_default();
    let ratings = lib.user_ratings().unwrap_or_default();
    apply_usage(&mut items, &usage, &ratings);
    items
}

/// The menu with the atlas found by [`atlas::locate`].
pub fn load(lib: &Library) -> Vec<Item> {
    load_with(lib, atlas::locate().as_deref())
}

/// An entry's scene by id, without loading the whole menu.
pub fn scene_by_id(lib: &Library, id: &str) -> Option<jetcarp_core::Scene> {
    if let Some(n) = id.strip_prefix("atlas:") {
        return atlas::load_full(&atlas::locate()?, n.parse().ok()?).ok();
    }
    if let Some(bare) = id.strip_prefix("find:") {
        return lib.get_quickstart(bare).ok().map(|q| q.scene);
    }
    jetcarp_core::quickstart::entries()
        .into_iter()
        .find(|q| q.id == id)
        .map(|q| q.scene)
}

/// Where a rating of an entry goes: `(target_kind, target_id)`.
pub fn rating_target(id: &str, row: Option<&QuickStartRow>) -> (String, String) {
    let bare = id.trim_start_matches("find:");
    if let Some(r) = row {
        if let ("pattern", Some(p)) = (r.source_kind.as_str(), &r.source_id) {
            return ("pattern".into(), p.clone());
        }
    }
    ("quickstart".into(), bare.to_string())
}

/// Fill in opens, the latest open, and the user's rating.
pub fn apply_usage(
    items: &mut [Item],
    usage: &[UsageRow],
    ratings: &HashMap<(String, String), f64>,
) {
    let by_id: HashMap<&str, &UsageRow> = usage.iter().map(|u| (u.entry_id.as_str(), u)).collect();
    for it in items {
        let u = by_id.get(it.q.id.as_str());
        it.opens = u.map(|u| u.opens).unwrap_or(0);
        it.last_opened = u.and_then(|u| u.last_opened.clone());
        it.rating = ratings
            .get(&rating_target(&it.q.id, it.row.as_ref()))
            .copied();
    }
}

/// Count an opening in memory (the app also writes it to the catalog).
pub fn note_open(items: &mut [Item], id: &str, when: &str) {
    if let Some(it) = items.iter_mut().find(|i| i.q.id == id) {
        it.opens += 1;
        it.last_opened = Some(when.to_string());
    }
}

fn cmp_f(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    // Descending, None last.
    match (a, b) {
        (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// Most opened first, the user's rating breaking ties, then the latest open.
pub fn popular(items: &[Item], n: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..items.len()).filter(|&i| items[i].opens > 0).collect();
    v.sort_by(|&a, &b| {
        let (x, y) = (&items[a], &items[b]);
        y.opens
            .cmp(&x.opens)
            .then(cmp_f(x.rating, y.rating))
            .then(y.last_opened.cmp(&x.last_opened))
            .then(x.q.id.cmp(&y.q.id))
    });
    v.truncate(n);
    v
}

/// Latest open first.
pub fn recent(items: &[Item], n: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..items.len())
        .filter(|&i| items[i].last_opened.is_some())
        .collect();
    v.sort_by(|&a, &b| {
        let (x, y) = (&items[a], &items[b]);
        y.last_opened.cmp(&x.last_opened).then(x.q.id.cmp(&y.q.id))
    });
    v.truncate(n);
    v
}

/// `n` distinct entries drawn from the whole menu with this seed (a partial Fisher-Yates
/// shuffle over the entries in id order, so the draw does not depend on the menu's order).
pub fn random(items: &[Item], seed: u64, n: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..items.len()).collect();
    idx.sort_by(|&a, &b| items[a].q.id.cmp(&items[b].q.id));
    let mut r = Rng::new(seed ^ 0x5157_5f72_616e_646f);
    let n = n.min(idx.len());
    for i in 0..n {
        let j = i + r.below((idx.len() - i) as u64) as usize;
        idx.swap(i, j);
    }
    idx.truncate(n);
    idx
}

/// Entries per category that are shuffled together at a time in [`shuffle`].
const BAND: usize = 40;

/// How many bands a category's entries wait behind the best of the others.
fn band_penalty(it: &Item) -> usize {
    if it.origin == Origin::Classics {
        return if it.q.featured { 1 } else { 6 };
    }
    match it.behaviour.as_str() {
        "Quick deaths" | "Frozen at once" | "Static" | "Quiet rows" => 4,
        "Ash & debris" | "Long fades" => 2,
        _ => 0,
    }
}

/// Assign [`Item::shuffle_rank`], the [`Sort::Featured`] order: the best [`BAND`] entries of every
/// category shuffled together, then the next band, ..., so the top of the screen is a mix of the
/// highest-scoring entries across all categories, different for each seed. My finds rank with
/// the best bands; dull categories wait a few bands, and the Classics after the rest.
pub fn shuffle(items: &mut [Item], seed: u64) {
    let mut by_cat: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        let group = if it.origin == Origin::MyFinds {
            "\u{0}my finds"
        } else if it.origin == Origin::Classics {
            "\u{0}classics"
        } else {
            it.behaviour.as_str()
        };
        by_cat.entry(group).or_default().push(i);
    }
    let mut r = Rng::new(seed ^ 0x7368_7566_666c_6521);
    let mut keys: Vec<(usize, u64, usize)> = Vec::with_capacity(items.len());
    let mut cats: Vec<(&str, Vec<usize>)> = by_cat.into_iter().collect();
    cats.sort_by_key(|c| c.0);
    for (_, mut list) in cats {
        list.sort_by(|&a, &b| {
            cmp_f(items[a].score, items[b].score)
                .then(items[a].rank.cmp(&items[b].rank))
                .then(items[a].q.id.cmp(&items[b].q.id))
        });
        for (pos, i) in list.into_iter().enumerate() {
            keys.push((pos / BAND + band_penalty(&items[i]), r.next_u64(), i));
        }
    }
    keys.sort_unstable_by_key(|k| (k.0, k.1));
    for (rank, (_, _, i)) in keys.into_iter().enumerate() {
        items[i].shuffle_rank = rank as u32;
    }
}

/// How the menu is sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Sort {
    /// The launch's [`shuffle`] of the best entries across all categories.
    #[default]
    Featured,
    Score,
    Rule,
    Tiling,
}

impl Sort {
    pub const ALL: [(Sort, &'static str); 4] = [
        (Sort::Featured, "Featured"),
        (Sort::Score, "Score"),
        (Sort::Rule, "Rule"),
        (Sort::Tiling, "Tiling"),
    ];
    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|x| x.0 == self)
            .map(|x| x.1)
            .unwrap_or("")
    }
}

/// Filters of the menu (empty fields match everything).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    pub text: String,
    pub grid: String,
    pub family: String,
    pub behaviour: String,
    pub origin: Option<Origin>,
}

impl Filter {
    pub fn matches(&self, it: &Item) -> bool {
        (self.grid.is_empty() || it.grid_group == self.grid)
            && (self.family.is_empty() || it.family == self.family)
            && (self.behaviour.is_empty() || it.behaviour == self.behaviour)
            && self.origin.is_none_or(|o| it.origin == o)
            && (self.text.trim().is_empty() || it.matches(&self.text))
    }
    pub fn is_empty(&self) -> bool {
        *self == Filter::default()
    }
}

/// The entries matching a filter, in order.
pub fn select(items: &[Item], f: &Filter, sort: Sort) -> Vec<usize> {
    let mut v: Vec<usize> = (0..items.len()).filter(|&i| f.matches(&items[i])).collect();
    let tie = |a: usize, b: usize| items[a].q.id.cmp(&items[b].q.id);
    match sort {
        Sort::Featured => v.sort_unstable_by_key(|&i| items[i].shuffle_rank),
        Sort::Score => v.sort_unstable_by(|&a, &b| {
            cmp_f(items[a].score, items[b].score)
                .then(items[a].shuffle_rank.cmp(&items[b].shuffle_rank))
                .then(tie(a, b))
        }),
        Sort::Rule => v.sort_unstable_by(|&a, &b| {
            items[a]
                .q
                .rule
                .cmp(&items[b].q.rule)
                .then(cmp_f(items[a].score, items[b].score))
                .then(tie(a, b))
        }),
        Sort::Tiling => v.sort_unstable_by(|&a, &b| {
            items[a]
                .q
                .tiling
                .cmp(&items[b].q.tiling)
                .then(cmp_f(items[a].score, items[b].score))
                .then(tie(a, b))
        }),
    }
    v
}

/// The values of a dimension with their counts: categories in [`CATEGORY_ORDER`] (the striking
/// ones first), others by count.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facets {
    /// Entries matching all the filters but the category.
    pub all: usize,
    pub categories: Vec<(String, usize)>,
    pub grids: Vec<(String, usize)>,
    pub families: Vec<(String, usize)>,
    pub origins: Vec<(Origin, usize)>,
}

/// The categories shown first in the sidebar, in order; a category not listed follows.
pub const CATEGORY_ORDER: [&str; 22] = [
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

fn listed_first(list: &[&str], m: HashMap<&str, usize>) -> Vec<(String, usize)> {
    let mut v: Vec<(&str, usize)> = m.into_iter().collect();
    v.sort_by_key(|&(k, n)| {
        let pos = list.iter().position(|&x| x == k).unwrap_or(usize::MAX);
        (pos, usize::MAX - n, k)
    });
    v.into_iter().map(|(k, n)| (k.to_string(), n)).collect()
}

/// Counts for the sidebar: each dimension with the filters of the others applied, so a click
/// always shows that many entries.
pub fn facets_for(items: &[Item], f: &Filter) -> Facets {
    let mut cats: HashMap<&str, usize> = HashMap::new();
    let mut grids: HashMap<&str, usize> = HashMap::new();
    let mut fams: HashMap<&str, usize> = HashMap::new();
    let mut origins: HashMap<Origin, usize> = HashMap::new();
    let mut all = 0;
    let text = f.text.trim();
    for it in items {
        let t = text.is_empty() || it.matches(text);
        if !t {
            continue;
        }
        let g = f.grid.is_empty() || it.grid_group == f.grid;
        let fa = f.family.is_empty() || it.family == f.family;
        let c = f.behaviour.is_empty() || it.behaviour == f.behaviour;
        let o = f.origin.is_none_or(|o| it.origin == o);
        if g && fa && o {
            all += 1;
            *cats.entry(&it.behaviour).or_default() += 1;
        }
        if fa && c && o {
            *grids.entry(&it.grid_group).or_default() += 1;
        }
        if g && c && o {
            *fams.entry(&it.family).or_default() += 1;
        }
        if g && fa && c {
            *origins.entry(it.origin).or_default() += 1;
        }
    }
    let mut families: Vec<(String, usize)> =
        fams.into_iter().map(|(k, n)| (k.to_string(), n)).collect();
    families.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    Facets {
        all,
        categories: listed_first(&CATEGORY_ORDER, cats)
            .into_iter()
            .filter(|(k, _)| !k.is_empty())
            .collect(),
        grids: listed_first(&GRID_ORDER, grids),
        families,
        origins: Origin::ALL
            .iter()
            .filter_map(|o| origins.get(o).map(|n| (*o, *n)))
            .collect(),
    }
}

/// The values of a category with their counts, most common first.
pub fn facets(items: &[Item], key: impl Fn(&Item) -> &str) -> Vec<(String, usize)> {
    let mut m: HashMap<&str, usize> = HashMap::new();
    for it in items {
        *m.entry(key(it)).or_default() += 1;
    }
    let mut v: Vec<(String, usize)> = m
        .into_iter()
        .filter(|(k, _)| !k.is_empty())
        .map(|(k, n)| (k.to_string(), n))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

// ---------------------------------------------------------------------------------------------
// Ratings

/// Rate an entry as the user, 1 to 5 (see [`rating_target`]).
pub fn rate_entry(lib: &Library, row: &QuickStartRow, stars: u8) -> Result<(), String> {
    if !(1..=5).contains(&stars) {
        return Err("ratings are 1 to 5".into());
    }
    let (kind, id) = rating_target(&row.id, Some(row));
    match kind.as_str() {
        "pattern" => lib.rate_pattern(&id, stars, "").map(|_| ()),
        _ => lib.rate(&kind, &id, "user", stars as f64, "").map(|_| ()),
    }
}

//! The Quick Start menu: every shown entry of the library's `quickstart` table, with the
//! categories, search, sorting, opening rows, and ratings the Quick Start screen uses.
//!
//! **Entries.** [`load`] reads the table only: what each shown entry lists ([`MenuRow`]: no
//! scene, so a menu of thousands holds none in memory; [`super::card`] and [`super::scene`] read
//! those by handle when a card is on screen or an entry is opened). A built-in entry whose rule
//! an atlas entry has is left out, unless it is featured (a hand-made scene, or one picked for
//! the starter set), so every named rule stays reachable without showing it twice.
//!
//! **Categories**: the category (an atlas entry's from its run; a built-in's or a promotion's the
//! nearest one, or "Classics", "My finds", "Packs"), the grid group ([`super::grid_group`]:
//! squares, hexagons, triangles, Archimedean, k-uniform, isohedral, aperiodic, other), the rule
//! family ([`super::family_label`]: Life-like, Generations, ...), and the source ([`QsSource`]).
//!
//! **Order**: [`Sort::Featured`] is a seeded [`shuffle`] of the best entries of every category
//! and the featured built-ins (the app draws a new seed at each start and on "Shuffle", and keeps
//! the order through reloads in between with [`keep_order`]); Score, Rule, and Tiling are plain
//! sorts. Filtering is [`select`] and the sidebar's counts are [`facets_for`]: each dimension is
//! counted with the filters of the others applied.
//!
//! **Opening rows.** [`popular`]: entries opened at least once, most opens first, then the
//! user's rating, then the latest open. [`recent`]: latest open first. [`random`]: a seeded draw
//! from the whole menu.
//!
//! **Usage and ratings.** Opening an entry calls `Library::note_quickstart_open` with its handle.
//! A rating goes to the most specific target ([`rating_target`]): the pattern or ant run an entry
//! was made from, else the entry itself.

use super::{CATEGORY_ORDER, GRID_ORDER};
use crate::library::MenuRow;
use crate::{Library, QsSource, UsageRow};
use jetcarp_core::rng::Rng;
use std::collections::HashMap;

/// One entry of the menu.
#[derive(Clone, Debug)]
pub struct Item {
    /// What it is opened by (and counted by in `quickstart_usage`).
    pub handle: String,
    /// The row's id (what ratings, media and provenance name).
    pub row_id: String,
    pub source: QsSource,
    /// A pack's manifest hash, a search's uuid, a built-in rule's reference.
    pub source_ref: String,
    /// What it was made from: (`pattern`, `ant`, `sweep_run`, `search_run`, `scene`) and its id.
    pub target_kind: String,
    pub target_id: Option<String>,
    pub title: String,
    pub description: String,
    /// The main rule's notation.
    pub rule: String,
    /// The tiling's display name, and its key.
    pub tiling: String,
    pub tiling_key: String,
    pub grid_group: String,
    /// Rule family (see [`super::family_label`]).
    pub family: String,
    /// The category.
    pub behaviour: String,
    pub score: Option<f64>,
    /// A featured built-in: a hand-made scene, or one picked for the starter set.
    pub featured: bool,
    /// It has a small preview scene of its own (an atlas entry): "open at preview size".
    pub has_preview: bool,
    /// When it was added.
    pub created: String,
    /// The user's latest rating.
    pub rating: Option<f64>,
    pub opens: u64,
    pub last_opened: Option<String>,
    /// Position in [`Sort::Featured`] (see [`shuffle`]).
    pub shuffle_rank: u32,
    /// Order within its source (built-ins as built, atlas entries best first, others oldest
    /// first).
    pub rank: usize,
    /// Lower-case text that search looks in.
    hay: String,
}

impl Item {
    /// The handle the app opens and counts it by.
    pub fn id(&self) -> &str {
        &self.handle
    }
    /// Does it match every word of a search?
    pub fn matches(&self, text: &str) -> bool {
        text.split_whitespace()
            .all(|w| self.hay.contains(&w.to_lowercase()))
    }
    /// Where it came from, for a tooltip.
    pub fn credit(&self) -> Option<String> {
        let short = |s: &str| s.chars().take(20).collect::<String>();
        match self.source {
            QsSource::Builtin if !self.source_ref.is_empty() => Some(self.source_ref.clone()),
            QsSource::Builtin => None,
            QsSource::Pack => Some(format!("pack {}", short(&self.source_ref))),
            QsSource::Atlas => Some(format!(
                "explore search {}, run {}",
                short(&self.source_ref),
                self.target_id
                    .as_deref()
                    .and_then(|t| t.rsplit(':').next())
                    .unwrap_or("")
            )),
            QsSource::Promotion => Some(match &self.target_id {
                Some(t) => format!("{} {t}", self.target_kind.replace('_', " ")),
                None => self.target_kind.replace('_', " "),
            }),
        }
    }
}

/// A row of the table as a menu item.
pub fn item(r: MenuRow, rank: usize) -> Item {
    let hay = [
        r.title.as_str(),
        &r.rule,
        &r.tiling_name,
        &r.tiling_key,
        &r.grid_group,
        &r.family,
        &r.category,
        &r.description,
        &r.handle,
        r.source.label(),
    ]
    .join("\n")
    .to_lowercase();
    Item {
        handle: r.handle,
        row_id: r.id,
        source: r.source,
        source_ref: r.source_ref,
        target_kind: r.target_kind,
        target_id: r.target_id,
        title: r.title,
        description: r.description,
        rule: r.rule,
        tiling: r.tiling_name,
        tiling_key: r.tiling_key,
        grid_group: r.grid_group,
        family: r.family,
        behaviour: r.category,
        score: r.score,
        featured: r.featured,
        has_preview: r.has_preview,
        created: r.created,
        rating: None,
        opens: 0,
        last_opened: None,
        shuffle_rank: rank as u32,
        rank,
        hay,
    }
}

/// The menu from rows of the table (shown ones, by rank): promotions, pack entries, the atlas,
/// then the built-ins, each ranked within its source. Nothing is left out: a built-in (a named,
/// hand-made example) and an atlas entry with the same rule (a run of the user's own search)
/// both show.
pub fn items_of(rows: Vec<MenuRow>) -> Vec<Item> {
    let mut by: HashMap<QsSource, Vec<MenuRow>> = HashMap::new();
    for r in rows {
        by.entry(r.source).or_default().push(r);
    }
    let mut out = Vec::new();
    for s in QsSource::ALL {
        let mut list = by.remove(&s).unwrap_or_default();
        list.sort_by(|a, b| {
            a.rank
                .cmp(&b.rank)
                .then(a.created.cmp(&b.created))
                .then(a.id.cmp(&b.id))
        });
        out.extend(list.into_iter().enumerate().map(|(k, r)| item(r, k)));
    }
    out
}

/// The menu: every shown entry of the table, with the user's opens and ratings. A library with
/// no file yet (reading never makes one) shows the built-ins it will hold.
pub fn load(lib: &Library) -> Vec<Item> {
    let rows = if lib.path().exists() {
        lib.quickstart_menu_rows().unwrap_or_else(|e| {
            eprintln!("Quick Start: {e}");
            vec![]
        })
    } else {
        super::builtin_rows().iter().map(MenuRow::of).collect()
    };
    let mut items = items_of(rows);
    let usage = lib.quickstart_usage().unwrap_or_default();
    let ratings = lib.user_ratings().unwrap_or_default();
    apply_usage(&mut items, &usage, &ratings);
    items
}

/// Where a rating of an entry goes, `(target_kind, target_id)`: the pattern or ant run it was
/// made from, else the entry (by its row id).
pub fn rating_target(it: &Item) -> (String, String) {
    match (it.target_kind.as_str(), &it.target_id) {
        ("pattern", Some(p)) => ("pattern".into(), p.clone()),
        ("ant", Some(a)) => ("ant".into(), a.clone()),
        _ => ("quickstart".into(), it.row_id.clone()),
    }
}

/// Fill in opens, the latest open, and the user's rating.
pub fn apply_usage(
    items: &mut [Item],
    usage: &[UsageRow],
    ratings: &HashMap<(String, String), f64>,
) {
    let by_handle: HashMap<&str, &UsageRow> =
        usage.iter().map(|u| (u.handle.as_str(), u)).collect();
    for it in items {
        let u = by_handle.get(it.handle.as_str());
        it.opens = u.map(|u| u.opens).unwrap_or(0);
        it.last_opened = u.and_then(|u| u.last_opened.clone());
        it.rating = ratings.get(&rating_target(it)).copied();
    }
}

/// Count an opening in memory (the app also writes it to the library).
pub fn note_open(items: &mut [Item], handle: &str, when: &str) {
    if let Some(it) = items.iter_mut().find(|i| i.handle == handle) {
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
            .then(x.handle.cmp(&y.handle))
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
        y.last_opened
            .cmp(&x.last_opened)
            .then(x.handle.cmp(&y.handle))
    });
    v.truncate(n);
    v
}

/// `n` distinct entries drawn from the whole menu with this seed (a partial Fisher-Yates
/// shuffle over the entries in handle order, so the draw does not depend on the menu's order).
pub fn random(items: &[Item], seed: u64, n: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..items.len()).collect();
    idx.sort_by(|&a, &b| items[a].handle.cmp(&items[b].handle));
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
    if it.source == QsSource::Builtin {
        return if it.featured { 0 } else { 6 };
    }
    match it.behaviour.as_str() {
        "Quick deaths" | "Frozen at once" | "Static" | "Quiet rows" => 4,
        "Ash & debris" | "Long fades" => 2,
        _ => 0,
    }
}

/// Assign [`Item::shuffle_rank`], the [`Sort::Featured`] order: the best [`BAND`] entries of every
/// category shuffled together, then the next band, ..., so the top of the screen is a mix of the
/// highest-scoring entries across all categories, different for each seed. Promotions rank with
/// the best bands, and so do all the featured built-ins (a hand-picked set, not ranked by score);
/// dull categories wait a few bands, and the other built-ins after the rest. Atlas and pack
/// entries are grouped by category; promotions, featured built-ins and the other built-ins each
/// form one group.
pub fn shuffle(items: &mut [Item], seed: u64) {
    let mut by_cat: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, it) in items.iter().enumerate() {
        let group = match it.source {
            QsSource::Promotion => "\u{0}my finds",
            QsSource::Builtin if it.featured => "\u{0}featured",
            QsSource::Builtin => "\u{0}classics",
            QsSource::Atlas | QsSource::Pack => it.behaviour.as_str(),
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
                .then(items[a].handle.cmp(&items[b].handle))
        });
        for (pos, i) in list.into_iter().enumerate() {
            let it = &items[i];
            let band = if it.source == QsSource::Builtin && it.featured {
                0
            } else {
                pos / BAND
            };
            keys.push((band + band_penalty(it), r.next_u64(), i));
        }
    }
    keys.sort_unstable_by_key(|k| (k.0, k.1));
    for (rank, (_, _, i)) in keys.into_iter().enumerate() {
        items[i].shuffle_rank = rank as u32;
    }
}

/// Keeps the [`Sort::Featured`] order of a menu loaded again (`items`, already [`shuffle`]d) as it
/// was (`before`), so it stays the same for the whole session: the entries both have keep their
/// order among themselves, in the places they take in the new shuffle, and new entries keep their
/// new places. The menu reloads whenever the library's Quick Start rows change (an entry hidden or
/// added), and each shuffle draws its numbers in turn, so one entry more or less moved every
/// card after it.
pub fn keep_order(items: &mut [Item], before: &[Item]) {
    let old: HashMap<&str, u32> = before
        .iter()
        .map(|i| (i.handle.as_str(), i.shuffle_rank))
        .collect();
    let mut kept: Vec<(u32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(k, i)| old.get(i.handle.as_str()).map(|&r| (r, k)))
        .collect();
    let mut places: Vec<u32> = kept.iter().map(|&(_, k)| items[k].shuffle_rank).collect();
    places.sort_unstable();
    kept.sort_unstable();
    for ((_, k), place) in kept.into_iter().zip(places) {
        items[k].shuffle_rank = place;
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
    pub source: Option<QsSource>,
}

impl Filter {
    pub fn matches(&self, it: &Item) -> bool {
        (self.grid.is_empty() || it.grid_group == self.grid)
            && (self.family.is_empty() || it.family == self.family)
            && (self.behaviour.is_empty() || it.behaviour == self.behaviour)
            && self.source.is_none_or(|o| it.source == o)
            && (self.text.trim().is_empty() || it.matches(&self.text))
    }
    pub fn is_empty(&self) -> bool {
        *self == Filter::default()
    }
}

/// The entries matching a filter, in order.
pub fn select(items: &[Item], f: &Filter, sort: Sort) -> Vec<usize> {
    let mut v: Vec<usize> = (0..items.len()).filter(|&i| f.matches(&items[i])).collect();
    let tie = |a: usize, b: usize| items[a].handle.cmp(&items[b].handle);
    match sort {
        Sort::Featured => v.sort_unstable_by_key(|&i| items[i].shuffle_rank),
        Sort::Score => v.sort_unstable_by(|&a, &b| {
            cmp_f(items[a].score, items[b].score)
                .then(items[a].shuffle_rank.cmp(&items[b].shuffle_rank))
                .then(tie(a, b))
        }),
        Sort::Rule => v.sort_unstable_by(|&a, &b| {
            items[a]
                .rule
                .cmp(&items[b].rule)
                .then(cmp_f(items[a].score, items[b].score))
                .then(tie(a, b))
        }),
        Sort::Tiling => v.sort_unstable_by(|&a, &b| {
            items[a]
                .tiling
                .cmp(&items[b].tiling)
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
    pub sources: Vec<(QsSource, usize)>,
}

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
    let mut sources: HashMap<QsSource, usize> = HashMap::new();
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
        let o = f.source.is_none_or(|o| it.source == o);
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
            *sources.entry(it.source).or_default() += 1;
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
        sources: QsSource::ALL
            .iter()
            .filter_map(|o| sources.get(o).map(|n| (*o, *n)))
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
// Ratings and hiding

/// Rate an entry as the user, 1 to 5 (see [`rating_target`]). Returns the target rated.
pub fn rate(lib: &Library, it: &Item, stars: u8) -> Result<(String, String), String> {
    if !(1..=5).contains(&stars) {
        return Err("ratings are 1 to 5".into());
    }
    let (kind, id) = rating_target(it);
    match kind.as_str() {
        "pattern" => lib.rate_pattern(&id, stars, "").map(|_| ())?,
        _ => lib.rate(&kind, &id, "user", stars as f64, "").map(|_| ())?,
    }
    Ok((kind, id))
}

/// Rate an entry by handle (reading it from the library), 1 to 5.
pub fn rate_handle(lib: &Library, handle: &str, stars: u8) -> Result<(String, String), String> {
    let q = lib.get_quickstart(handle)?;
    rate(lib, &item(MenuRow::of(&q), 0), stars)
}

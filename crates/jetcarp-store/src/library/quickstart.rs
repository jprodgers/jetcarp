//! The `quickstart` table: every Quick Start entry the app can open, from four sources
//! ([`QsSource`]), in one table.
//!
//! | Source | How rows get in | `id` | `handle` | `source_ref` |
//! | --- | --- | --- | --- | --- |
//! | `builtin` | written from the build's entries ([`crate::quickstart::builtin_rows`]) whenever `meta.builtin_version` is not the build's ([`sync_builtins_in`], on opening) | the built-in id | the same (`life`, `rule:B3/S23`, `jaca:...`) | the reference a named rule was checked against |
//! | `pack` | a pack's import | the pack id (`qs-...`) | the same | the pack's manifest hash |
//! | `atlas` | `quickstart.publish` from an explore search ([`Library::publish_atlas`]) | `atlas-<8 hex of the search>-<run id>` | `atlas:<8 hex>-<run id>` | the search's uuid |
//! | `promotion` | `quickstart.add` (a pattern, an ant run, a sweep or search run, a scene) | the pattern's or ant's id, `sweep-<hash>`, ... | `find:<id>` | empty |
//!
//! A row's `handle` is what the app, rotations, profiles and front ends name it by, and what
//! `quickstart_usage` counts; ratings, media and provenance name its `id`. `hidden` leaves it
//! out of the menu (and of packs and the website); hiding a built-in survives the next write of
//! the built-ins. Besides its `scene`, a row keeps what the menu shows without reading the scene:
//! its `category`, `grid_group` and `family` (the menu's names), `tiling_name`, `tiling_key`,
//! `rule`, `score`, `rank` (its order within its source) and `featured`; an atlas row also keeps
//! the small `preview` scene its card runs, and the `mature` state (deflated) and generation the
//! card starts from.

use super::{Library, Origin, e, now_utc, provenance, schema, short_tiling_key};
use jetcarp_core::pattern::range::rule_family;
use jetcarp_core::rules::Rule;
use jetcarp_core::world::Scene;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Where a Quick Start entry comes from.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum QsSource {
    /// Built into JETCARP.
    Builtin,
    /// Imported from a pack.
    Pack,
    /// Published from an explore search.
    Atlas,
    /// Added from the library: a pattern, an ant run, a sweep or search run, a scene.
    #[default]
    Promotion,
}

impl QsSource {
    pub const ALL: [QsSource; 4] = [
        QsSource::Promotion,
        QsSource::Pack,
        QsSource::Atlas,
        QsSource::Builtin,
    ];
    /// `builtin`, `pack`, `atlas`, `promotion`.
    pub fn name(self) -> &'static str {
        match self {
            QsSource::Builtin => "builtin",
            QsSource::Pack => "pack",
            QsSource::Atlas => "atlas",
            QsSource::Promotion => "promotion",
        }
    }
    pub fn parse(s: &str) -> Result<QsSource, String> {
        let s = s.trim().to_ascii_lowercase();
        QsSource::ALL
            .into_iter()
            .find(|x| x.name() == s)
            .ok_or_else(|| format!("unknown source '{s}' (builtin, pack, atlas, promotion)"))
    }
    /// What the menu calls it: the badge on a card and the sidebar's Source list. Built-in entries
    /// are "Built-in", not "Classics", which is also a category (a sandpile card said "Classics").
    pub fn label(self) -> &'static str {
        match self {
            QsSource::Builtin => "Built-in",
            QsSource::Pack => "Packs",
            QsSource::Atlas => "Atlas",
            QsSource::Promotion => "My finds",
        }
    }

    /// The rule family an entry whose rule has none is listed under (as it always has been).
    fn family_fallback(self) -> &'static str {
        match self {
            QsSource::Builtin => "Classics",
            other => other.label(),
        }
    }
}

/// What a Quick Start entry was made from (`target_kind`).
pub const TARGET_KINDS: [&str; 5] = ["pattern", "ant", "sweep_run", "search_run", "scene"];

/// One Quick Start entry (table `quickstart`).
#[derive(Clone, Debug, Serialize)]
pub struct QuickStartRow {
    /// See the module notes: a pattern's id, `sweep-<hash>`, `qs-<hex>`, `atlas-...`, a built-in id.
    pub id: String,
    /// The name it is opened by: `find:<id>`, `atlas:...`, `qs-<hex>`, a built-in id.
    pub handle: String,
    pub source: QsSource,
    /// A pack's manifest hash, a search's uuid, or a built-in rule's reference.
    pub source_ref: String,
    /// `pattern`, `ant`, `sweep_run`, `search_run`, or `scene`.
    pub target_kind: String,
    /// The pattern's or ant's id, `<search uuid>:<run id>`, ... (none for a scene).
    pub target_id: Option<String>,
    pub title: String,
    pub description: String,
    /// The menu's category (`Gliders & ships`, `Classics`, `My finds`, ...).
    pub category: String,
    /// The menu's grid group (`Squares`, `Archimedean`, ...).
    pub grid_group: String,
    /// The menu's rule family (`Life-like`, `Generations`, ...).
    pub family: String,
    /// The tiling's short key (`square`, `k3-019`, `ih28`; an atlas row's as its search names it).
    pub tiling_key: String,
    /// The tiling's display name.
    pub tiling_name: String,
    /// Main rule notation and its family (`lifelike`, `generations`, ...).
    pub rule: String,
    pub rule_family: String,
    /// `still life`, `oscillator`, `spaceship`, an ant outcome, an atlas category (free text).
    pub behaviour: String,
    pub scene: Scene,
    /// The small scene a card runs (atlas rows), when it is not the scene made smaller.
    pub preview: Option<Scene>,
    /// The generation of `mature`.
    pub mature_gen: u32,
    /// The preview's state at `mature_gen`, deflated: a card starts there.
    #[serde(skip)]
    pub mature: Option<Vec<u8>>,
    pub score: Option<f64>,
    /// Order within its source (built-ins as built, atlas rows best first).
    pub rank: i64,
    /// A featured built-in: a hand-made scene, or one picked for the starter set.
    pub featured: bool,
    pub hidden: bool,
    pub created: String,
    pub updated: String,
}

impl QuickStartRow {
    /// A promotion's row for a scene (handle `find:<id>`), made from a `target_kind`
    /// ([`TARGET_KINDS`]), with the rule, the tiling and the menu's names read from the scene.
    pub fn for_scene(id: &str, target_kind: &str, scene: Scene) -> QuickStartRow {
        let now = now_utc();
        let mut q = QuickStartRow {
            id: id.to_string(),
            handle: format!("find:{id}"),
            source: QsSource::Promotion,
            source_ref: String::new(),
            target_kind: target_kind.to_string(),
            target_id: None,
            title: scene.name.clone(),
            description: scene.description.clone(),
            category: String::new(),
            grid_group: String::new(),
            family: String::new(),
            tiling_key: String::new(),
            tiling_name: String::new(),
            rule: String::new(),
            rule_family: String::new(),
            behaviour: String::new(),
            scene,
            preview: None,
            mature_gen: 0,
            mature: None,
            score: None,
            rank: 0,
            featured: false,
            hidden: false,
            created: now.clone(),
            updated: now,
        };
        q.complete();
        q
    }

    /// Fill the columns left empty from the scene, the source and the behaviour: the rule and
    /// its family, the tiling's key and name, and the menu's category, grid group and family.
    pub fn complete(&mut self) {
        if self.rule.is_empty()
            && let Some(c) = self.scene.cas.first()
        {
            self.rule = c.rule.clone();
        }
        if self.rule_family.is_empty() {
            let first = self.scene.cas.first().map(|c| c.rule.as_str());
            self.rule_family = first
                .or(Some(self.rule.as_str()))
                .and_then(|r| Rule::parse(r).ok())
                .map(|(r, _)| rule_family(&r).to_string())
                .unwrap_or_default();
        }
        if self.tiling_key.is_empty() {
            self.tiling_key = short_tiling_key(&self.scene.tiling);
        }
        if self.tiling_name.is_empty() {
            self.tiling_name = jetcarp_core::pattern::tiling_display_name(&self.scene.tiling);
        }
        if self.grid_group.is_empty() {
            self.grid_group = crate::quickstart::grid_group(&self.scene.tiling).to_string();
        }
        if self.family.is_empty() {
            self.family =
                crate::quickstart::family_of_rule(&self.rule, self.source.family_fallback());
        }
        if self.category.is_empty() {
            self.category = crate::quickstart::category_of(self.source, &self.behaviour);
        }
    }
}

/// Every column, in the order [`row_from`] reads them.
const QCOLS: &str = "id, handle, source, source_ref, target_kind, target_id, title, description,
    category, grid_group, family, tiling_key, tiling_name, rule, rule_family, behaviour, scene,
    preview, mature_gen, mature, score, rank, featured, hidden, created, updated";

/// A row, or `None` when its scene no longer reads (it is left out rather than failing a list).
fn row_from(r: &rusqlite::Row) -> rusqlite::Result<Option<QuickStartRow>> {
    let source: String = r.get(2)?;
    let scene: String = r.get(16)?;
    let preview: Option<String> = r.get(17)?;
    let Ok(scene) = Scene::from_json(&scene) else {
        return Ok(None);
    };
    let Ok(source) = QsSource::parse(&source) else {
        return Ok(None);
    };
    Ok(Some(QuickStartRow {
        id: r.get(0)?,
        handle: r.get(1)?,
        source,
        source_ref: r.get(3)?,
        target_kind: r.get(4)?,
        target_id: r.get(5)?,
        title: r.get(6)?,
        description: r.get(7)?,
        category: r.get(8)?,
        grid_group: r.get(9)?,
        family: r.get(10)?,
        tiling_key: r.get(11)?,
        tiling_name: r.get(12)?,
        rule: r.get(13)?,
        rule_family: r.get(14)?,
        behaviour: r.get(15)?,
        scene,
        preview: preview.and_then(|p| Scene::from_json(&p).ok()),
        mature_gen: r.get::<_, i64>(18)? as u32,
        mature: r.get(19)?,
        score: r.get(20)?,
        rank: r.get(21)?,
        featured: r.get::<_, i64>(22)? != 0,
        hidden: r.get::<_, i64>(23)? != 0,
        created: r.get(24)?,
        updated: r.get(25)?,
    }))
}

/// What the menu lists of an entry: everything but its scenes and mature state, so a menu of
/// thousands of entries holds no scene in memory ([`crate::quickstart::scene`] and
/// [`crate::quickstart::card`] read those by handle).
#[derive(Clone, Debug, PartialEq)]
pub struct MenuRow {
    pub id: String,
    pub handle: String,
    pub source: QsSource,
    pub source_ref: String,
    pub target_kind: String,
    pub target_id: Option<String>,
    pub title: String,
    pub description: String,
    pub category: String,
    pub grid_group: String,
    pub family: String,
    pub tiling_key: String,
    pub tiling_name: String,
    pub rule: String,
    pub score: Option<f64>,
    pub rank: i64,
    pub featured: bool,
    pub has_preview: bool,
    pub created: String,
}

impl MenuRow {
    /// What the menu lists of a whole row.
    pub fn of(q: &QuickStartRow) -> MenuRow {
        MenuRow {
            id: q.id.clone(),
            handle: q.handle.clone(),
            source: q.source,
            source_ref: q.source_ref.clone(),
            target_kind: q.target_kind.clone(),
            target_id: q.target_id.clone(),
            title: q.title.clone(),
            description: q.description.clone(),
            category: q.category.clone(),
            grid_group: q.grid_group.clone(),
            family: q.family.clone(),
            tiling_key: q.tiling_key.clone(),
            tiling_name: q.tiling_name.clone(),
            rule: q.rule.clone(),
            score: q.score,
            rank: q.rank,
            featured: q.featured,
            has_preview: q.preview.is_some(),
            created: q.created.clone(),
        }
    }
}

/// The stored key of an entry named by its handle, its id, or `find:<id>`.
fn id_of(c: &Connection, key: &str) -> Result<Option<String>, String> {
    let key = key.trim();
    let bare = key.strip_prefix("find:").unwrap_or(key);
    c.prepare_cached(
        "SELECT id FROM quickstart WHERE handle = ?1 OR id = ?1 OR id = ?2
         ORDER BY handle = ?1 DESC, id = ?1 DESC LIMIT 1",
    )
    .map_err(e)?
    .query_row(params![key, bare], |r| r.get(0))
    .optional()
    .map_err(e)
}

/// Insert or replace a row (keeping its creation date; with `keep_hidden`, its `hidden` too),
/// inside a transaction. A row of another source with the same id is left alone and an error
/// returned. A new row of any source but `builtin` gets its provenance row 0: found (an atlas
/// row's by `job`, the search that found it) or as `origin` says.
pub(super) fn quickstart_in(
    tx: &Transaction,
    q: &QuickStartRow,
    keep_hidden: bool,
    job: i64,
    origin: &Origin,
) -> Result<(), String> {
    let (pin_key, pin) = schema::pin_for(tx, &q.scene.tiling)?;
    let had: Option<String> = tx
        .prepare_cached("SELECT source FROM quickstart WHERE id = ?1")
        .map_err(e)?
        .query_row([&q.id], |r| r.get(0))
        .optional()
        .map_err(e)?;
    if let Some(s) = &had
        && s != q.source.name()
    {
        return Err(format!(
            "Quick Start entry {} is a {s} entry, not a {} one",
            q.id,
            q.source.name()
        ));
    }
    let hidden = if keep_hidden {
        "hidden"
    } else {
        "excluded.hidden"
    };
    tx.prepare_cached(&format!(
        "INSERT INTO quickstart (id, handle, source, source_ref, target_kind, target_id, title,
            description, category, grid_group, family, tiling_key, tiling_name, rule,
            rule_family, behaviour, scene, preview, mature_gen, mature, score, rank, featured,
            hidden, pin_key, geometry_pin, needs_reverify, engine_version, created, updated)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
            ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, 0, ?27, ?28, ?29)
         ON CONFLICT (id) DO UPDATE SET handle = excluded.handle,
            source_ref = excluded.source_ref, target_kind = excluded.target_kind,
            target_id = excluded.target_id, title = excluded.title,
            description = excluded.description, category = excluded.category,
            grid_group = excluded.grid_group, family = excluded.family,
            tiling_key = excluded.tiling_key, tiling_name = excluded.tiling_name,
            rule = excluded.rule, rule_family = excluded.rule_family,
            behaviour = excluded.behaviour, scene = excluded.scene, preview = excluded.preview,
            mature_gen = excluded.mature_gen, mature = excluded.mature, score = excluded.score,
            rank = excluded.rank, featured = excluded.featured, hidden = {hidden},
            pin_key = excluded.pin_key, geometry_pin = excluded.geometry_pin, needs_reverify = 0,
            engine_version = excluded.engine_version, updated = excluded.updated"
    ))
    .map_err(e)?
    .execute(params![
        q.id,
        q.handle,
        q.source.name(),
        q.source_ref,
        q.target_kind,
        q.target_id,
        q.title,
        q.description,
        q.category,
        q.grid_group,
        q.family,
        q.tiling_key,
        q.tiling_name,
        q.rule,
        q.rule_family,
        q.behaviour,
        q.scene.to_json(),
        q.preview.as_ref().map(|p| p.to_json()),
        q.mature_gen as i64,
        q.mature,
        q.score,
        q.rank,
        q.featured as i64,
        q.hidden as i64,
        pin_key,
        pin,
        jetcarp_core::VERSION,
        q.created,
        now_utc(),
    ])
    .map_err(e)?;
    if had.is_none() && q.source != QsSource::Builtin {
        let source = match (q.source, &q.target_id) {
            (QsSource::Atlas, Some(t)) => format!("{}:{t}", q.target_kind),
            _ => q.target_kind.clone(),
        };
        provenance::first_in(
            tx,
            "quickstart",
            &q.id,
            origin,
            job,
            &pin,
            &source,
            &q.created,
            "",
        )?;
    }
    Ok(())
}

/// Delete rows and their media rows and provenance (the media files stay until
/// `jetcarp media gc`).
fn delete_in(tx: &Transaction, ids: &[String]) -> Result<(), String> {
    for id in ids {
        tx.execute("DELETE FROM quickstart WHERE id = ?1", [id])
            .map_err(e)?;
        tx.execute(
            "DELETE FROM media WHERE target_kind = 'quickstart' AND target_id = ?1",
            [id],
        )
        .map_err(e)?;
        provenance::forget_in(tx, "quickstart", id)?;
    }
    Ok(())
}

/// What writing the built-ins did.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct BuiltinSync {
    /// The build's built-in version (now `meta.builtin_version`).
    pub version: String,
    /// Rows written (added or brought up to date).
    pub written: usize,
    /// Built-in rows of an older build that this one does not have, deleted.
    pub removed: usize,
}

/// Write the build's built-in entries when `meta.builtin_version` is not the build's
/// ([`crate::quickstart::builtin_version`]): every built-in row brought up to date (a hidden one
/// stays hidden), the rows of built-ins this build does not have deleted. `None` when they were
/// up to date.
pub(super) fn sync_builtins_in(tx: &Transaction) -> Result<Option<BuiltinSync>, String> {
    let want = crate::quickstart::builtin_version();
    let have: Option<String> = tx
        .query_row(
            "SELECT value FROM meta WHERE key = 'builtin_version'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(e)?;
    if have.as_deref() == Some(want) {
        return Ok(None);
    }
    let old: HashSet<String> = {
        let mut st = tx
            .prepare("SELECT id FROM quickstart WHERE source = 'builtin'")
            .map_err(e)?;
        st.query_map([], |r| r.get(0))
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)?
    };
    let mut out = BuiltinSync {
        version: want.to_string(),
        ..Default::default()
    };
    let rows = crate::quickstart::builtin_rows();
    for q in rows {
        // A row of another source under a built-in's id or handle keeps it (only a hand-made
        // library could have one).
        let clash: bool = tx
            .prepare_cached(
                "SELECT 1 FROM quickstart WHERE (id = ?1 OR handle = ?2) AND source != 'builtin'",
            )
            .map_err(e)?
            .exists(params![q.id, q.handle])
            .map_err(e)?;
        if clash {
            continue;
        }
        quickstart_in(tx, q, true, 0, &Origin::found())?;
        out.written += 1;
    }
    let keep: HashSet<&str> = rows.iter().map(|q| q.id.as_str()).collect();
    let gone: Vec<String> = old
        .into_iter()
        .filter(|id| !keep.contains(id.as_str()))
        .collect();
    out.removed = gone.len();
    delete_in(tx, &gone)?;
    tx.execute(
        "INSERT INTO meta (key, value) VALUES ('builtin_version', ?1)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [want],
    )
    .map_err(e)?;
    Ok(Some(out))
}

/// What publishing an atlas did.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct AtlasWrite {
    /// Rows new to the table.
    pub added: usize,
    /// Rows it had, written again (a hidden one stays hidden).
    pub updated: usize,
    /// The search's earlier rows no longer chosen, deleted (hidden ones are kept, hidden).
    pub removed: usize,
}

impl Library {
    /// Insert or replace a Quick Start entry (keeping its creation date). A new entry gets its
    /// provenance row 0 (found here); a built-in, none.
    pub fn put_quickstart(&self, q: &QuickStartRow) -> Result<(), String> {
        let mut q = q.clone();
        q.complete();
        self.write(|tx| quickstart_in(tx, &q, false, 0, &Origin::found()))
    }

    /// Quick Start entries of these sources (every source when empty), hidden ones too, oldest
    /// first.
    pub fn quickstart(&self, sources: &[QsSource]) -> Result<Vec<QuickStartRow>, String> {
        let filter = if sources.is_empty() {
            String::new()
        } else {
            format!(
                "WHERE source IN ({})",
                sources
                    .iter()
                    .map(|s| format!("'{}'", s.name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        self.read(vec![], |c| {
            let mut st = c
                .prepare(&format!(
                    "SELECT {QCOLS} FROM quickstart {filter} ORDER BY created, id"
                ))
                .map_err(e)?;
            let rows = st
                .query_map([], row_from)
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)?;
            Ok(rows.into_iter().flatten().collect())
        })
    }

    /// One Quick Start entry by its handle, its id, or `find:<id>`.
    pub fn get_quickstart(&self, key: &str) -> Result<QuickStartRow, String> {
        let missing = || format!("no Quick Start entry '{}'", key.trim());
        self.read(None, |c| {
            let Some(id) = id_of(c, key)? else {
                return Ok(None);
            };
            c.prepare_cached(&format!("SELECT {QCOLS} FROM quickstart WHERE id = ?1"))
                .map_err(e)?
                .query_row([&id], row_from)
                .optional()
                .map(Option::flatten)
                .map_err(e)
        })?
        .ok_or_else(missing)
    }

    /// Hide an entry from the menu (and from packs and the website), or show it again.
    pub fn set_quickstart_hidden(&self, key: &str, hidden: bool) -> Result<QuickStartRow, String> {
        let missing = || format!("no Quick Start entry '{}'", key.trim());
        self.write(|tx| {
            let id = id_of(tx, key)?.ok_or_else(missing)?;
            tx.execute(
                "UPDATE quickstart SET hidden = ?2, updated = ?3 WHERE id = ?1",
                params![id, hidden as i64, now_utc()],
            )
            .map_err(e)?;
            Ok(())
        })?;
        self.get_quickstart(key)
    }

    /// Delete a Quick Start entry and its media rows (the files stay until `jetcarp media gc`).
    /// A built-in comes back with the next build's built-ins; hide it instead.
    pub fn remove_quickstart(&self, key: &str) -> Result<(), String> {
        self.write(|tx| match id_of(tx, key)? {
            Some(id) => delete_in(tx, &[id]),
            None => Ok(()),
        })
    }

    /// Write the build's built-in entries if the library's are another build's (opening a
    /// library does this too; this makes the file when there is none yet).
    pub fn sync_builtins(&self) -> Result<Option<BuiltinSync>, String> {
        self.write(sync_builtins_in)
    }

    /// The rows of an atlas published from the explore search `job` (whose uuid is `uuid`), in
    /// one transaction: every row written (a new one with its provenance: found by that search),
    /// and the search's earlier rows not among them deleted, unless hidden.
    pub fn publish_atlas(
        &self,
        job: i64,
        uuid: &str,
        rows: &[QuickStartRow],
    ) -> Result<AtlasWrite, String> {
        self.write(|tx| {
            let old: Vec<(String, bool)> = {
                let mut st = tx
                    .prepare(
                        "SELECT id, hidden FROM quickstart WHERE source = 'atlas'
                         AND source_ref = ?1",
                    )
                    .map_err(e)?;
                st.query_map([uuid], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)))
                    .map_err(e)?
                    .collect::<Result<_, _>>()
                    .map_err(e)?
            };
            let had: HashSet<&str> = old.iter().map(|x| x.0.as_str()).collect();
            let mut out = AtlasWrite::default();
            for q in rows {
                if q.source != QsSource::Atlas || q.source_ref != uuid {
                    return Err(format!("atlas row {} is not from the search {uuid}", q.id));
                }
                quickstart_in(tx, q, true, job, &Origin::found())?;
                if had.contains(q.id.as_str()) {
                    out.updated += 1;
                } else {
                    out.added += 1;
                }
            }
            let now: HashSet<&str> = rows.iter().map(|q| q.id.as_str()).collect();
            let gone: Vec<String> = old
                .iter()
                .filter(|(id, hidden)| !hidden && !now.contains(id.as_str()))
                .map(|x| x.0.clone())
                .collect();
            out.removed = gone.len();
            delete_in(tx, &gone)?;
            Ok(out)
        })
    }

    /// What the menu lists of every shown entry ([`MenuRow`]), by source and rank.
    pub fn quickstart_menu_rows(&self) -> Result<Vec<MenuRow>, String> {
        self.read(vec![], |c| {
            let mut st = c
                .prepare(
                    "SELECT id, handle, source, source_ref, target_kind, target_id, title,
                        description, category, grid_group, family, tiling_key, tiling_name, rule,
                        score, rank, featured, preview IS NOT NULL, created
                     FROM quickstart WHERE hidden = 0 ORDER BY rank, created, id",
                )
                .map_err(e)?;
            let rows = st
                .query_map([], |r| {
                    let source: String = r.get(2)?;
                    Ok(QsSource::parse(&source).ok().map(|source| MenuRow {
                        id: r.get(0).unwrap_or_default(),
                        handle: r.get(1).unwrap_or_default(),
                        source,
                        source_ref: r.get(3).unwrap_or_default(),
                        target_kind: r.get(4).unwrap_or_default(),
                        target_id: r.get(5).unwrap_or_default(),
                        title: r.get(6).unwrap_or_default(),
                        description: r.get(7).unwrap_or_default(),
                        category: r.get(8).unwrap_or_default(),
                        grid_group: r.get(9).unwrap_or_default(),
                        family: r.get(10).unwrap_or_default(),
                        tiling_key: r.get(11).unwrap_or_default(),
                        tiling_name: r.get(12).unwrap_or_default(),
                        rule: r.get(13).unwrap_or_default(),
                        score: r.get(14).unwrap_or_default(),
                        rank: r.get(15).unwrap_or_default(),
                        featured: r.get::<_, i64>(16).unwrap_or(0) != 0,
                        has_preview: r.get(17).unwrap_or(false),
                        created: r.get(18).unwrap_or_default(),
                    }))
                })
                .map_err(e)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(e)?;
            Ok(rows.into_iter().flatten().collect())
        })
    }

    /// An entry's scene, by handle (or id), without reading the rest of the row.
    pub fn quickstart_scene(&self, key: &str) -> Result<Scene, String> {
        let missing = || format!("no Quick Start entry '{}'", key.trim());
        let text: String = self
            .read(None, |c| {
                let Some(id) = id_of(c, key)? else {
                    return Ok(None);
                };
                c.prepare_cached("SELECT scene FROM quickstart WHERE id = ?1")
                    .map_err(e)?
                    .query_row([&id], |r| r.get(0))
                    .optional()
                    .map_err(e)
            })?
            .ok_or_else(missing)?;
        Scene::from_json(&text)
    }

    /// What an entry's card starts from, by handle (or id): its scene, its own preview scene if
    /// it has one, and the mature state (deflated) with its generation.
    #[allow(clippy::type_complexity)]
    pub fn quickstart_card(
        &self,
        key: &str,
    ) -> Result<(Scene, Option<Scene>, u32, Option<Vec<u8>>), String> {
        let missing = || format!("no Quick Start entry '{}'", key.trim());
        let (scene, preview, generation, mature): (String, Option<String>, i64, Option<Vec<u8>>) =
            self.read(None, |c| {
                let Some(id) = id_of(c, key)? else {
                    return Ok(None);
                };
                c.prepare_cached(
                    "SELECT scene, preview, mature_gen, mature FROM quickstart WHERE id = ?1",
                )
                .map_err(e)?
                .query_row([&id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .optional()
                .map_err(e)
            })?
            .ok_or_else(missing)?;
        let preview = match preview {
            Some(p) => Some(Scene::from_json(&p)?),
            None => None,
        };
        Ok((
            Scene::from_json(&scene)?,
            preview,
            generation as u32,
            mature,
        ))
    }

    /// Changes whenever a Quick Start entry is added, changed, hidden, or removed (for the app's
    /// refresh check).
    pub fn quickstart_stamp(&self) -> String {
        self.read(String::new(), |c| {
            c.query_row(
                "SELECT count(*) || '|' || coalesce(max(updated), '') FROM quickstart",
                [],
                |r| r.get::<_, String>(0),
            )
            .map_err(e)
        })
        .unwrap_or_default()
    }

    /// How many Quick Start entries each source has (`builtin`, `pack`, `atlas`, `promotion`),
    /// hidden ones included; sources with none are left out.
    pub fn quickstart_counts(&self) -> Result<std::collections::BTreeMap<String, u64>, String> {
        self.read(Default::default(), |c| {
            let mut st = c
                .prepare("SELECT source, count(*) FROM quickstart GROUP BY source")
                .map_err(e)?;
            st.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })
            .map_err(e)?
            .collect::<Result<_, _>>()
            .map_err(e)
        })
    }

    /// Count an opening of a Quick Start entry, by its handle.
    pub fn note_quickstart_open(&self, handle: &str) -> Result<(), String> {
        let now = now_utc();
        self.write(|tx| {
            tx.execute(
                "INSERT INTO quickstart_usage (handle, opens, first_opened, last_opened)
                 VALUES (?1, 1, ?2, ?2)
                 ON CONFLICT (handle) DO UPDATE SET opens = opens + 1, last_opened = ?2",
                params![handle, now],
            )
            .map_err(e)?;
            Ok(())
        })
    }
}

/// Schema 2's Quick Start rows as schema 3 keeps them (the migration [`schema::migrate_2_3`]):
/// a row with a pack id (`qs-...`) came from a pack (its `source_ref` the pack its provenance
/// names), every other one is a promotion (handle `find:<id>`); `sweep` becomes `sweep_run`, an
/// ant's scene `ant`; `candidate` and `rejected` entries are hidden. Pins, flags and dates are
/// kept as they were.
pub(super) fn migrate_rows_in(tx: &Transaction) -> Result<usize, String> {
    #[allow(clippy::type_complexity)]
    let old: Vec<(
        (String, String, Option<String>, String, String, String),
        (String, String, Option<f64>, String),
        (String, String, i64, String, String, String),
    )> = {
        let mut st = tx
            .prepare(
                "SELECT id, source_kind, source_id, title, description, behaviour, rule, scene,
                    score, status, pin_key, geometry_pin, needs_reverify, engine_version,
                    created, updated FROM quickstart",
            )
            .map_err(e)?;
        st.query_map([], |r| {
            Ok((
                (
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ),
                (r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?),
                (
                    r.get(10)?,
                    r.get(11)?,
                    r.get(12)?,
                    r.get(13)?,
                    r.get(14)?,
                    r.get(15)?,
                ),
            ))
        })
        .map_err(e)?
        .collect::<Result<_, _>>()
        .map_err(e)?
    };
    tx.execute_batch(&format!(
        "DROP INDEX IF EXISTS quickstart_status; DROP INDEX IF EXISTS quickstart_source;
         DROP INDEX IF EXISTS quickstart_pin; DROP TABLE quickstart; {}",
        schema::QUICKSTART_3
    ))
    .map_err(e)?;
    let n = old.len();
    for ((id, kind, source_id, title, description, behaviour), (rule, scene, score, status), p) in
        old
    {
        let pack = id.starts_with("qs-");
        let pack_hash: String = if pack {
            tx.query_row(
                "SELECT pack_hash FROM provenance WHERE target_kind = 'quickstart'
                 AND target_id = ?1 AND pack_hash != '' ORDER BY seq DESC LIMIT 1",
                [&id],
                |r| r.get(0),
            )
            .optional()
            .map_err(e)?
            .unwrap_or_default()
        } else {
            String::new()
        };
        let (target_kind, target_id) = match kind.as_str() {
            "pattern" => ("pattern", source_id),
            "sweep" => ("sweep_run", source_id),
            _ if id.starts_with("ant-") => ("ant", Some(id.clone())),
            _ => ("scene", source_id),
        };
        let source = if pack {
            QsSource::Pack
        } else {
            QsSource::Promotion
        };
        let handle = if pack {
            id.clone()
        } else {
            format!("find:{id}")
        };
        let hidden = matches!(status.as_str(), "candidate" | "rejected");
        // The menu's names, from the scene when it still reads.
        let mut q = match Scene::from_json(&scene) {
            Ok(sc) => {
                let mut q = QuickStartRow::for_scene(&id, target_kind, sc);
                q.source = source;
                q.category.clear();
                q.family.clear();
                q.rule = rule.clone();
                q.behaviour = behaviour.clone();
                q.complete();
                q
            }
            Err(_) => QuickStartRow::for_scene(
                &id,
                target_kind,
                Scene::new(jetcarp_core::topology::TilingSpec::new("square")),
            ),
        };
        q.rule = rule;
        let (pin_key, pin, flagged, engine, created, updated) = p;
        tx.execute(
            "INSERT INTO quickstart (id, handle, source, source_ref, target_kind, target_id,
                title, description, category, grid_group, family, tiling_key, tiling_name, rule,
                rule_family, behaviour, scene, score, hidden, pin_key, geometry_pin,
                needs_reverify, engine_version, created, updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
            params![
                id,
                handle,
                source.name(),
                pack_hash,
                target_kind,
                target_id,
                title,
                description,
                q.category,
                q.grid_group,
                q.family,
                q.tiling_key,
                q.tiling_name,
                q.rule,
                q.rule_family,
                behaviour,
                scene,
                score,
                hidden as i64,
                pin_key,
                pin,
                flagged,
                engine,
                created,
                updated
            ],
        )
        .map_err(e)?;
    }
    Ok(n)
}

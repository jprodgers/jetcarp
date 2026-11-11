//! The catalog's Quick Start entries: promoting a pattern to Quick Start, and the entries the
//! catalog holds ("My finds": promoted patterns and sweep runs). The built-in entries are the
//! engine's (`jetcarp_core::quickstart`); the menu that merges them with the atlas is
//! [`crate::qsmenu`].

use crate::{Library, QuickStartRow, Update};
use jetcarp_core::pattern::CurationState;
use jetcarp_core::quickstart::{MY_FINDS, QuickStart, entries, main_rule};
use jetcarp_core::world::Scene;

/// Save a Quick Start entry ("My finds") for the record in the catalog's `quickstart` table (id =
/// the pattern id; promoting again replaces it) and mark the pattern promoted. Returns the scene
/// and the entry id.
pub fn promote(
    lib: &Library,
    key: &str,
    title: Option<&str>,
    description: Option<&str>,
) -> Result<(Scene, String), String> {
    let rec = lib.get(key)?;
    let sc = jetcarp_core::pattern::promote::showcase_scene(&rec, title, description)?;
    let mut row = QuickStartRow::for_scene(&rec.id, "pattern", sc.clone());
    row.source_id = Some(rec.id.clone());
    row.grid = rec.tiling_name.clone();
    row.behaviour = rec.class.name().to_string();
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

/// The catalog's shown Quick Start entries (status `auto` or `approved`; promoted patterns and
/// sweep runs, "My finds"), oldest first, under the family "My finds" and marked featured, with
/// ids `find:<id>`. Candidates and rejected entries are left out.
pub fn user_finds(lib: &crate::Library) -> Vec<QuickStart> {
    use crate::QsStatus;
    let rows = lib
        .quickstart(&[QsStatus::Auto, QsStatus::Approved])
        .unwrap_or_default();
    rows.into_iter().map(|q| find_entry(lib, q)).collect()
}

/// A catalog Quick Start row as an entry (id `find:<id>`, family "My finds", featured).
pub fn find_entry(lib: &crate::Library, q: crate::QuickStartRow) -> QuickStart {
    let (rule, states, _) = main_rule(&q.scene);
    QuickStart {
        id: format!("find:{}", q.id),
        tiling: jetcarp_core::pattern::tiling_display_name(&q.scene.tiling),
        scene: q.scene,
        family: MY_FINDS.into(),
        states,
        featured: true,
        rule,
        source: Some(format!(
            "{} ({} {})",
            lib.path().display(),
            q.source_kind,
            q.id
        )),
    }
}

/// The shown finds in the default catalog (`JETCARP_LIBRARY`, else
/// `<data>/catalog.sqlite`) first, marked featured, then the built-in entries.
/// The app shows this list.
pub fn all_entries() -> Vec<QuickStart> {
    let mut v = user_finds(&crate::Library::open_default());
    v.extend(entries());
    v
}

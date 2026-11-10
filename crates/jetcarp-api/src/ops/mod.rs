//! The operations, by group, and the registry.
//!
//! | Group | Operations |
//! | --- | --- |
//! | [`system`] | `system.paths`, `system.doctor`, `system.uninstall` |
//! | [`reference`] | `reference.rules`, `reference.tilings`, `reference.isohedral`, `reference.palettes`, `reference.seeds` |
//! | [`files`] | `files.list` |
//! | [`scene`] | `scene.load`, `scene.save`, `scene.verify`, `scene.run` |
//! | [`world`] | `world.new`, `world.open`, `world.status`, `world.save`, `world.read`, `world.inspect`, `world.draw`, `world.paste`, `world.reseed`, `world.apply`, `world.step`, `world.render` |
//! | [`library`] | `library.info`, `library.query`, `library.get`, `library.update`, `library.remove`, `library.render`, `library.export`, `library.import`, `library.paste`, `library.verify`, `library.prune` |
//! | [`quickstart`] | `quickstart.list`, `quickstart.get`, `quickstart.add`, `quickstart.update`, `quickstart.remove`, `quickstart.check` |
//! | [`media`] | `media.render`, `media.list`, `site.export` |
//! | [`wallpaper`] | `wallpaper.export` |
//! | [`sheet`] | `sheet.rules`, `sheet.sweep`, `sheet.isohedral` |
//! | [`search`] | `search.start` |
//! | [`jobs`] | `jobs.list`, `jobs.status`, `jobs.stop`, `jobs.resume`, `jobs.remove`, `jobs.report` |

pub mod files;
pub mod jobs;
pub mod library;
pub mod media;
pub mod quickstart;
pub mod reference;
pub mod scene;
pub mod search;
pub mod sheet;
pub mod system;
pub mod wallpaper;
pub mod world;

use crate::op::{DynOp, OnWorld, Plain};

/// Every operation, in a fixed order.
pub static OPS: &[&dyn DynOp] = &[
    &Plain::<system::SystemPaths>::NEW,
    &Plain::<system::SystemDoctor>::NEW,
    &Plain::<system::SystemUninstall>::NEW,
    &Plain::<reference::ReferenceRules>::NEW,
    &Plain::<reference::ReferenceTilings>::NEW,
    &Plain::<reference::ReferenceIsohedral>::NEW,
    &Plain::<reference::ReferencePalettes>::NEW,
    &Plain::<reference::ReferenceSeeds>::NEW,
    &Plain::<files::FilesList>::NEW,
    &Plain::<scene::SceneLoad>::NEW,
    &Plain::<scene::SceneSave>::NEW,
    &Plain::<scene::SceneVerify>::NEW,
    &Plain::<scene::SceneRun>::NEW,
    &OnWorld::<world::WorldNew>::NEW,
    &OnWorld::<world::WorldOpen>::NEW,
    &OnWorld::<world::WorldStatusOp>::NEW,
    &OnWorld::<world::WorldSave>::NEW,
    &OnWorld::<world::WorldRead>::NEW,
    &OnWorld::<world::WorldInspect>::NEW,
    &OnWorld::<world::WorldDraw>::NEW,
    &OnWorld::<world::WorldPaste>::NEW,
    &OnWorld::<world::WorldReseed>::NEW,
    &OnWorld::<world::WorldApply>::NEW,
    &OnWorld::<world::WorldStep>::NEW,
    &OnWorld::<world::WorldRender>::NEW,
    &Plain::<library::LibraryInfoOp>::NEW,
    &Plain::<library::LibraryQuery>::NEW,
    &Plain::<library::LibraryGet>::NEW,
    &Plain::<library::LibraryUpdate>::NEW,
    &Plain::<library::LibraryRemove>::NEW,
    &Plain::<library::LibraryRender>::NEW,
    &Plain::<library::LibraryExport>::NEW,
    &Plain::<library::LibraryImport>::NEW,
    &Plain::<library::LibraryPaste>::NEW,
    &Plain::<library::LibraryVerify>::NEW,
    &Plain::<library::LibraryPrune>::NEW,
    &Plain::<quickstart::QuickstartList>::NEW,
    &Plain::<quickstart::QuickstartGet>::NEW,
    &Plain::<quickstart::QuickstartAdd>::NEW,
    &Plain::<quickstart::QuickstartUpdate>::NEW,
    &Plain::<quickstart::QuickstartRemove>::NEW,
    &Plain::<quickstart::QuickstartCheck>::NEW,
    &Plain::<media::MediaRender>::NEW,
    &Plain::<media::MediaListOp>::NEW,
    &Plain::<media::SiteExport>::NEW,
    &OnWorld::<wallpaper::WallpaperExport>::NEW,
    &Plain::<sheet::SheetRules>::NEW,
    &Plain::<sheet::SheetSweep>::NEW,
    &Plain::<sheet::SheetIsohedral>::NEW,
    &Plain::<search::SearchStart>::NEW,
    &Plain::<jobs::JobsList>::NEW,
    &Plain::<jobs::JobsStatus>::NEW,
    &Plain::<jobs::JobsStop>::NEW,
    &Plain::<jobs::JobsResume>::NEW,
    &Plain::<jobs::JobsRemove>::NEW,
    &Plain::<jobs::JobsReport>::NEW,
];

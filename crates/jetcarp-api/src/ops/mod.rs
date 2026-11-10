//! The operations, by group, and the registry.
//!
//! | Group | Operations |
//! | --- | --- |
//! | [`system`] | `system.paths`, `system.doctor`, `system.uninstall` |
//! | [`reference`] | `reference.rules`, `reference.tilings`, `reference.isohedral`, `reference.palettes`, `reference.seeds` |
//! | [`files`] | `files.list` |
//! | [`scene`] | `scene.load`, `scene.save`, `scene.verify`, `scene.run` |
//! | [`world`] | `world.new`, `world.open`, `world.status`, `world.save`, `world.read`, `world.inspect`, `world.draw`, `world.paste`, `world.reseed`, `world.apply`, `world.step`, `world.render` |
//!
//! The library, Quick Start, media, wallpaper, and site operations follow (refactor step 3b),
//! then the search and job operations (step 5).

pub mod files;
pub mod reference;
pub mod scene;
pub mod system;
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
];

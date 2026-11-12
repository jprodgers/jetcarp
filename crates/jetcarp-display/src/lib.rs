//! Everything a display needs to show a JETCARP world, without a window toolkit: the tile renderer
//! ([`gpu`]), choosing and colouring what a view shows ([`view`]), stepping on the GPU
//! ([`simgpu`]), and screensaver profiles ([`profiles`]). The app wraps these in egui; a wallpaper
//! or screensaver host draws to its own wgpu surface.
//!
//! **Feature `library`** (on by default): rotations find the Quick Start entries they name in the
//! user's library ([`stage::library_lookup`]), and the macOS saver can be installed
//! ([`saver::install`], which bakes those scenes into the bundle). The saver's own code is built
//! without it, so it links no store, job runner or SQLite.

pub mod autostart;
pub mod caption;
pub mod desktop_picture;
pub mod exit;
pub mod gnome;
pub mod gpu;
pub mod host_settings;
pub mod macsaver;
pub mod painter;
pub mod profiles;
pub mod reseed_presets;
pub mod rotation;
pub mod saver;
pub mod simgpu;
pub mod stage;
pub mod view;
pub mod winsaver;

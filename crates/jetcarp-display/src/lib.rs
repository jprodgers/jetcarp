//! Everything a display needs to show a JETCARP world, without a window toolkit: the tile renderer
//! ([`gpu`]), choosing and colouring what a view shows ([`view`]), stepping on the GPU
//! ([`simgpu`]), and screensaver profiles ([`profiles`]). The app wraps these in egui; a wallpaper
//! or screensaver host draws to its own wgpu surface.

pub mod autostart;
pub mod caption;
pub mod desktop_picture;
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
